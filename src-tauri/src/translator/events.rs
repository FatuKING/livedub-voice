//! Typed client and server events of the realtime translations WebSocket.
//!
//! Shapes follow the OpenAI API reference:
//! - client events: <https://developers.openai.com/api/reference/resources/realtime/translation-client-events>
//! - server events: <https://developers.openai.com/api/reference/resources/realtime/translation-server-events>
//!
//! Server events the client does not know are parsed as [`ServerEvent::Unknown`] and ignored, so a
//! new event type on OpenAI's side never breaks a session.

use serde::{Deserialize, Serialize};

use crate::settings::Language;

/// Model used for the source-language transcript (`session.input_transcript.delta`).
pub const INPUT_TRANSCRIPTION_MODEL: &str = "gpt-realtime-whisper";

/// An event sent by the client.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum ClientEvent {
    /// Configures the session (target language, input transcription).
    #[serde(rename = "session.update")]
    SessionUpdate { session: SessionConfig },

    /// Appends base64 PCM16 mono 24 kHz little-endian audio to the input buffer.
    #[serde(rename = "session.input_audio_buffer.append")]
    InputAudioBufferAppend { audio: String },

    /// Asks the server to flush pending audio, send the remaining output and then
    /// `session.closed`.
    #[serde(rename = "session.close")]
    SessionClose,
}

impl ClientEvent {
    /// The `session.update` sent right after connecting.
    pub fn session_update(target_language: Language) -> Self {
        Self::SessionUpdate {
            session: SessionConfig {
                audio: AudioConfig {
                    input: Some(InputAudioConfig {
                        transcription: Some(TranscriptionConfig {
                            model: INPUT_TRANSCRIPTION_MODEL.to_owned(),
                        }),
                    }),
                    output: OutputAudioConfig {
                        language: target_language,
                    },
                },
            },
        }
    }

    /// Serializes the event to the JSON text sent over the socket.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

/// `session` object of `session.update`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionConfig {
    pub audio: AudioConfig,
}

/// `session.audio`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InputAudioConfig>,
    pub output: OutputAudioConfig,
}

/// `session.audio.input`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputAudioConfig {
    /// Enables `session.input_transcript.delta` events. Translation runs from the audio either way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription: Option<TranscriptionConfig>,
}

/// `session.audio.input.transcription`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptionConfig {
    pub model: String,
}

/// `session.audio.output`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutputAudioConfig {
    /// Target language of the translated audio and transcript (its API code, e.g. `"es"`).
    pub language: Language,
}

/// An event received from the server.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "error")]
    Error { error: ApiError },

    #[serde(rename = "session.created")]
    SessionCreated { session: SessionInfo },

    #[serde(rename = "session.updated")]
    SessionUpdated { session: SessionInfo },

    #[serde(rename = "session.closed")]
    SessionClosed,

    /// Source-language transcript text (append-only).
    #[serde(rename = "session.input_transcript.delta")]
    InputTranscriptDelta { delta: String },

    /// Translated transcript text (append-only).
    #[serde(rename = "session.output_transcript.delta")]
    OutputTranscriptDelta { delta: String },

    /// Translated audio: base64 PCM16. Chunk length varies.
    #[serde(rename = "session.output_audio.delta")]
    OutputAudioDelta {
        delta: String,
        #[serde(default)]
        sample_rate: Option<u32>,
        #[serde(default)]
        channels: Option<u16>,
    },

    /// Any event type this client does not handle.
    #[serde(other)]
    Unknown,
}

/// The `error` object of an `error` server event.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ApiError {
    /// Error category, e.g. `invalid_request_error` or `server_error`.
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    pub message: String,
    #[serde(default)]
    pub param: Option<String>,
}

impl ApiError {
    /// The most specific code available (`code`, else `type`, else `"unknown"`).
    pub fn best_code(&self) -> String {
        self.code
            .clone()
            .or_else(|| self.kind.clone())
            .unwrap_or_else(|| "unknown".to_owned())
    }
}

/// The parts of the server `session` object the client uses.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct SessionInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Only the `type` field, to log the name of events that could not be parsed.
#[derive(Deserialize)]
struct EventType {
    #[serde(rename = "type")]
    kind: String,
}

/// Parses a server text message.
///
/// Unknown event types become [`ServerEvent::Unknown`] (logged at debug). A message that is not
/// valid JSON, or a known event with an unexpected shape, is an error the caller logs and skips.
pub fn parse_server_event(text: &str) -> serde_json::Result<ServerEvent> {
    let event = serde_json::from_str::<ServerEvent>(text)?;
    if event == ServerEvent::Unknown {
        let kind = serde_json::from_str::<EventType>(text)
            .map(|event| event.kind)
            .unwrap_or_default();
        tracing::debug!(event_type = %kind, "ignoring unknown translation server event");
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn to_value(event: &ClientEvent) -> Value {
        serde_json::from_str(&event.to_json().unwrap()).unwrap()
    }

    #[test]
    fn session_update_serializes_language_and_transcription() {
        assert_eq!(
            to_value(&ClientEvent::session_update(Language::Spanish)),
            json!({
                "type": "session.update",
                "session": {
                    "audio": {
                        "input": { "transcription": { "model": "gpt-realtime-whisper" } },
                        "output": { "language": "es" }
                    }
                }
            })
        );
        assert_eq!(
            to_value(&ClientEvent::session_update(Language::English))["session"]["audio"]["output"]
                ["language"],
            json!("en")
        );
        for language in Language::ALL {
            assert_eq!(
                to_value(&ClientEvent::session_update(language))["session"]["audio"]["output"]["language"],
                json!(language.code())
            );
        }
    }

    #[test]
    fn append_and_close_serialize() {
        assert_eq!(
            to_value(&ClientEvent::InputAudioBufferAppend {
                audio: "AAAA".into()
            }),
            json!({ "type": "session.input_audio_buffer.append", "audio": "AAAA" })
        );
        assert_eq!(
            to_value(&ClientEvent::SessionClose),
            json!({ "type": "session.close" })
        );
    }

    #[test]
    fn parses_session_created_from_the_docs() {
        let text = r#"{
            "type": "session.created",
            "event_id": "event_123",
            "session": {
                "id": "sess_123",
                "type": "translation",
                "model": "gpt-realtime-translate",
                "expires_at": 1714857600,
                "audio": {
                    "input": {
                        "transcription": { "model": "gpt-realtime-whisper" },
                        "noise_reduction": { "type": "near_field" }
                    },
                    "output": { "language": "fr" }
                }
            }
        }"#;

        assert_eq!(
            parse_server_event(text).unwrap(),
            ServerEvent::SessionCreated {
                session: SessionInfo {
                    id: Some("sess_123".into()),
                    model: Some("gpt-realtime-translate".into()),
                }
            }
        );
    }

    #[test]
    fn parses_session_updated_and_closed() {
        let updated = r#"{"type":"session.updated","event_id":"e","session":{"id":"sess_1","audio":{"output":{"language":"es"}}}}"#;
        assert!(matches!(
            parse_server_event(updated).unwrap(),
            ServerEvent::SessionUpdated { .. }
        ));
        assert_eq!(
            parse_server_event(r#"{"event_id":"event_987","type":"session.closed"}"#).unwrap(),
            ServerEvent::SessionClosed
        );
    }

    #[test]
    fn parses_transcript_deltas_from_the_docs() {
        assert_eq!(
            parse_server_event(
                r#"{"event_id":"event_125","type":"session.input_transcript.delta","delta":" hear","elapsed_ms":1200}"#
            )
            .unwrap(),
            ServerEvent::InputTranscriptDelta {
                delta: " hear".into()
            }
        );
        assert_eq!(
            parse_server_event(
                r#"{"event_id":"event_124","type":"session.output_transcript.delta","delta":" escuch","elapsed_ms":null}"#
            )
            .unwrap(),
            ServerEvent::OutputTranscriptDelta {
                delta: " escuch".into()
            }
        );
    }

    #[test]
    fn parses_output_audio_delta_from_the_docs() {
        let text = r#"{
            "event_id": "event_123",
            "type": "session.output_audio.delta",
            "delta": "AQD//w==",
            "sample_rate": 24000,
            "channels": 1,
            "format": "pcm16",
            "elapsed_ms": 1200
        }"#;

        assert_eq!(
            parse_server_event(text).unwrap(),
            ServerEvent::OutputAudioDelta {
                delta: "AQD//w==".into(),
                sample_rate: Some(24000),
                channels: Some(1),
            }
        );
        // Optional fields may be missing.
        assert!(matches!(
            parse_server_event(r#"{"type":"session.output_audio.delta","delta":""}"#).unwrap(),
            ServerEvent::OutputAudioDelta {
                sample_rate: None,
                ..
            }
        ));
    }

    #[test]
    fn parses_error_event_from_the_docs() {
        let text = r#"{
            "event_id": "event_890",
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "code": "invalid_event",
                "message": "The 'type' field is missing.",
                "param": null,
                "event_id": "event_567"
            }
        }"#;

        let ServerEvent::Error { error } = parse_server_event(text).unwrap() else {
            panic!("not an error event");
        };
        assert_eq!(error.best_code(), "invalid_event");
        assert_eq!(error.kind.as_deref(), Some("invalid_request_error"));
        assert_eq!(error.message, "The 'type' field is missing.");
    }

    #[test]
    fn error_code_falls_back_to_type() {
        let text = r#"{"type":"error","error":{"type":"server_error","message":"boom"}}"#;

        let ServerEvent::Error { error } = parse_server_event(text).unwrap() else {
            panic!("not an error event");
        };
        assert_eq!(error.best_code(), "server_error");
    }

    #[test]
    fn unknown_events_are_ignored() {
        for text in [
            r#"{"type":"rate_limits.updated","rate_limits":[]}"#,
            r#"{"type":"session.something_new","foo":{"bar":1}}"#,
        ] {
            assert_eq!(parse_server_event(text).unwrap(), ServerEvent::Unknown);
        }
    }

    #[test]
    fn malformed_messages_are_errors() {
        assert!(parse_server_event("not json").is_err());
        assert!(parse_server_event(r#"{"no_type":true}"#).is_err());
        // A known event missing a required field.
        assert!(parse_server_event(r#"{"type":"session.output_audio.delta"}"#).is_err());
    }
}
