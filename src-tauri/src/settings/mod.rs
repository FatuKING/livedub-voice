//! User settings.
//!
//! The settings (panel language, language direction, devices, source app, ducking volume) are
//! stored as a JSON
//! file (`settings.json`) in Tauri's app config dir
//! (`%APPDATA%\io.github.fatuking.livedubvoice`). The OpenAI API key is never part of this file:
//! it lives only in Windows Credential Manager.
//!
//! File I/O works on plain paths ([`load`], [`save`]) so it can be tested without a Tauri
//! `AppHandle`; [`SettingsStore`] wraps it with the in-memory copy kept in Tauri managed state.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Current version of the settings file format. Bump it when the shape changes, and add the
/// step from the previous version to [`migrate`].
///
/// History: 1 = initial format; 2 = adds `uiLanguage`.
pub const SETTINGS_VERSION: u32 = 2;

/// Name of the settings file inside the app config dir.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// Highest allowed original-audio volume (percent).
pub const MAX_ORIGINAL_VOLUME: u8 = 100;

/// A target language of the translator (`session.audio.output.language`).
///
/// These are the output languages `gpt-realtime-translate` supports, with the codes the API
/// expects, as listed in OpenAI's realtime translation guide
/// (<https://developers.openai.com/cookbook/examples/voice_solutions/realtime_translation_guide>).
/// The source language is auto-detected by the model, so only the target is ever chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "pt")]
    Portuguese,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "zh")]
    Chinese,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "hi")]
    Hindi,
    #[serde(rename = "id")]
    Indonesian,
    #[serde(rename = "vi")]
    Vietnamese,
    #[serde(rename = "it")]
    Italian,
}

impl Language {
    /// Every target language, Spanish and English first.
    pub const ALL: [Language; 13] = [
        Language::Spanish,
        Language::English,
        Language::Portuguese,
        Language::French,
        Language::Japanese,
        Language::Russian,
        Language::Chinese,
        Language::German,
        Language::Korean,
        Language::Hindi,
        Language::Indonesian,
        Language::Vietnamese,
        Language::Italian,
    ];

    /// The code the API expects (and `settings.json` stores), e.g. `"es"`.
    pub const fn code(self) -> &'static str {
        match self {
            Language::Spanish => "es",
            Language::English => "en",
            Language::Portuguese => "pt",
            Language::French => "fr",
            Language::Japanese => "ja",
            Language::Russian => "ru",
            Language::Chinese => "zh",
            Language::German => "de",
            Language::Korean => "ko",
            Language::Hindi => "hi",
            Language::Indonesian => "id",
            Language::Vietnamese => "vi",
            Language::Italian => "it",
        }
    }

    /// The language with this API code, if the translator supports it.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }
}

/// Language of the panel itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiLanguage {
    /// Follow the Windows display language (Spanish for any `es*`, English otherwise).
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "es")]
    Spanish,
}

/// Audio the incoming direction captures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceApp {
    /// Every app on the system except LiveDub Voice itself (avoids feedback loops).
    AllExceptSelf,
    /// A specific process (and its children), by executable name, e.g. `Discord.exe`.
    Process { name: String },
}

/// Incoming direction: other participants → the user's headphones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IncomingSettings {
    pub enabled: bool,
    /// Language the user hears.
    pub target_language: Language,
    pub source_app: SourceApp,
    /// Playback device for the translation; `None` means the system default.
    pub output_device_id: Option<String>,
    /// Volume of the original (untranslated) audio mixed under the translation, 0–100.
    pub original_volume: u8,
}

impl Default for IncomingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            target_language: Language::Spanish,
            source_app: SourceApp::AllExceptSelf,
            output_device_id: None,
            original_volume: 20,
        }
    }
}

/// Outgoing direction: the user's microphone → a virtual cable used as mic by the call/game app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OutgoingSettings {
    pub enabled: bool,
    /// Language the other participants hear.
    pub target_language: Language,
    /// Microphone to capture; `None` means the system default.
    pub input_device_id: Option<String>,
    /// Playback device to write the translation to (normally a virtual cable); `None` means
    /// not chosen yet.
    pub output_device_id: Option<String>,
}

impl Default for OutgoingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            target_language: Language::English,
            input_device_id: None,
            output_device_id: None,
        }
    }
}

/// All user settings, as stored in `settings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// File format version, for future migrations.
    pub version: u32,
    /// Language of the panel (not of the translation).
    pub ui_language: UiLanguage,
    pub incoming: IncomingSettings,
    pub outgoing: OutgoingSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            ui_language: UiLanguage::default(),
            incoming: IncomingSettings::default(),
            outgoing: OutgoingSettings::default(),
        }
    }
}

impl Settings {
    /// Checks that the settings are usable, returning [`AppError::InvalidSettings`] otherwise.
    pub fn validate(&self) -> AppResult<()> {
        if self.version != SETTINGS_VERSION {
            return Err(invalid(format!(
                "unsupported settings version {} (expected {SETTINGS_VERSION})",
                self.version
            )));
        }
        if self.incoming.original_volume > MAX_ORIGINAL_VOLUME {
            return Err(invalid(format!(
                "original audio volume must be between 0 and {MAX_ORIGINAL_VOLUME}, got {}",
                self.incoming.original_volume
            )));
        }
        if let SourceApp::Process { name } = &self.incoming.source_app
            && name.trim().is_empty()
        {
            return Err(invalid("source app process name must not be empty".into()));
        }
        check_device_id("incoming output device", &self.incoming.output_device_id)?;
        check_device_id("outgoing input device", &self.outgoing.input_device_id)?;
        check_device_id("outgoing output device", &self.outgoing.output_device_id)?;
        Ok(())
    }
}

fn invalid(message: String) -> AppError {
    AppError::InvalidSettings(message)
}

fn check_device_id(label: &str, id: &Option<String>) -> AppResult<()> {
    match id {
        Some(id) if id.trim().is_empty() => Err(invalid(format!(
            "{label} id must not be empty (use null for the default device)"
        ))),
        _ => Ok(()),
    }
}

/// Upgrades a settings file read from disk to [`SETTINGS_VERSION`], one version at a time.
///
/// A file without `version` is taken as current (fields it lacks get their defaults). A version
/// newer than this build knows is left alone, so [`Settings::validate`] rejects it.
pub fn migrate(value: &mut serde_json::Value) -> Result<(), String> {
    let Some(object) = value.as_object_mut() else {
        return Err("settings file is not a JSON object".into());
    };
    let Some(version) = object.get("version") else {
        return Ok(());
    };
    let mut version = version
        .as_u64()
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| format!("invalid settings version {version}"))?;

    if version == 1 {
        // 1 → 2: the panel language setting, following the OS by default.
        object
            .entry("uiLanguage")
            .or_insert_with(|| serde_json::json!("system"));
        version = 2;
    }

    object.insert("version".into(), version.into());
    Ok(())
}

/// Resets a target language this build does not know (e.g. one written by a newer version) to
/// that direction's default, with a warning, instead of discarding the whole file.
fn reset_unknown_languages(value: &mut serde_json::Value) {
    let defaults = [
        ("incoming", IncomingSettings::default().target_language),
        ("outgoing", OutgoingSettings::default().target_language),
    ];
    for (direction, default) in defaults {
        let Some(slot) = value
            .get_mut(direction)
            .and_then(|settings| settings.get_mut("targetLanguage"))
        else {
            continue;
        };
        if slot.as_str().and_then(Language::from_code).is_none() {
            tracing::warn!(
                direction,
                language = %slot,
                default = default.code(),
                "unknown target language in settings, using the default"
            );
            *slot = default.code().into();
        }
    }
}

/// Loads settings from `path`.
///
/// Files from older versions are migrated (see [`migrate`]); the upgraded file is written on the
/// next save. Never fails: a missing file yields the defaults, and a file that cannot be read,
/// parsed or validated is moved aside to `<file>.bak` (best effort) and the defaults are returned.
pub fn load(path: &Path) -> Settings {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::info!(path = %path.display(), "no settings file, using defaults");
            return Settings::default();
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not read settings, using defaults");
            return Settings::default();
        }
    };

    let parsed = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|error| error.to_string())
        .and_then(|mut value| {
            migrate(&mut value)?;
            reset_unknown_languages(&mut value);
            serde_json::from_value::<Settings>(value).map_err(|error| error.to_string())
        })
        .and_then(|settings| {
            settings
                .validate()
                .map(|()| settings)
                .map_err(|error| error.to_string())
        });

    match parsed {
        Ok(settings) => settings,
        Err(error) => {
            let backup = backup_path(path);
            match fs::rename(path, &backup) {
                Ok(()) => tracing::warn!(
                    path = %path.display(),
                    backup = %backup.display(),
                    %error,
                    "invalid settings file, backed it up and using defaults"
                ),
                Err(rename_error) => tracing::warn!(
                    path = %path.display(),
                    %error,
                    %rename_error,
                    "invalid settings file, could not back it up; using defaults"
                ),
            }
            Settings::default()
        }
    }
}

/// Saves `settings` to `path` atomically: writes a temp file in the same directory, flushes it
/// to disk and renames it over the target. Creates the parent directory if needed.
pub fn save(path: &Path, settings: &Settings) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|error| AppError::Internal(format!("could not serialize settings: {error}")))?;

    let tmp = with_suffix(path, ".tmp");
    let write_result = (|| -> io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error.into());
    }
    Ok(())
}

/// Path of the backup kept for an invalid settings file.
pub fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Settings file location plus the current settings, kept in Tauri managed state.
#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    /// Loads the settings from `path` (see [`load`]).
    pub fn load(path: PathBuf) -> Self {
        let current = load(&path);
        Self {
            path,
            current: Mutex::new(current),
        }
    }

    /// Path of the settings file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A copy of the current settings.
    pub fn get(&self) -> AppResult<Settings> {
        Ok(self.lock()?.clone())
    }

    /// Validates `settings`, writes them to disk and makes them current.
    pub fn save(&self, settings: Settings) -> AppResult<Settings> {
        settings.validate()?;
        let mut current = self.lock()?;
        save(&self.path, &settings)?;
        *current = settings.clone();
        tracing::info!(path = %self.path.display(), "settings saved");
        Ok(settings)
    }

    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Settings>> {
        self.current
            .lock()
            .map_err(|_| AppError::Internal("settings lock poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("nested").join(SETTINGS_FILE_NAME)
    }

    #[test]
    fn default_json_shape_is_stable() {
        assert_eq!(
            serde_json::to_value(Settings::default()).unwrap(),
            json!({
                "version": 2,
                "uiLanguage": "system",
                "incoming": {
                    "enabled": false,
                    "targetLanguage": "es",
                    "sourceApp": { "kind": "allExceptSelf" },
                    "outputDeviceId": null,
                    "originalVolume": 20
                },
                "outgoing": {
                    "enabled": false,
                    "targetLanguage": "en",
                    "inputDeviceId": null,
                    "outputDeviceId": null
                }
            })
        );
    }

    #[test]
    fn process_source_app_serializes_with_name() {
        let source = SourceApp::Process {
            name: "Discord.exe".into(),
        };
        assert_eq!(
            serde_json::to_value(&source).unwrap(),
            json!({ "kind": "process", "name": "Discord.exe" })
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        let mut settings = Settings::default();
        settings.incoming.enabled = true;
        settings.incoming.target_language = Language::English;
        settings.incoming.source_app = SourceApp::Process {
            name: "Discord.exe".into(),
        };
        settings.incoming.original_volume = 55;
        settings.outgoing.output_device_id = Some("cable-input".into());

        save(&path, &settings).unwrap();

        assert_eq!(load(&path), settings);
        assert!(!with_suffix(&path, ".tmp").exists());
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(load(&settings_path(&dir)), Settings::default());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        fs::write(
            &path,
            r#"{ "version": 2, "incoming": { "enabled": true } }"#,
        )
        .unwrap();

        let settings = load(&path);

        assert!(settings.incoming.enabled);
        assert_eq!(settings.incoming.original_volume, 20);
        assert_eq!(settings.outgoing, OutgoingSettings::default());
    }

    #[test]
    fn ui_language_defaults_to_system_and_serializes_lowercase() {
        assert_eq!(Settings::default().ui_language, UiLanguage::System);
        assert_eq!(
            serde_json::to_value(UiLanguage::Spanish).unwrap(),
            json!("es")
        );
        assert_eq!(
            serde_json::from_value::<UiLanguage>(json!("en")).unwrap(),
            UiLanguage::English
        );
    }

    #[test]
    fn version_1_file_migrates_with_default_ui_language() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        let v1 = json!({
            "version": 1,
            "incoming": {
                "enabled": true,
                "targetLanguage": "en",
                "sourceApp": { "kind": "process", "name": "Discord.exe" },
                "outputDeviceId": "spk",
                "originalVolume": 35
            },
            "outgoing": {
                "enabled": false,
                "targetLanguage": "es",
                "inputDeviceId": null,
                "outputDeviceId": "cable-input"
            }
        });
        fs::write(&path, v1.to_string()).unwrap();

        let settings = load(&path);

        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.ui_language, UiLanguage::System);
        assert!(settings.incoming.enabled);
        assert_eq!(settings.incoming.target_language, Language::English);
        assert_eq!(
            settings.incoming.source_app,
            SourceApp::Process {
                name: "Discord.exe".into()
            }
        );
        assert_eq!(settings.incoming.original_volume, 35);
        assert_eq!(
            settings.outgoing.output_device_id.as_deref(),
            Some("cable-input")
        );
        // Migrating is not an error: the file stays where it is, no backup.
        assert!(path.exists());
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn migrate_keeps_a_ui_language_already_present() {
        let mut value = json!({ "version": 1, "uiLanguage": "es" });

        migrate(&mut value).unwrap();

        assert_eq!(value, json!({ "version": 2, "uiLanguage": "es" }));
    }

    #[test]
    fn migrate_leaves_newer_versions_for_validation() {
        let mut value = json!({ "version": 99 });

        migrate(&mut value).unwrap();

        assert_eq!(value, json!({ "version": 99 }));
    }

    #[test]
    fn current_file_round_trips_ui_language() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        let settings = Settings {
            ui_language: UiLanguage::Spanish,
            ..Settings::default()
        };

        save(&path, &settings).unwrap();

        assert_eq!(load(&path).ui_language, UiLanguage::Spanish);
    }

    #[test]
    fn every_language_round_trips_through_its_code() {
        for language in Language::ALL {
            let value = serde_json::to_value(language).unwrap();
            assert_eq!(value, json!(language.code()));
            assert_eq!(serde_json::from_value::<Language>(value).unwrap(), language);
            assert_eq!(Language::from_code(language.code()), Some(language));
        }
    }

    #[test]
    fn language_codes_match_the_api() {
        let codes: Vec<_> = Language::ALL.iter().map(|l| l.code()).collect();
        assert_eq!(
            codes,
            [
                "es", "en", "pt", "fr", "ja", "ru", "zh", "de", "ko", "hi", "id", "vi", "it"
            ]
        );
        assert_eq!(Language::from_code("xx"), None);
    }

    #[test]
    fn spanish_english_file_still_loads_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        let file = json!({
            "version": 2,
            "uiLanguage": "es",
            "incoming": {
                "enabled": true,
                "targetLanguage": "en",
                "sourceApp": { "kind": "allExceptSelf" },
                "outputDeviceId": null,
                "originalVolume": 20
            },
            "outgoing": {
                "enabled": false,
                "targetLanguage": "es",
                "inputDeviceId": null,
                "outputDeviceId": "cable-input"
            }
        });
        fs::write(&path, file.to_string()).unwrap();

        let settings = load(&path);

        assert_eq!(settings.incoming.target_language, Language::English);
        assert_eq!(settings.outgoing.target_language, Language::Spanish);
        assert_eq!(serde_json::to_value(&settings).unwrap(), file);
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn new_language_saves_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        let mut settings = Settings::default();
        settings.incoming.target_language = Language::Japanese;
        settings.outgoing.target_language = Language::Portuguese;

        save(&path, &settings).unwrap();

        assert_eq!(load(&path), settings);
    }

    #[test]
    fn unknown_language_falls_back_to_the_direction_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        fs::write(
            &path,
            r#"{
                "version": 2,
                "incoming": { "enabled": true, "targetLanguage": "xx", "originalVolume": 40 },
                "outgoing": { "targetLanguage": 7, "outputDeviceId": "cable-input" }
            }"#,
        )
        .unwrap();

        let settings = load(&path);

        assert_eq!(
            settings.incoming.target_language,
            IncomingSettings::default().target_language
        );
        assert_eq!(
            settings.outgoing.target_language,
            OutgoingSettings::default().target_language
        );
        // The rest of the file is kept, and it is not treated as corrupt.
        assert!(settings.incoming.enabled);
        assert_eq!(settings.incoming.original_volume, 40);
        assert_eq!(
            settings.outgoing.output_device_id.as_deref(),
            Some("cable-input")
        );
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn corrupt_file_loads_defaults_and_keeps_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        fs::write(&path, "{ not json").unwrap();

        assert_eq!(load(&path), Settings::default());
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(backup_path(&path)).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn invalid_values_in_file_load_defaults_and_keep_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE_NAME);
        fs::write(
            &path,
            r#"{ "version": 2, "incoming": { "originalVolume": 150 } }"#,
        )
        .unwrap();

        assert_eq!(load(&path), Settings::default());
        assert!(backup_path(&path).exists());
    }

    #[test]
    fn validate_accepts_defaults() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn validate_rejects_volume_out_of_range() {
        let mut settings = Settings::default();
        settings.incoming.original_volume = 101;

        let error = settings.validate().unwrap_err();

        assert_eq!(error.code(), "invalid_settings");
        assert!(error.to_string().contains("between 0 and 100"));
    }

    #[test]
    fn validate_rejects_empty_process_name() {
        let mut settings = Settings::default();
        settings.incoming.source_app = SourceApp::Process { name: "  ".into() };

        assert!(matches!(
            settings.validate(),
            Err(AppError::InvalidSettings(_))
        ));
    }

    #[test]
    fn validate_rejects_empty_device_id() {
        let mut settings = Settings::default();
        settings.outgoing.input_device_id = Some(String::new());

        assert!(matches!(
            settings.validate(),
            Err(AppError::InvalidSettings(_))
        ));
    }

    #[test]
    fn validate_rejects_unknown_version() {
        let settings = Settings {
            version: 99,
            ..Settings::default()
        };

        assert!(matches!(
            settings.validate(),
            Err(AppError::InvalidSettings(_))
        ));
    }

    #[test]
    fn store_save_validates_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(&dir);
        let store = SettingsStore::load(path.clone());

        let mut invalid = Settings::default();
        invalid.incoming.original_volume = 200;
        assert!(store.save(invalid).is_err());
        assert!(!path.exists());

        let mut valid = Settings::default();
        valid.outgoing.enabled = true;
        store.save(valid.clone()).unwrap();

        assert_eq!(store.get().unwrap(), valid);
        assert_eq!(SettingsStore::load(path).get().unwrap(), valid);
    }
}
