//! OpenAI realtime translation client (`gpt-realtime-translate`).
//!
//! [`connect`] opens the translations WebSocket, configures the target language and returns a
//! [`TranslatorSession`]: PCM16 audio goes in through an [`AudioSender`], and translated audio and
//! transcript deltas come out as [`TranslatorOutput`]s. Each pipeline direction owns its own
//! session.
//!
//! On top of it, [`Translator`] is the provider-agnostic interface the pipelines use: it starts a
//! [`LiveSession`] that takes PCM16 mono 24 kHz audio of any length through an [`AudioInput`]
//! (cut into 200 ms appends, silence included, dropping the oldest audio if the provider stalls).
//! [`OpenAiTranslator`] is the real provider; [`FakeTranslator`] replays a recorded session or
//! echoes the input, for development without an API key (`LIVEDUB_FAKE_TRANSLATOR=1`). On the
//! playback side, [`JitterBuffer`] smooths out the irregular output deltas.
//!
//! Guide: <https://developers.openai.com/api/docs/guides/realtime-translation>
//!
//! The API key only travels in the `Authorization` header of the WebSocket handshake. It is never
//! logged, and error messages are built from fixed text (plus server messages with anything that
//! looks like a key removed).

pub mod events;
pub mod fake;
pub mod jitter;
pub mod live;
mod openai;
pub mod pcm;
mod session;

pub use fake::{FAKE_TRANSLATOR_ENV, FakeMode, FakeTranslator};
pub use jitter::JitterBuffer;
pub use live::{AudioInput, LiveSession, Translator};
pub use openai::OpenAiTranslator;
pub use session::{AudioSender, TranslatorOutput, TranslatorSession, connect, connect_to};

/// Translations WebSocket endpoint.
pub const TRANSLATIONS_URL: &str =
    "wss://api.openai.com/v1/realtime/translations?model=gpt-realtime-translate";

/// Errors of the translation client. Messages never include the API key.
#[derive(Debug, thiserror::Error)]
pub enum TranslatorError {
    /// OpenAI rejected the API key (HTTP 401, or an `invalid_api_key` error event after the
    /// handshake).
    #[error("OpenAI rejected the API key")]
    Auth,

    /// The key is valid but the account cannot use the model (HTTP 403/404).
    #[error("this OpenAI account cannot use gpt-realtime-translate")]
    NoModelAccess,

    /// Too many requests or no quota left (HTTP 429).
    #[error("OpenAI rate limit or quota reached, try again later")]
    RateLimited,

    /// OpenAI could not be reached (DNS, TCP, TLS, timeout).
    #[error("could not reach OpenAI: {0}")]
    Network(String),

    /// The server answered something the client did not expect.
    #[error("unexpected answer from OpenAI: {0}")]
    Protocol(String),

    /// OpenAI sent an `error` event while setting up the session.
    #[error("OpenAI error ({code}): {message}")]
    Api { code: String, message: String },

    /// The session is closed; no more audio can be sent.
    #[error("the translation session is closed")]
    Closed,
}

impl TranslatorError {
    /// Stable, machine-readable code (used as the `AppError` code).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Auth => "openai_auth",
            Self::NoModelAccess => "openai_no_model_access",
            Self::RateLimited => "openai_rate_limited",
            Self::Network(_) => "network",
            Self::Protocol(_) => "openai_protocol",
            Self::Api { .. } => "openai_api",
            Self::Closed => "translator_closed",
        }
    }

    /// Maps the HTTP status of a rejected handshake.
    pub fn from_handshake_status(status: u16) -> Self {
        match status {
            401 => Self::Auth,
            403 | 404 => Self::NoModelAccess,
            429 => Self::RateLimited,
            other => Self::Protocol(format!("handshake rejected with HTTP {other}")),
        }
    }
}

impl TranslatorError {
    /// Maps an `error` event received while setting up the session. OpenAI accepts the WebSocket
    /// handshake even with a wrong key and reports it as an `invalid_api_key` error event.
    pub fn from_api_error(code: String, message: &str) -> Self {
        match code.as_str() {
            "invalid_api_key" | "invalid_authentication" | "unauthorized" => Self::Auth,
            "model_not_found" | "permission_denied" | "insufficient_permissions" => {
                Self::NoModelAccess
            }
            "rate_limit_exceeded" | "insufficient_quota" => Self::RateLimited,
            _ => Self::Api {
                code,
                message: redact_secrets(message),
            },
        }
    }
}

/// Removes anything that looks like an OpenAI key (`sk-…`) from a message received from the
/// server, so it can be logged and shown safely.
pub fn redact_secrets(message: &str) -> String {
    message
        .split(' ')
        .map(|word| {
            if word.contains("sk-") {
                "[redacted]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_handshake_statuses() {
        assert!(matches!(
            TranslatorError::from_handshake_status(401),
            TranslatorError::Auth
        ));
        assert!(matches!(
            TranslatorError::from_handshake_status(403),
            TranslatorError::NoModelAccess
        ));
        assert!(matches!(
            TranslatorError::from_handshake_status(404),
            TranslatorError::NoModelAccess
        ));
        assert!(matches!(
            TranslatorError::from_handshake_status(429),
            TranslatorError::RateLimited
        ));
        assert_eq!(
            TranslatorError::from_handshake_status(500).code(),
            "openai_protocol"
        );
    }

    #[test]
    fn maps_setup_error_codes() {
        let message = "Incorrect API key provided: sk-fake***0000.";
        assert!(matches!(
            TranslatorError::from_api_error("invalid_api_key".into(), message),
            TranslatorError::Auth
        ));
        assert!(matches!(
            TranslatorError::from_api_error("model_not_found".into(), ""),
            TranslatorError::NoModelAccess
        ));
        assert!(matches!(
            TranslatorError::from_api_error("insufficient_quota".into(), ""),
            TranslatorError::RateLimited
        ));
        let other = TranslatorError::from_api_error("weird".into(), message);
        assert_eq!(other.code(), "openai_api");
        assert!(!other.to_string().contains("sk-fake"), "{other}");
    }

    #[test]
    fn redacts_key_like_words() {
        assert_eq!(
            redact_secrets("Incorrect API key provided: sk-abc***wxyz. Check it."),
            "Incorrect API key provided: [redacted] Check it."
        );
        assert_eq!(redact_secrets("no secrets here"), "no secrets here");
    }
}
