//! WebSocket session with the translations endpoint.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::events::{ClientEvent, ServerEvent, parse_server_event};
use super::pcm::{decode_pcm16, encode_pcm16};
use super::{TRANSLATIONS_URL, TranslatorError, redact_secrets};
use crate::credentials::ApiKey;
use crate::settings::Language;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long the handshake and the session setup may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to wait for `session.closed` after sending `session.close`.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(15);

/// Queued audio chunks before [`AudioSender::try_send`] reports the queue as full (800 ms of
/// 200 ms chunks). Kept short so a stalled socket shows up quickly as backpressure; the live
/// input queue in front of it then drops the oldest audio instead of adding delay.
const AUDIO_QUEUE: usize = 4;

/// Queued outputs before the socket stops being read until the consumer catches up.
const OUTPUT_QUEUE: usize = 256;

/// Something the session produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslatorOutput {
    /// Translated audio: PCM16 mono 24 kHz samples.
    Audio(Vec<i16>),
    /// Source-language transcript text (append-only delta).
    InputTranscriptDelta(String),
    /// Translated transcript text (append-only delta).
    OutputTranscriptDelta(String),
    /// OpenAI reported an error. The session may keep going; `Closed` follows if it does not.
    Error { code: String, message: String },
    /// The session ended. Always the last output.
    Closed,
}

enum Command {
    /// Base64 PCM16 audio.
    Audio(String),
    /// Graceful close: `session.close`, drain, close the socket.
    Close,
}

/// Sends PCM16 mono 24 kHz audio to a session. Cheap to clone.
#[derive(Clone)]
pub struct AudioSender {
    commands: mpsc::Sender<Command>,
}

impl AudioSender {
    /// Queues a chunk of audio, waiting if the queue is full. Silence must be sent too.
    pub async fn send(&self, samples: &[i16]) -> Result<(), TranslatorError> {
        self.commands
            .send(Command::Audio(encode_pcm16(samples)))
            .await
            .map_err(|_| TranslatorError::Closed)
    }

    /// Queues a chunk of audio without waiting (for audio threads). Fails when the queue is full or
    /// the session is closed; the chunk is dropped.
    pub fn try_send(&self, samples: &[i16]) -> Result<(), TranslatorError> {
        self.commands
            .try_send(Command::Audio(encode_pcm16(samples)))
            .map_err(|_| TranslatorError::Closed)
    }
}

impl std::fmt::Debug for AudioSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AudioSender")
    }
}

/// An open translation session.
///
/// Read outputs with [`next_output`](Self::next_output) until [`TranslatorOutput::Closed`]. To
/// finish, call [`close`](Self::close) and keep reading: the server first sends the translation of
/// the audio it still holds. Dropping the session (and every [`AudioSender`]) also closes it.
#[derive(Debug)]
pub struct TranslatorSession {
    audio: AudioSender,
    outputs: mpsc::Receiver<TranslatorOutput>,
    session_id: Option<String>,
    task: JoinHandle<()>,
}

impl TranslatorSession {
    /// A handle to send audio from other tasks or threads.
    pub fn audio_sender(&self) -> AudioSender {
        self.audio.clone()
    }

    /// Sends a chunk of audio (see [`AudioSender::send`]).
    pub async fn send_audio(&self, samples: &[i16]) -> Result<(), TranslatorError> {
        self.audio.send(samples).await
    }

    /// The next output, or `None` once [`TranslatorOutput::Closed`] has been returned.
    pub async fn next_output(&mut self) -> Option<TranslatorOutput> {
        self.outputs.recv().await
    }

    /// Starts a graceful close. Keep calling [`next_output`](Self::next_output) until `Closed`.
    pub async fn close(&self) {
        // If the task is gone the session is already closed.
        let _ = self.audio.commands.send(Command::Close).await;
    }

    /// OpenAI's session id (`sess_…`), for logs.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Whether the background task has finished (the socket is closed).
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

/// Opens a translation session against OpenAI with `target_language` as output language.
pub async fn connect(
    api_key: &ApiKey,
    target_language: Language,
) -> Result<TranslatorSession, TranslatorError> {
    connect_to(TRANSLATIONS_URL, api_key, target_language).await
}

/// Like [`connect`], against another endpoint (a local test server).
pub async fn connect_to(
    url: &str,
    api_key: &ApiKey,
    target_language: Language,
) -> Result<TranslatorSession, TranslatorError> {
    let mut request = url
        .into_client_request()
        .map_err(|error| TranslatorError::Protocol(format!("invalid endpoint: {error}")))?;
    let mut auth = HeaderValue::from_str(&format!("Bearer {}", api_key.expose()))
        .map_err(|_| TranslatorError::Auth)?;
    auth.set_sensitive(true);
    request.headers_mut().insert(AUTHORIZATION, auth);

    let (mut socket, _response) =
        timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .map_err(|_| TranslatorError::Network("connection timed out".into()))?
            .map_err(handshake_error)?;

    let session_id = timeout(CONNECT_TIMEOUT, setup(&mut socket, target_language))
        .await
        .map_err(|_| TranslatorError::Network("session setup timed out".into()))??;
    tracing::info!(
        session_id = session_id.as_deref().unwrap_or("unknown"),
        ?target_language,
        "translation session started"
    );

    let (commands_tx, commands_rx) = mpsc::channel(AUDIO_QUEUE);
    let (outputs_tx, outputs_rx) = mpsc::channel(OUTPUT_QUEUE);
    let task = tokio::spawn(run(socket, commands_rx, outputs_tx));

    Ok(TranslatorSession {
        audio: AudioSender {
            commands: commands_tx,
        },
        outputs: outputs_rx,
        session_id,
        task,
    })
}

/// Maps a failed handshake to a key-free error.
fn handshake_error(error: tungstenite::Error) -> TranslatorError {
    match error {
        tungstenite::Error::Http(response) => {
            TranslatorError::from_handshake_status(response.status().as_u16())
        }
        tungstenite::Error::Io(error) => TranslatorError::Network(error.to_string()),
        tungstenite::Error::Tls(error) => TranslatorError::Network(format!("TLS error: {error}")),
        tungstenite::Error::Url(error) => {
            TranslatorError::Protocol(format!("invalid endpoint: {error}"))
        }
        other => TranslatorError::Protocol(other.to_string()),
    }
}

/// Sends `session.update` and waits for `session.updated`. Returns the session id.
async fn setup(
    socket: &mut Socket,
    target_language: Language,
) -> Result<Option<String>, TranslatorError> {
    send_event(socket, &ClientEvent::session_update(target_language)).await?;

    let mut session_id = None;
    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| TranslatorError::Network(error.to_string()))?;
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        match parse_server_event(text.as_str()) {
            Ok(ServerEvent::SessionCreated { session }) => session_id = session.id,
            Ok(ServerEvent::SessionUpdated { session }) => {
                return Ok(session.id.or(session_id));
            }
            Ok(ServerEvent::Error { error }) => {
                return Err(TranslatorError::from_api_error(
                    error.best_code(),
                    &error.message,
                ));
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "unreadable translation server event"),
        }
    }
    Err(TranslatorError::Protocol(
        "the connection closed before the session was ready".into(),
    ))
}

async fn send_event(socket: &mut Socket, event: &ClientEvent) -> Result<(), TranslatorError> {
    let json = event
        .to_json()
        .map_err(|error| TranslatorError::Protocol(error.to_string()))?;
    socket
        .send(Message::text(json))
        .await
        .map_err(|error| TranslatorError::Network(error.to_string()))
}

/// Pumps audio in and outputs out until the session ends, then closes the socket and reports
/// [`TranslatorOutput::Closed`].
async fn run(
    mut socket: Socket,
    mut commands: mpsc::Receiver<Command>,
    outputs: mpsc::Sender<TranslatorOutput>,
) {
    // Set once `session.close` is sent: stop taking audio and wait for `session.closed`.
    let mut close_deadline: Option<Instant> = None;

    loop {
        tokio::select! {
            command = commands.recv(), if close_deadline.is_none() => {
                let event = match command {
                    Some(Command::Audio(audio)) => ClientEvent::InputAudioBufferAppend { audio },
                    Some(Command::Close) | None => {
                        close_deadline = Some(Instant::now() + CLOSE_TIMEOUT);
                        ClientEvent::SessionClose
                    }
                };
                if let Err(error) = send_event(&mut socket, &event).await {
                    tracing::warn!(%error, "could not send to the translation socket");
                    let _ = outputs.send(TranslatorOutput::Error {
                        code: error.code().to_owned(),
                        message: error.to_string(),
                    }).await;
                    break;
                }
            }
            message = socket.next() => {
                let text = match message {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(frame))) => {
                        tracing::info!(?frame, "translation socket closed by the server");
                        break;
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => {
                        tracing::warn!(%error, "translation socket error");
                        let _ = outputs.send(TranslatorOutput::Error {
                            code: "network".into(),
                            message: format!("connection lost: {error}"),
                        }).await;
                        break;
                    }
                    None => break,
                };
                match handle_event(text.as_str()) {
                    Step::Output(output) => {
                        if outputs.send(output).await.is_err() {
                            // Nobody is listening any more.
                            break;
                        }
                    }
                    Step::Closed => break,
                    Step::Skip => {}
                }
            }
            () = wait_until(close_deadline) => {
                tracing::warn!("no session.closed before the timeout, closing the socket");
                break;
            }
        }
    }

    // Best effort: the server may already be gone.
    let _ = socket.close(None).await;
    tracing::info!("translation session closed");
    let _ = outputs.send(TranslatorOutput::Closed).await;
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

pub(super) enum Step {
    Output(TranslatorOutput),
    Closed,
    Skip,
}

/// Turns a server text message into what the session does next.
pub(super) fn handle_event(text: &str) -> Step {
    let event = match parse_server_event(text) {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!(%error, "unreadable translation server event");
            return Step::Skip;
        }
    };
    match event {
        ServerEvent::OutputAudioDelta {
            delta,
            sample_rate,
            channels,
        } => {
            if sample_rate.is_some_and(|rate| rate != super::pcm::SAMPLE_RATE)
                || channels.is_some_and(|channels| channels != 1)
            {
                tracing::warn!(?sample_rate, ?channels, "unexpected output audio format");
            }
            match decode_pcm16(&delta) {
                Ok(samples) => Step::Output(TranslatorOutput::Audio(samples)),
                Err(error) => {
                    tracing::warn!(%error, "unreadable output audio");
                    Step::Skip
                }
            }
        }
        ServerEvent::InputTranscriptDelta { delta } => {
            Step::Output(TranslatorOutput::InputTranscriptDelta(delta))
        }
        ServerEvent::OutputTranscriptDelta { delta } => {
            Step::Output(TranslatorOutput::OutputTranscriptDelta(delta))
        }
        ServerEvent::Error { error } => {
            let code = error.best_code();
            let message = redact_secrets(&error.message);
            tracing::warn!(%code, %message, "translation error from OpenAI");
            Step::Output(TranslatorOutput::Error { code, message })
        }
        ServerEvent::SessionClosed => Step::Closed,
        ServerEvent::SessionCreated { .. }
        | ServerEvent::SessionUpdated { .. }
        | ServerEvent::Unknown => Step::Skip,
    }
}

#[cfg(test)]
// The handshake callbacks must return tungstenite's (large) `ErrorResponse`.
#[allow(clippy::result_large_err)]
mod tests {
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{
        ErrorResponse, Request as HandshakeRequest, Response as HandshakeResponse,
    };

    use super::*;

    const FAKE_KEY: &str = "sk-test-0000000000000000000000abcd";

    fn key() -> ApiKey {
        ApiKey::parse(FAKE_KEY).unwrap()
    }

    /// Accepts one WebSocket connection, checks the auth header and returns the socket.
    async fn accept(listener: &TcpListener) -> WebSocketStream<TcpStream> {
        let (stream, _) = listener.accept().await.unwrap();
        let check = |request: &HandshakeRequest, response: HandshakeResponse| {
            let auth = request.headers().get(AUTHORIZATION).unwrap();
            assert_eq!(auth.to_str().unwrap(), format!("Bearer {FAKE_KEY}"));
            Ok::<_, ErrorResponse>(response)
        };
        tokio_tungstenite::accept_hdr_async(stream, check)
            .await
            .unwrap()
    }

    async fn recv_json(socket: &mut WebSocketStream<TcpStream>) -> Value {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
                _ => continue,
            }
        }
    }

    async fn send_json(socket: &mut WebSocketStream<TcpStream>, value: Value) {
        socket.send(Message::text(value.to_string())).await.unwrap();
    }

    async fn listener() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "ws://{}/v1/realtime/translations",
            listener.local_addr().unwrap()
        );
        (listener, url)
    }

    /// Full session against a local fake server: setup, audio in, outputs out, graceful close.
    #[tokio::test]
    async fn runs_a_session_against_a_fake_server() {
        let (listener, url) = listener().await;

        let server = tokio::spawn(async move {
            let mut socket = accept(&listener).await;
            send_json(
                &mut socket,
                json!({ "type": "session.created", "session": { "id": "sess_1" } }),
            )
            .await;

            let update = recv_json(&mut socket).await;
            assert_eq!(update["type"], "session.update");
            assert_eq!(update["session"]["audio"]["output"]["language"], "en");
            send_json(
                &mut socket,
                json!({ "type": "session.updated", "session": { "id": "sess_1" } }),
            )
            .await;

            let append = recv_json(&mut socket).await;
            assert_eq!(append["type"], "session.input_audio_buffer.append");
            assert_eq!(append["audio"], encode_pcm16(&[1, -1, 300]));

            send_json(&mut socket, json!({ "type": "rate_limits.updated" })).await;
            send_json(
                &mut socket,
                json!({ "type": "session.input_transcript.delta", "delta": "hola" }),
            )
            .await;
            send_json(
                &mut socket,
                json!({ "type": "session.output_transcript.delta", "delta": "hello" }),
            )
            .await;
            send_json(
                &mut socket,
                json!({ "type": "session.output_audio.delta", "delta": encode_pcm16(&[7, 8]) }),
            )
            .await;
            socket.send(Message::text("garbage")).await.unwrap();
            send_json(
                &mut socket,
                json!({ "type": "error", "error": { "type": "server_error", "code": "boom", "message": "bad key sk-abc" } }),
            )
            .await;

            let close = recv_json(&mut socket).await;
            assert_eq!(close["type"], "session.close");
            send_json(
                &mut socket,
                json!({ "type": "session.output_transcript.delta", "delta": "!" }),
            )
            .await;
            send_json(&mut socket, json!({ "type": "session.closed" })).await;

            // The client closes the socket.
            while let Some(Ok(message)) = socket.next().await {
                if message.is_close() {
                    break;
                }
            }
        });

        let mut session = connect_to(&url, &key(), Language::English).await.unwrap();
        assert_eq!(session.session_id(), Some("sess_1"));
        session.send_audio(&[1, -1, 300]).await.unwrap();

        let mut outputs = Vec::new();
        for _ in 0..4 {
            outputs.push(session.next_output().await.unwrap());
        }
        session.close().await;
        while let Some(output) = session.next_output().await {
            outputs.push(output);
        }

        assert_eq!(
            outputs,
            vec![
                TranslatorOutput::InputTranscriptDelta("hola".into()),
                TranslatorOutput::OutputTranscriptDelta("hello".into()),
                TranslatorOutput::Audio(vec![7, 8]),
                TranslatorOutput::Error {
                    code: "boom".into(),
                    message: "bad key [redacted]".into()
                },
                TranslatorOutput::OutputTranscriptDelta("!".into()),
                TranslatorOutput::Closed,
            ]
        );
        server.await.unwrap();
        assert!(session.audio_sender().send(&[0]).await.is_err());
    }

    #[tokio::test]
    async fn setup_error_event_fails_connect() {
        let (listener, url) = listener().await;
        let server = tokio::spawn(async move {
            let mut socket = accept(&listener).await;
            let _update = recv_json(&mut socket).await;
            send_json(
                &mut socket,
                json!({ "type": "error", "error": { "type": "invalid_request_error", "code": "invalid_language", "message": "nope" } }),
            )
            .await;
        });

        let error = connect_to(&url, &key(), Language::Spanish)
            .await
            .unwrap_err();

        assert!(
            matches!(&error, TranslatorError::Api { code, .. } if code == "invalid_language"),
            "{error:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejected_handshake_maps_the_status() {
        let (listener, url) = listener().await;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let reject = |_: &HandshakeRequest, _: HandshakeResponse| {
                let mut response = ErrorResponse::new(Some("Incorrect API key provided".into()));
                *response.status_mut() = tungstenite::http::StatusCode::UNAUTHORIZED;
                Err(response)
            };
            let _ = tokio_tungstenite::accept_hdr_async(stream, reject).await;
        });

        let error = connect_to(&url, &key(), Language::Spanish)
            .await
            .unwrap_err();

        assert!(matches!(error, TranslatorError::Auth), "{error:?}");
        assert!(!error.to_string().contains(FAKE_KEY));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn unreachable_server_is_a_network_error() {
        let error = connect_to("ws://127.0.0.1:9/", &key(), Language::Spanish)
            .await
            .unwrap_err();

        assert_eq!(error.code(), "network");
        assert!(!error.to_string().contains(FAKE_KEY));
    }

    #[test]
    fn handle_event_skips_unknown_and_bad_audio() {
        assert!(matches!(handle_event(r#"{"type":"x.y"}"#), Step::Skip));
        assert!(matches!(
            handle_event(r#"{"type":"session.output_audio.delta","delta":"AQD/"}"#),
            Step::Skip
        ));
        assert!(matches!(
            handle_event(r#"{"type":"session.closed"}"#),
            Step::Closed
        ));
    }
}
