//! Application error type shared by the core and the Tauri commands.

use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::translator::TranslatorError;
use crate::usage::CapPeriod;

/// Errors returned by the core.
///
/// Commands return [`AppResult`], and errors reach the panel serialized as
/// `{ "code": "...", "message": "..." }` (see the `AppError` type in `src/lib/api.ts`).
///
/// Error messages are shown to the user and may end up in logs: never put secrets such as the
/// OpenAI API key in them.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// A file system or other I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The settings sent by the panel (or read from disk) are not valid.
    #[error("invalid settings: {0}")]
    InvalidSettings(String),

    /// The API key typed by the user does not look like an OpenAI key. The message never
    /// includes the key.
    #[error("invalid API key: {0}")]
    InvalidApiKey(String),

    /// An action needs the OpenAI API key and none is stored.
    #[error("no OpenAI API key is set")]
    NoApiKey,

    /// Windows Credential Manager could not read, store or delete the API key.
    #[error("credential store error: {0}")]
    CredentialStore(String),

    /// The OpenAI translation session failed (auth, model access, rate limit, network,
    /// protocol). The code comes from [`TranslatorError::code`].
    #[error(transparent)]
    Translator(#[from] TranslatorError),

    /// A translation session dropped and every reconnect attempt failed (see
    /// [`crate::pipeline::reconnect`]). Holds the last attempt's error.
    #[error(
        "lost the connection to OpenAI and could not reconnect after {attempts} attempts: {last}"
    )]
    ReconnectFailed {
        attempts: u32,
        last: TranslatorError,
    },

    /// Windows audio (WASAPI) failed, for example while listing devices.
    #[error("audio device error: {0}")]
    Audio(String),

    /// The outgoing translation has no output device (virtual cable) chosen.
    #[error("no output device is chosen for the outgoing translation")]
    NoOutputDevice,

    /// A device chosen in the settings is not connected (or disabled).
    #[error("device not found: {0}")]
    DeviceMissing(String),

    /// The app chosen as the incoming source is not running. Holds its executable name.
    #[error("the source app is not running: {0}")]
    SourceAppNotRunning(String),

    /// The in-app spending cap is reached (see [`crate::usage`]): a direction may only start
    /// once the user raises the cap or chooses to keep going today.
    #[error("the {period} spending cap is reached: spent {spent_usd:.2} of {limit_usd:.2} USD")]
    SpendingCapReached {
        period: CapPeriod,
        spent_usd: f64,
        limit_usd: f64,
    },

    /// Windows refused a keyboard shortcut (another app registered it, or Windows reserves
    /// it). Holds the shortcut.
    #[error("shortcut not available (another app may be using it): {0}")]
    HotkeyUnavailable(String),

    /// A keyboard shortcut types a character on the keyboard layout (`AltGr` is `Ctrl+Alt`), so
    /// it cannot be taken. Holds the shortcut.
    #[error("shortcut types a character on this keyboard layout: {0}")]
    HotkeyTypesCharacter(String),

    /// An unexpected internal failure that the user cannot act on.
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Stable, machine-readable code the panel can branch on.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::InvalidSettings(_) => "invalid_settings",
            Self::InvalidApiKey(_) => "invalid_api_key",
            Self::NoApiKey => "no_api_key",
            Self::CredentialStore(_) => "credential_store",
            Self::Translator(error) => error.code(),
            Self::ReconnectFailed { .. } => "reconnect_failed",
            Self::Audio(_) => "audio",
            Self::NoOutputDevice => "no_output_device",
            Self::DeviceMissing(_) => "device_missing",
            Self::SourceAppNotRunning(_) => "source_app_not_running",
            Self::SpendingCapReached { .. } => "spending_cap_reached",
            Self::HotkeyUnavailable(_) => "hotkey_unavailable",
            Self::HotkeyTypesCharacter(_) => "hotkey_types_character",
            Self::Internal(_) => "internal",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

/// Result type used across the core and by Tauri commands.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn io_error_serializes_with_code_and_message() {
        let error = AppError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "code": "io", "message": "I/O error: missing" })
        );
    }

    #[test]
    fn invalid_settings_error_serializes_with_code_and_message() {
        let error = AppError::InvalidSettings("bad volume".into());

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "code": "invalid_settings", "message": "invalid settings: bad volume" })
        );
    }

    #[test]
    fn api_key_errors_serialize_with_code_and_message() {
        assert_eq!(
            serde_json::to_value(AppError::NoApiKey).unwrap(),
            json!({ "code": "no_api_key", "message": "no OpenAI API key is set" })
        );
        assert_eq!(
            serde_json::to_value(AppError::InvalidApiKey("bad prefix".into())).unwrap(),
            json!({ "code": "invalid_api_key", "message": "invalid API key: bad prefix" })
        );
        assert_eq!(
            serde_json::to_value(AppError::CredentialStore("denied".into())).unwrap(),
            json!({ "code": "credential_store", "message": "credential store error: denied" })
        );
    }

    #[test]
    fn translator_errors_serialize_with_their_own_code() {
        assert_eq!(
            serde_json::to_value(AppError::from(TranslatorError::Auth)).unwrap(),
            json!({ "code": "openai_auth", "message": "OpenAI rejected the API key" })
        );
        assert_eq!(
            serde_json::to_value(AppError::from(TranslatorError::RateLimited {
                retry_after: None
            }))
            .unwrap()["code"],
            json!("openai_rate_limited")
        );
        assert_eq!(
            serde_json::to_value(AppError::from(TranslatorError::QuotaExceeded)).unwrap()["code"],
            json!("openai_quota")
        );
    }

    #[test]
    fn reconnect_failure_names_the_attempts_and_the_last_error() {
        let error = AppError::ReconnectFailed {
            attempts: 6,
            last: TranslatorError::Network("connection refused".into()),
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({
                "code": "reconnect_failed",
                "message": "lost the connection to OpenAI and could not reconnect after 6 attempts: could not reach OpenAI: connection refused"
            })
        );
    }

    #[test]
    fn audio_error_serializes_with_code_and_message() {
        let error = AppError::Audio("could not list audio devices".into());

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "code": "audio", "message": "audio device error: could not list audio devices" })
        );
    }

    #[test]
    fn device_errors_serialize_with_code_and_message() {
        assert_eq!(
            serde_json::to_value(AppError::NoOutputDevice).unwrap(),
            json!({
                "code": "no_output_device",
                "message": "no output device is chosen for the outgoing translation"
            })
        );
        assert_eq!(
            serde_json::to_value(AppError::DeviceMissing(
                "the microphone is not connected".into()
            ))
            .unwrap(),
            json!({
                "code": "device_missing",
                "message": "device not found: the microphone is not connected"
            })
        );
        assert_eq!(
            serde_json::to_value(AppError::SourceAppNotRunning("Discord.exe".into())).unwrap(),
            json!({
                "code": "source_app_not_running",
                "message": "the source app is not running: Discord.exe"
            })
        );
    }

    #[test]
    fn spending_cap_error_names_the_period_and_amounts() {
        let error = AppError::SpendingCapReached {
            period: CapPeriod::Daily,
            spent_usd: 1.004,
            limit_usd: 1.0,
        };

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({
                "code": "spending_cap_reached",
                "message": "the daily spending cap is reached: spent 1.00 of 1.00 USD"
            })
        );
    }

    #[test]
    fn internal_error_serializes_with_code_and_message() {
        let error = AppError::Internal("boom".into());

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "code": "internal", "message": "internal error: boom" })
        );
    }
}
