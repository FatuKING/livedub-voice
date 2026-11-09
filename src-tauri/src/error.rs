//! Application error type shared by the core and the Tauri commands.

use serde::ser::{Serialize, SerializeStruct, Serializer};

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
    fn internal_error_serializes_with_code_and_message() {
        let error = AppError::Internal("boom".into());

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "code": "internal", "message": "internal error: boom" })
        );
    }
}
