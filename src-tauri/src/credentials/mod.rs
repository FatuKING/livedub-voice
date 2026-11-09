//! OpenAI API key storage.
//!
//! The key lives only in Windows Credential Manager (through the `keyring` crate). It is never
//! written to `settings.json`, logs or error messages, and the panel never gets it back after
//! saving: it only sees an [`ApiKeyStatus`] (whether a key is set and its last four characters).
//!
//! Storage goes through the [`SecretStore`] trait so tests can use an in-memory fake instead of
//! the real Credential Manager. Checking the key against OpenAI lives in [`validation`].

pub mod validation;

use std::fmt;

use serde::Serialize;
use zeroize::Zeroize;

use crate::error::{AppError, AppResult};

/// Credential Manager service name (the app identifier).
pub const KEYRING_SERVICE: &str = "io.github.fatuking.livedubvoice";

/// Credential Manager user name for the OpenAI API key.
pub const KEYRING_USER: &str = "openai-api-key";

/// Prefix every OpenAI secret key starts with (`sk-…`, `sk-proj-…`, `sk-svcacct-…`).
const API_KEY_PREFIX: &str = "sk-";

/// Shortest key accepted by the format check. Real keys are much longer; this only rejects
/// obvious mistakes such as pasting the prefix alone.
const MIN_API_KEY_LEN: usize = 20;

/// Longest key accepted by the format check (Credential Manager blobs are limited to 2560 bytes).
const MAX_API_KEY_LEN: usize = 512;

/// An OpenAI API key.
///
/// `Debug` is redacted, there is no `Display` or `Serialize`, and the buffer is zeroed on drop.
/// Use [`ApiKey::expose`] only where the raw value is really needed (the store and the HTTP
/// `Authorization` header).
pub struct ApiKey(String);

impl ApiKey {
    /// Parses a key typed or pasted by the user: trims surrounding whitespace and runs a basic
    /// format check. The error message never includes the key.
    pub fn parse(raw: &str) -> AppResult<Self> {
        let key = raw.trim();
        if key.is_empty() {
            return Err(AppError::InvalidApiKey("the key is empty".into()));
        }
        if !key.starts_with(API_KEY_PREFIX) {
            return Err(AppError::InvalidApiKey(
                "OpenAI API keys start with \"sk-\"".into(),
            ));
        }
        if key.len() < MIN_API_KEY_LEN || key.len() > MAX_API_KEY_LEN {
            return Err(AppError::InvalidApiKey(
                "the key does not have the length of an OpenAI API key".into(),
            ));
        }
        if !key.chars().all(|c| c.is_ascii_graphic()) {
            return Err(AppError::InvalidApiKey(
                "the key contains spaces or unexpected characters".into(),
            ));
        }
        Ok(Self(key.to_owned()))
    }

    /// Wraps a key read back from the store, without the format check.
    fn from_stored(value: String) -> Self {
        Self(value)
    }

    /// The raw key. Never log or display it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The last four characters, shown to the user as `sk-…abcd`.
    pub fn last4(&self) -> String {
        let chars: Vec<char> = self.0.chars().collect();
        let start = chars.len().saturating_sub(4);
        chars[start..].iter().collect()
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// What the panel may know about the stored key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyStatus {
    pub has_key: bool,
    /// Last four characters of the stored key; `None` when no key is set.
    pub last4: Option<String>,
}

impl ApiKeyStatus {
    fn of(key: Option<&ApiKey>) -> Self {
        Self {
            has_key: key.is_some(),
            last4: key.map(ApiKey::last4),
        }
    }
}

/// Where the API key is kept. The app uses [`KeyringStore`]; tests use an in-memory fake.
pub trait SecretStore: Send + Sync {
    /// Returns the stored key, or `None` when there is none.
    fn get(&self) -> AppResult<Option<ApiKey>>;
    /// Stores the key, replacing any previous one.
    fn set(&self, key: &ApiKey) -> AppResult<()>;
    /// Deletes the stored key. Deleting when there is none is not an error.
    fn delete(&self) -> AppResult<()>;
}

/// [`SecretStore`] backed by Windows Credential Manager.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry() -> AppResult<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(credential_error)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self) -> AppResult<Option<ApiKey>> {
        match Self::entry()?.get_password() {
            Ok(value) => Ok(Some(ApiKey::from_stored(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(credential_error(error)),
        }
    }

    fn set(&self, key: &ApiKey) -> AppResult<()> {
        Self::entry()?
            .set_password(key.expose())
            .map_err(credential_error)
    }

    fn delete(&self) -> AppResult<()> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(credential_error(error)),
        }
    }
}

/// Maps a `keyring` error to a key-free [`AppError`].
///
/// Only fixed text (plus the OS error for platform failures) is kept: some `keyring` variants
/// carry the stored bytes, so their `Debug`/`Display` must never reach a message or a log.
fn credential_error(error: keyring::Error) -> AppError {
    let detail = match error {
        keyring::Error::PlatformFailure(source) => format!("system error: {source}"),
        keyring::Error::NoStorageAccess(source) => {
            format!("Credential Manager is not accessible: {source}")
        }
        keyring::Error::NoEntry => "no stored key".to_owned(),
        keyring::Error::BadEncoding(_) => "the stored key is not valid text".to_owned(),
        keyring::Error::TooLong(..) => "the key is too long to store".to_owned(),
        keyring::Error::Invalid(..) => "invalid credential attributes".to_owned(),
        keyring::Error::Ambiguous(_) => "more than one stored key matches".to_owned(),
        _ => "unknown credential store error".to_owned(),
    };
    AppError::CredentialStore(detail)
}

/// The API key service kept in Tauri managed state.
pub struct Credentials {
    store: Box<dyn SecretStore>,
}

impl Credentials {
    pub fn new(store: impl SecretStore + 'static) -> Self {
        Self {
            store: Box::new(store),
        }
    }

    /// Uses Windows Credential Manager.
    pub fn system() -> Self {
        Self::new(KeyringStore)
    }

    /// Whether a key is stored, and its last four characters.
    pub fn status(&self) -> AppResult<ApiKeyStatus> {
        Ok(ApiKeyStatus::of(self.store.get()?.as_ref()))
    }

    /// Parses (trims and format-checks) and stores a key typed by the user.
    pub fn save(&self, raw: &str) -> AppResult<ApiKey> {
        let key = ApiKey::parse(raw)?;
        self.store.set(&key)?;
        tracing::info!(last4 = %key.last4(), "OpenAI API key saved");
        Ok(key)
    }

    /// The stored key, if any.
    pub fn load(&self) -> AppResult<Option<ApiKey>> {
        self.store.get()
    }

    /// Deletes the stored key.
    pub fn delete(&self) -> AppResult<()> {
        self.store.delete()?;
        tracing::info!("OpenAI API key removed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// In-memory [`SecretStore`] for tests.
    #[derive(Default)]
    struct MemoryStore(Mutex<Option<String>>);

    impl SecretStore for MemoryStore {
        fn get(&self) -> AppResult<Option<ApiKey>> {
            let value = self
                .0
                .lock()
                .map_err(|_| AppError::Internal("lock".into()))?;
            Ok(value.clone().map(ApiKey::from_stored))
        }

        fn set(&self, key: &ApiKey) -> AppResult<()> {
            let mut value = self
                .0
                .lock()
                .map_err(|_| AppError::Internal("lock".into()))?;
            *value = Some(key.expose().to_owned());
            Ok(())
        }

        fn delete(&self) -> AppResult<()> {
            let mut value = self
                .0
                .lock()
                .map_err(|_| AppError::Internal("lock".into()))?;
            *value = None;
            Ok(())
        }
    }

    const FAKE_KEY: &str = "sk-test-0000000000000000000000abcd";

    #[test]
    fn status_is_empty_without_a_key() {
        let credentials = Credentials::new(MemoryStore::default());

        assert_eq!(
            credentials.status().unwrap(),
            ApiKeyStatus {
                has_key: false,
                last4: None
            }
        );
        assert!(credentials.load().unwrap().is_none());
    }

    #[test]
    fn save_trims_stores_and_reports_last4() {
        let credentials = Credentials::new(MemoryStore::default());

        let saved = credentials.save(&format!("  {FAKE_KEY}\n")).unwrap();

        assert_eq!(saved.expose(), FAKE_KEY);
        assert_eq!(credentials.load().unwrap().unwrap().expose(), FAKE_KEY);
        assert_eq!(
            credentials.status().unwrap(),
            ApiKeyStatus {
                has_key: true,
                last4: Some("abcd".into())
            }
        );
    }

    #[test]
    fn save_replaces_the_previous_key() {
        let credentials = Credentials::new(MemoryStore::default());
        credentials.save(FAKE_KEY).unwrap();

        credentials
            .save("sk-test-1111111111111111111111wxyz")
            .unwrap();

        assert_eq!(credentials.status().unwrap().last4.as_deref(), Some("wxyz"));
    }

    #[test]
    fn save_rejects_a_malformed_key_without_storing_it() {
        let credentials = Credentials::new(MemoryStore::default());

        let error = credentials.save("not-a-key-0000000000000").unwrap_err();

        assert_eq!(error.code(), "invalid_api_key");
        assert!(!credentials.status().unwrap().has_key);
    }

    #[test]
    fn delete_removes_the_key_and_is_idempotent() {
        let credentials = Credentials::new(MemoryStore::default());
        credentials.save(FAKE_KEY).unwrap();

        credentials.delete().unwrap();
        credentials.delete().unwrap();

        assert!(!credentials.status().unwrap().has_key);
    }

    #[test]
    fn parse_checks_the_format() {
        assert!(ApiKey::parse(FAKE_KEY).is_ok());
        assert!(ApiKey::parse("sk-proj-AbC_123-xyz0000000000").is_ok());

        for bad in [
            "",
            "   ",
            "pk-0000000000000000000000",
            "sk-short",
            "sk-0000000000 0000000000000",
            "sk-0000000000000000000000ñ",
        ] {
            let error = ApiKey::parse(bad).unwrap_err();
            assert_eq!(error.code(), "invalid_api_key", "accepted {bad:?}");
        }
        assert!(ApiKey::parse(&format!("sk-{}", "a".repeat(600))).is_err());
    }

    #[test]
    fn parse_errors_never_contain_the_key() {
        let raw = "sk-has a space-SECRETPART000000";

        let message = ApiKey::parse(raw).unwrap_err().to_string();

        assert!(!message.contains("SECRETPART"), "{message}");
    }

    #[test]
    fn last4_takes_the_last_four_characters() {
        assert_eq!(ApiKey::from_stored(FAKE_KEY.into()).last4(), "abcd");
        assert_eq!(ApiKey::from_stored("ab".into()).last4(), "ab");
        assert_eq!(ApiKey::from_stored(String::new()).last4(), "");
    }

    #[test]
    fn debug_is_redacted() {
        let key = ApiKey::parse(FAKE_KEY).unwrap();

        let debug = format!("{key:?}");
        let status_debug = format!(
            "{:?}",
            ApiKeyStatus::of(Some(&key)) // status only carries last4
        );

        assert_eq!(debug, "ApiKey(<redacted>)");
        assert!(!status_debug.contains(FAKE_KEY));
    }

    #[test]
    fn credential_errors_do_not_leak_stored_bytes() {
        let error = credential_error(keyring::Error::BadEncoding(FAKE_KEY.as_bytes().to_vec()));

        assert_eq!(error.code(), "credential_store");
        assert!(!error.to_string().contains(FAKE_KEY));
    }

    /// Round trip against the real Windows Credential Manager. It stores a fake key under the
    /// app's real entry and then deletes it, so it would remove a key saved by the app.
    /// Run manually with `cargo test -- --ignored keyring_round_trip`.
    #[test]
    #[ignore = "touches the real Windows Credential Manager"]
    fn keyring_round_trip() {
        let credentials = Credentials::system();

        credentials.save(FAKE_KEY).unwrap();
        assert_eq!(credentials.status().unwrap().last4.as_deref(), Some("abcd"));
        credentials.delete().unwrap();
        assert!(!credentials.status().unwrap().has_key);
    }
}
