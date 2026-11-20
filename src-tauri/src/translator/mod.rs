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
//! Errors are typed ([`TranslatorError`]): fatal ones (key, billing, model access, language) end
//! a direction, retryable ones (network, timeouts, rate limits, server errors) make the pipeline
//! reconnect with backoff (see [`crate::pipeline::reconnect`]).
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

use std::time::Duration;

use tokio_tungstenite::tungstenite::http::HeaderMap;

pub use fake::{FAKE_TRANSLATOR_ENV, FakeMode, FakeTranslator};
pub use jitter::JitterBuffer;
pub use live::{AudioInput, LiveSession, Translator};
pub use openai::OpenAiTranslator;
pub use session::{
    AudioDelta, AudioSender, TranslatorOutput, TranslatorSession, connect, connect_to,
};

/// Translations WebSocket endpoint.
pub const TRANSLATIONS_URL: &str =
    "wss://api.openai.com/v1/realtime/translations?model=gpt-realtime-translate";

/// Longest wait a retry hint may ask for (a bad hint must not park a direction for hours).
const MAX_RETRY_HINT_S: f64 = 3600.0;

/// Errors of the translation client. Messages never include the API key.
///
/// Each error is either **fatal** (retrying cannot help: the key, the account or the request is
/// wrong) or **retryable** (the network, OpenAI's servers or a rate limit: a later attempt may
/// work), see [`is_retryable`](Self::is_retryable). The pipelines reconnect a dropped session
/// only after retryable errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TranslatorError {
    /// OpenAI rejected the API key (HTTP 401, or an `invalid_api_key` error event after the
    /// handshake). Fatal.
    #[error("OpenAI rejected the API key")]
    Auth,

    /// The key is valid but the account cannot use the model (HTTP 403/404). Fatal.
    #[error("this OpenAI account cannot use gpt-realtime-translate")]
    NoModelAccess,

    /// The account has no credits or quota left (`insufficient_quota`, billing). Fatal.
    #[error("the OpenAI account has no quota left")]
    QuotaExceeded,

    /// OpenAI does not translate into the chosen language. Fatal.
    #[error("OpenAI does not support the chosen target language")]
    UnsupportedLanguage,

    /// Too many requests (HTTP 429, `rate_limit_exceeded`). Retryable, not before
    /// `retry_after` when OpenAI said how long to wait.
    #[error("OpenAI rate limit reached")]
    RateLimited { retry_after: Option<Duration> },

    /// OpenAI's servers failed (HTTP 5xx, `server_error`). Retryable.
    #[error("OpenAI server error: {0}")]
    Server(String),

    /// OpenAI could not be reached, or the connection was lost or went silent (DNS, TCP, TLS,
    /// timeout). Retryable.
    #[error("could not reach OpenAI: {0}")]
    Network(String),

    /// The server answered something the client did not expect (a handshake rejected with an
    /// unexpected status, an invalid endpoint). Fatal.
    #[error("unexpected answer from OpenAI: {0}")]
    Protocol(String),

    /// OpenAI sent an `error` event with another code. Retryable: OpenAI says most errors leave
    /// the session open, and reconnecting is bounded by the reconnect policy.
    #[error("OpenAI error ({code}): {message}")]
    Api { code: String, message: String },

    /// The session is closed; no more audio can be sent. Retryable.
    #[error("the translation session is closed")]
    Closed,
}

impl TranslatorError {
    /// Stable, machine-readable code (used as the `AppError` code).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Auth => "openai_auth",
            Self::NoModelAccess => "openai_no_model_access",
            Self::QuotaExceeded => "openai_quota",
            Self::UnsupportedLanguage => "openai_unsupported_language",
            Self::RateLimited { .. } => "openai_rate_limited",
            Self::Server(_) => "openai_server",
            Self::Network(_) => "network",
            Self::Protocol(_) => "openai_protocol",
            Self::Api { .. } => "openai_api",
            Self::Closed => "translator_closed",
        }
    }

    /// Whether trying again later may work. `false` for what only the user can fix (key,
    /// billing, model access, language) and for protocol errors.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. }
            | Self::Server(_)
            | Self::Network(_)
            | Self::Api { .. }
            | Self::Closed => true,
            Self::Auth
            | Self::NoModelAccess
            | Self::QuotaExceeded
            | Self::UnsupportedLanguage
            | Self::Protocol(_) => false,
        }
    }

    /// How long OpenAI asked to wait before trying again, if it said.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } => *retry_after,
            _ => None,
        }
    }

    /// Maps a rejected handshake. `body` is the response body, if any (an OpenAI error object
    /// tells an exhausted quota from a rate limit); `retry_after` comes from the headers (see
    /// [`retry_after_header`]).
    pub fn from_handshake(status: u16, body: Option<&[u8]>, retry_after: Option<Duration>) -> Self {
        let api_code = body
            .and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok())
            .and_then(|json| {
                let error = json.get("error")?;
                error
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| error.get("type").and_then(serde_json::Value::as_str))
                    .map(str::to_owned)
            });
        match status {
            401 => Self::Auth,
            403 | 404 => Self::NoModelAccess,
            429 if api_code.as_deref() == Some("insufficient_quota") => Self::QuotaExceeded,
            429 => Self::RateLimited { retry_after },
            408 => Self::Network(format!("handshake timed out (HTTP {status})")),
            500..=599 => Self::Server(format!("handshake rejected with HTTP {status}")),
            other => Self::Protocol(format!("handshake rejected with HTTP {other}")),
        }
    }

    /// Maps the HTTP status of a rejected handshake, without body or headers.
    pub fn from_handshake_status(status: u16) -> Self {
        Self::from_handshake(status, None, None)
    }

    /// Maps the code of an `error` event (see [`events::ApiError::classified_code`]), or one of
    /// this client's own codes forwarded in a [`TranslatorOutput::Error`]. OpenAI accepts the
    /// WebSocket handshake even with a wrong key and reports it as an `invalid_api_key` error
    /// event.
    ///
    /// OpenAI does not document the codes of the translation endpoint; these are the codes of
    /// its other APIs, matched leniently. Anything else is [`Self::Api`] (retryable).
    pub fn from_api_error(code: String, message: &str) -> Self {
        let message = redact_secrets(message);
        match code.as_str() {
            "invalid_api_key" | "invalid_authentication" | "unauthorized" | "openai_auth" => {
                Self::Auth
            }
            "model_not_found"
            | "permission_denied"
            | "insufficient_permissions"
            | "openai_no_model_access" => Self::NoModelAccess,
            "insufficient_quota"
            | "billing_hard_limit_reached"
            | "billing_not_active"
            | "openai_quota" => Self::QuotaExceeded,
            "unsupported_language" | "invalid_language" | "openai_unsupported_language" => {
                Self::UnsupportedLanguage
            }
            "rate_limit_exceeded" | "rate_limit_error" | "openai_rate_limited" => {
                Self::RateLimited {
                    retry_after: parse_retry_hint(&message),
                }
            }
            "server_error"
            | "internal_error"
            | "service_unavailable"
            | "overloaded"
            | "openai_server" => Self::Server(message),
            "network" => Self::Network(message),
            "translator_closed" => Self::Closed,
            _ => Self::Api { code, message },
        }
    }
}

/// Reads a wait hint such as "Please try again in 1.5s." or "try again in 450ms" from an
/// OpenAI error message.
pub fn parse_retry_hint(message: &str) -> Option<Duration> {
    const MARKER: &str = "try again in ";
    let lower = message.to_ascii_lowercase();
    let rest = &lower[lower.find(MARKER)? + MARKER.len()..];
    let number_len = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let value: f64 = rest[..number_len].parse().ok()?;
    let unit = rest[number_len..].trim_start();
    let seconds = if unit.starts_with("ms") {
        value / 1000.0
    } else if unit.starts_with('s') {
        value
    } else if unit.starts_with('m') {
        value * 60.0
    } else {
        return None;
    };
    (seconds.is_finite() && seconds >= 0.0)
        .then(|| Duration::from_secs_f64(seconds.min(MAX_RETRY_HINT_S)))
}

/// Reads the `retry-after-ms` or `Retry-After` (seconds) header of a rejected handshake.
pub fn retry_after_header(headers: &HeaderMap) -> Option<Duration> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
    };
    header("retry-after-ms")
        .map(|ms| ms / 1000.0)
        .or_else(|| header("retry-after"))
        .map(|seconds| Duration::from_secs_f64(seconds.min(MAX_RETRY_HINT_S)))
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
    use tokio_tungstenite::tungstenite::http::HeaderValue;

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
            TranslatorError::RateLimited { retry_after: None }
        ));
        assert_eq!(
            TranslatorError::from_handshake_status(503).code(),
            "openai_server"
        );
        assert_eq!(
            TranslatorError::from_handshake_status(400).code(),
            "openai_protocol"
        );
    }

    #[test]
    fn handshake_429_tells_quota_from_rate_limit() {
        let quota = br#"{"error":{"type":"insufficient_quota","code":"insufficient_quota","message":"You exceeded your current quota"}}"#;
        assert!(matches!(
            TranslatorError::from_handshake(429, Some(quota), None),
            TranslatorError::QuotaExceeded
        ));
        let limited = TranslatorError::from_handshake(
            429,
            Some(br#"{"error":{"code":"rate_limit_exceeded"}}"#),
            Some(Duration::from_secs(3)),
        );
        assert_eq!(limited.retry_after(), Some(Duration::from_secs(3)));
        assert!(limited.is_retryable());
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
            TranslatorError::QuotaExceeded
        ));
        assert!(matches!(
            TranslatorError::from_api_error("unsupported_language".into(), ""),
            TranslatorError::UnsupportedLanguage
        ));
        assert_eq!(
            TranslatorError::from_api_error(
                "rate_limit_exceeded".into(),
                "Rate limit reached. Please try again in 1.5s."
            )
            .retry_after(),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            TranslatorError::from_api_error("server_error".into(), "boom").code(),
            "openai_server"
        );
        let other = TranslatorError::from_api_error("weird".into(), message);
        assert_eq!(other.code(), "openai_api");
        assert!(!other.to_string().contains("sk-fake"), "{other}");
    }

    #[test]
    fn own_codes_round_trip() {
        for error in [
            TranslatorError::Auth,
            TranslatorError::NoModelAccess,
            TranslatorError::QuotaExceeded,
            TranslatorError::UnsupportedLanguage,
            TranslatorError::RateLimited { retry_after: None },
            TranslatorError::Server("x".into()),
            TranslatorError::Network("x".into()),
            TranslatorError::Closed,
        ] {
            let back = TranslatorError::from_api_error(error.code().into(), "x");
            assert_eq!(back.code(), error.code());
        }
    }

    #[test]
    fn fatal_and_retryable_errors() {
        for fatal in [
            TranslatorError::Auth,
            TranslatorError::NoModelAccess,
            TranslatorError::QuotaExceeded,
            TranslatorError::UnsupportedLanguage,
            TranslatorError::Protocol("x".into()),
        ] {
            assert!(!fatal.is_retryable(), "{fatal:?}");
        }
        for retryable in [
            TranslatorError::RateLimited { retry_after: None },
            TranslatorError::Server("x".into()),
            TranslatorError::Network("x".into()),
            TranslatorError::Api {
                code: "x".into(),
                message: String::new(),
            },
            TranslatorError::Closed,
        ] {
            assert!(retryable.is_retryable(), "{retryable:?}");
        }
    }

    #[test]
    fn parses_retry_hints() {
        assert_eq!(
            parse_retry_hint("Please try again in 1.5s."),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            parse_retry_hint("Please try again in 450ms"),
            Some(Duration::from_millis(450))
        );
        assert_eq!(
            parse_retry_hint("Try again in 2m"),
            Some(Duration::from_secs(120))
        );
        assert_eq!(parse_retry_hint("Try again later."), None);
        assert_eq!(parse_retry_hint("no hint"), None);
    }

    #[test]
    fn reads_retry_after_headers() {
        let mut headers = HeaderMap::new();
        assert_eq!(retry_after_header(&headers), None);
        headers.insert("retry-after", HeaderValue::from_static("7"));
        assert_eq!(retry_after_header(&headers), Some(Duration::from_secs(7)));
        headers.insert("retry-after-ms", HeaderValue::from_static("250"));
        assert_eq!(
            retry_after_header(&headers),
            Some(Duration::from_millis(250))
        );
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
