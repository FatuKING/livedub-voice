//! [`Translator`] backed by OpenAI's `gpt-realtime-translate`.

use futures_util::future::BoxFuture;
use tokio::sync::mpsc;

use super::live::{
    AudioInput, DEFAULT_INPUT_QUEUE, InputReader, LiveSession, OUTPUT_QUEUE, Translator,
    TranslatorOutput, input_channel,
};
use super::session::{TranslatorSession, connect_to};
use super::{TRANSLATIONS_URL, TranslatorError};
use crate::credentials::ApiKey;
use crate::settings::Language;

/// Streams audio to OpenAI: every 200 ms chunk (silence included) becomes a
/// `session.input_audio_buffer.append`; stopping sends `session.close` and waits for
/// `session.closed`.
pub struct OpenAiTranslator {
    api_key: ApiKey,
    url: String,
    input_queue: usize,
}

impl OpenAiTranslator {
    /// A translator using `api_key` against the real endpoint.
    pub fn new(api_key: ApiKey) -> Self {
        Self::with_url(api_key, TRANSLATIONS_URL)
    }

    /// Like [`new`](Self::new), against another endpoint (a local test server).
    pub fn with_url(api_key: ApiKey, url: impl Into<String>) -> Self {
        Self {
            api_key,
            url: url.into(),
            input_queue: DEFAULT_INPUT_QUEUE,
        }
    }

    /// Changes how many 200 ms chunks may wait before the oldest is dropped.
    pub fn with_input_queue(mut self, chunks: usize) -> Self {
        self.input_queue = chunks;
        self
    }
}

impl std::fmt::Debug for OpenAiTranslator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiTranslator")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl Translator for OpenAiTranslator {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn start(
        &self,
        target_language: Language,
    ) -> BoxFuture<'_, Result<LiveSession, TranslatorError>> {
        Box::pin(async move {
            let session = connect_to(&self.url, &self.api_key, target_language).await?;
            let (input, reader) = input_channel(self.input_queue);
            let (outputs_tx, outputs_rx) = mpsc::channel(OUTPUT_QUEUE);
            let task = tokio::spawn(drive(session, reader, outputs_tx, input.clone()));
            Ok(LiveSession::new(input, outputs_rx, task))
        })
    }
}

/// Forwards queued chunks to the socket and outputs to the consumer until the session ends.
async fn drive(
    mut session: TranslatorSession,
    reader: InputReader,
    outputs: mpsc::Sender<TranslatorOutput>,
    input: AudioInput,
) {
    let sender = session.audio_sender();
    // A task of its own so a slow socket never holds back the outputs; while it waits, the input
    // queue drops the oldest chunks.
    let mut audio = tokio::spawn(async move {
        while let Some(chunk) = reader.next_chunk().await {
            if sender.send(&chunk).await.is_err() {
                break;
            }
        }
    });

    let mut closing = false;
    loop {
        tokio::select! {
            _ = &mut audio, if !closing => {
                // Input stopped and every chunk was sent: ask the server to finish.
                closing = true;
                session.close().await;
            }
            output = session.next_output() => {
                let Some(output) = output else { break };
                let last = output == TranslatorOutput::Closed;
                if outputs.send(output).await.is_err() || last {
                    break;
                }
            }
        }
    }
    input.close();
    audio.abort();
}

#[cfg(test)]
mod tests {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::tungstenite::Message;

    use super::*;
    use crate::translator::pcm::{CHUNK_SAMPLES, decode_pcm16, encode_pcm16};

    async fn recv_json(socket: &mut WebSocketStream<TcpStream>) -> Value {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                return serde_json::from_str(text.as_str()).unwrap();
            }
        }
    }

    async fn send_json(socket: &mut WebSocketStream<TcpStream>, value: Value) {
        socket.send(Message::text(value.to_string())).await.unwrap();
    }

    #[tokio::test]
    async fn streams_chunks_and_closes_gracefully() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _update = recv_json(&mut socket).await;
            send_json(
                &mut socket,
                json!({ "type": "session.updated", "session": {} }),
            )
            .await;

            let mut appended = Vec::new();
            loop {
                let event = recv_json(&mut socket).await;
                match event["type"].as_str().unwrap() {
                    "session.input_audio_buffer.append" => {
                        let samples = decode_pcm16(event["audio"].as_str().unwrap()).unwrap();
                        appended.push(samples.len());
                        if appended.len() == 1 {
                            send_json(
                                &mut socket,
                                json!({ "type": "session.output_audio.delta", "delta": encode_pcm16(&[4, 5]) }),
                            )
                            .await;
                        }
                    }
                    "session.close" => break,
                    other => panic!("unexpected {other}"),
                }
            }
            send_json(
                &mut socket,
                json!({ "type": "session.output_transcript.delta", "delta": "bye" }),
            )
            .await;
            send_json(&mut socket, json!({ "type": "session.closed" })).await;
            while let Some(Ok(message)) = socket.next().await {
                if message.is_close() {
                    break;
                }
            }
            appended
        });

        let key = ApiKey::parse("sk-test-0000000000000000000000abcd").unwrap();
        let translator = OpenAiTranslator::with_url(key, url);
        let mut session = translator.start(Language::English).await.unwrap();
        session.push(&vec![1; CHUNK_SAMPLES + 1000]);
        assert_eq!(
            session.next_output().await,
            Some(TranslatorOutput::Audio(vec![4, 5].into()))
        );
        session.push(&vec![2; 500]);

        let rest = session.finish().await;
        assert_eq!(
            rest,
            vec![
                TranslatorOutput::OutputTranscriptDelta("bye".into()),
                TranslatorOutput::Closed
            ]
        );
        assert_eq!(server.await.unwrap(), vec![CHUNK_SAMPLES, 1500]);
    }

    /// Probes how the real endpoint treats a session that stops receiving audio (what the voice
    /// detection relies on): speech, then no appends at all for a while (pings only), then
    /// speech again. Prints a timeline of what the server sent. Spends a few cents.
    ///
    /// Run manually, with two speech WAVs (PCM16 mono 24 kHz):
    /// `$env:LIVEDUB_PROBE_WAVS = "a.wav;b.wav"; cargo test -- --ignored --nocapture probe_idle`
    /// (optional `LIVEDUB_PROBE_IDLE_S`, default 60). The key comes from `OPENAI_API_KEY` or the
    /// one stored in Credential Manager.
    #[tokio::test]
    #[ignore = "needs an OpenAI key, network access and speech WAVs; spends a few cents"]
    async fn probe_idle_session() {
        use std::time::{Duration, Instant};

        let key = match std::env::var("OPENAI_API_KEY") {
            Ok(raw) => ApiKey::parse(&raw).unwrap(),
            Err(_) => crate::credentials::Credentials::system()
                .load()
                .unwrap()
                .unwrap(),
        };
        let wavs: Vec<Vec<i16>> = std::env::var("LIVEDUB_PROBE_WAVS")
            .unwrap()
            .split(';')
            .map(|path| {
                let mut reader = hound::WavReader::open(path).unwrap();
                assert_eq!(reader.spec().sample_rate, 24_000);
                reader.samples::<i16>().map(Result::unwrap).collect()
            })
            .collect();
        let idle = Duration::from_secs(
            std::env::var("LIVEDUB_PROBE_IDLE_S")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(60),
        );

        let mut session = OpenAiTranslator::new(key)
            .start(Language::Spanish)
            .await
            .unwrap();
        let input = session.input();
        let started = Instant::now();
        let at = move || started.elapsed().as_secs_f64();

        // The feeder: each WAV followed by 1.5 s of silence, in real time, then nothing.
        let feeder = {
            let wavs = wavs.clone();
            tokio::spawn(async move {
                for (index, wav) in wavs.iter().enumerate() {
                    if index > 0 {
                        println!("{:7.2}s  -- no appends for {} s --", at(), idle.as_secs());
                        tokio::time::sleep(idle).await;
                    }
                    println!("{:7.2}s  >> appending speech {index}", at());
                    let mut audio = wav.clone();
                    audio.extend(std::iter::repeat_n(0, 36_000));
                    for chunk in audio.chunks(CHUNK_SAMPLES) {
                        input.push(chunk);
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                    println!("{:7.2}s  >> appending stopped", at());
                }
            })
        };

        let total = Duration::from_secs_f64(
            wavs.iter()
                .map(|wav| wav.len() as f64 / 24_000.0 + 1.5)
                .sum::<f64>(),
        ) + idle * (wavs.len() as u32 - 1)
            + Duration::from_secs(12);
        let deadline = tokio::time::Instant::now() + total;
        let mut audible = 0;
        let mut silent = 0;
        loop {
            let output = tokio::select! {
                output = session.next_output() => output,
                () = tokio::time::sleep_until(deadline) => break,
            };
            match output {
                Some(TranslatorOutput::Audio(delta)) => {
                    let loud = !crate::translator::jitter::is_digital_silence(&delta.samples);
                    if loud {
                        audible += 1;
                    } else {
                        silent += 1;
                    }
                    println!(
                        "{:7.2}s  delta {} ms {}",
                        at(),
                        delta.samples.len() / 24,
                        if loud { "audible" } else { "silent" }
                    );
                }
                Some(TranslatorOutput::OutputTranscriptDelta(text)) => {
                    println!("{:7.2}s  caption {text:?}", at());
                }
                Some(TranslatorOutput::InputTranscriptDelta(text)) => {
                    println!("{:7.2}s  source {text:?}", at());
                }
                Some(TranslatorOutput::Error { code, message }) => {
                    println!("{:7.2}s  ERROR {code}: {message}", at());
                }
                Some(TranslatorOutput::Closed) | None => {
                    println!("{:7.2}s  CLOSED by the server", at());
                    break;
                }
            }
        }
        feeder.await.unwrap();
        println!(
            "{:7.2}s  closing ({audible} audible, {silent} silent deltas)",
            at()
        );
        let rest = session.finish().await;
        println!("{:7.2}s  closed, {} more outputs", at(), rest.len());
    }
}
