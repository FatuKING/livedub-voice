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
            Some(TranslatorOutput::Audio(vec![4, 5]))
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
}
