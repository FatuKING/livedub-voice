//! Checks an API key against OpenAI.
//!
//! The check is a `GET /v1/models/gpt-realtime-translate` with the key as Bearer token: it is free
//! (no tokens are spent) and tells apart a wrong key (401) from an account without access to the
//! model (403/404). A valid key on an account without credits still passes: billing errors only
//! show up once audio is streamed.

use std::time::Duration;

use serde::Serialize;

use super::ApiKey;
use crate::error::{AppError, AppResult};

/// Model metadata endpoint used for the check.
pub const VALIDATION_URL: &str = "https://api.openai.com/v1/models/gpt-realtime-translate";

/// How long the check may take before it is reported as offline.
const VALIDATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of checking a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum KeyValidation {
    /// The key works and can use the model.
    Valid,
    /// OpenAI rejected the key (401).
    InvalidKey,
    /// The key is valid but the account cannot use the model (403/404).
    NoModelAccess,
    /// Too many requests (429); try again later.
    RateLimited,
    /// OpenAI could not be reached (no network, DNS failure, timeout).
    Offline,
    /// Any other answer. `status` is the HTTP status, if a response arrived.
    Unexpected { status: Option<u16> },
}

/// Maps the HTTP status of the models endpoint to a [`KeyValidation`].
pub fn outcome_for_status(status: u16) -> KeyValidation {
    match status {
        200..=299 => KeyValidation::Valid,
        401 => KeyValidation::InvalidKey,
        403 | 404 => KeyValidation::NoModelAccess,
        429 => KeyValidation::RateLimited,
        other => KeyValidation::Unexpected {
            status: Some(other),
        },
    }
}

/// Validates keys with a shared HTTP client (kept in Tauri managed state).
pub struct KeyValidator {
    client: reqwest::Client,
    url: String,
}

impl KeyValidator {
    /// A validator against the real OpenAI endpoint.
    pub fn new() -> AppResult<Self> {
        Self::with_url(VALIDATION_URL)
    }

    fn with_url(url: &str) -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(VALIDATION_TIMEOUT)
            .user_agent(concat!("livedub-voice/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| {
                AppError::Internal(format!("could not create HTTP client: {error}"))
            })?;
        Ok(Self {
            client,
            url: url.to_owned(),
        })
    }

    /// Checks `key` against OpenAI. Network failures are an outcome, not an error.
    pub async fn validate(&self, key: &ApiKey) -> KeyValidation {
        // `bearer_auth` marks the header as sensitive, so it is redacted from reqwest's `Debug`.
        let response = self
            .client
            .get(&self.url)
            .bearer_auth(key.expose())
            .send()
            .await;
        let outcome = match response {
            Ok(response) => outcome_for_status(response.status().as_u16()),
            Err(error) if error.is_timeout() || error.is_connect() => KeyValidation::Offline,
            Err(error) => {
                // The error holds the URL, never the Authorization header.
                tracing::warn!(%error, "API key check failed");
                KeyValidation::Unexpected { status: None }
            }
        };
        tracing::info!(?outcome, "API key checked");
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_http_statuses_to_outcomes() {
        assert_eq!(outcome_for_status(200), KeyValidation::Valid);
        assert_eq!(outcome_for_status(401), KeyValidation::InvalidKey);
        assert_eq!(outcome_for_status(403), KeyValidation::NoModelAccess);
        assert_eq!(outcome_for_status(404), KeyValidation::NoModelAccess);
        assert_eq!(outcome_for_status(429), KeyValidation::RateLimited);
        assert_eq!(
            outcome_for_status(500),
            KeyValidation::Unexpected { status: Some(500) }
        );
        assert_eq!(
            outcome_for_status(302),
            KeyValidation::Unexpected { status: Some(302) }
        );
    }

    #[test]
    fn outcomes_serialize_to_the_panel_shape() {
        assert_eq!(
            serde_json::to_value(KeyValidation::NoModelAccess).unwrap(),
            json!({ "kind": "noModelAccess" })
        );
        assert_eq!(
            serde_json::to_value(KeyValidation::Unexpected { status: Some(500) }).unwrap(),
            json!({ "kind": "unexpected", "status": 500 })
        );
    }

    #[tokio::test]
    async fn unreachable_server_is_offline() {
        // Port 9 on localhost (discard) is closed on CI and dev machines: the connection is refused.
        let validator = KeyValidator::with_url("http://127.0.0.1:9/v1/models/x").unwrap();
        let key = ApiKey::parse("sk-test-0000000000000000000000abcd").unwrap();

        assert_eq!(validator.validate(&key).await, KeyValidation::Offline);
    }

    /// Checks a real key against OpenAI. Run manually:
    /// `$env:OPENAI_API_KEY = "sk-…"; cargo test -- --ignored validates_a_real_key`.
    #[tokio::test]
    #[ignore = "needs OPENAI_API_KEY and network access"]
    async fn validates_a_real_key() {
        let raw = std::env::var("OPENAI_API_KEY").unwrap();
        let key = ApiKey::parse(&raw).unwrap();

        let outcome = KeyValidator::new().unwrap().validate(&key).await;

        assert_eq!(outcome, KeyValidation::Valid);
    }
}
