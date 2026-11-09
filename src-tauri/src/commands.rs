//! Tauri commands exposed to the panel.
//!
//! Every command the panel can `invoke` lives here. Commands that can fail return
//! [`AppResult`](crate::error::AppResult), so errors reach the panel as `{ code, message }`.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio::{self, AudioDevice, InputLevel, Monitor, MonitorEvent};
use crate::credentials::validation::{KeyValidation, KeyValidator};
use crate::credentials::{ApiKeyStatus, Credentials};
use crate::error::{AppError, AppResult};
use crate::settings::{Settings, SettingsStore};

/// Product name shown to the user.
pub const APP_NAME: &str = "LiveDub Voice";

/// Basic information about the running app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    /// Build profile: `"debug"` or `"release"`.
    pub profile: String,
}

impl AppInfo {
    /// Information about the current build.
    pub fn current() -> Self {
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        Self {
            name: APP_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            profile: profile.to_owned(),
        }
    }
}

/// Returns the app name, version and build profile.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo::current()
}

/// Returns the current settings.
#[tauri::command]
pub fn get_settings(store: State<'_, SettingsStore>) -> AppResult<Settings> {
    store.get()
}

/// Validates and saves the settings, returning what was saved.
#[tauri::command]
pub fn save_settings(store: State<'_, SettingsStore>, settings: Settings) -> AppResult<Settings> {
    store.save(settings)
}

/// Result of `save_api_key`: the key is stored even when the check could not reach OpenAI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveApiKeyResult {
    pub status: ApiKeyStatus,
    pub validation: KeyValidation,
}

/// Whether an OpenAI API key is stored, and its last four characters. Never returns the key.
#[tauri::command]
pub fn get_api_key_status(credentials: State<'_, Credentials>) -> AppResult<ApiKeyStatus> {
    credentials.status()
}

/// Trims, format-checks and stores the key in Windows Credential Manager, then checks it against
/// OpenAI. The key stays stored whatever the check says (for example when offline).
#[tauri::command]
pub async fn save_api_key(
    credentials: State<'_, Credentials>,
    validator: State<'_, KeyValidator>,
    key: String,
) -> AppResult<SaveApiKeyResult> {
    let key = credentials.save(&key)?;
    let validation = validator.validate(&key).await;
    Ok(SaveApiKeyResult {
        status: credentials.status()?,
        validation,
    })
}

/// Checks the stored key against OpenAI again.
#[tauri::command]
pub async fn validate_api_key(
    credentials: State<'_, Credentials>,
    validator: State<'_, KeyValidator>,
) -> AppResult<KeyValidation> {
    let key = credentials.load()?.ok_or(AppError::NoApiKey)?;
    Ok(validator.validate(&key).await)
}

/// Deletes the stored key.
#[tauri::command]
pub fn delete_api_key(credentials: State<'_, Credentials>) -> AppResult<ApiKeyStatus> {
    credentials.delete()?;
    credentials.status()
}

/// Lists the active audio input and output devices.
///
/// Enumeration is blocking COM work, so it runs on a blocking thread (which initializes COM for
/// itself) instead of the async runtime.
#[tauri::command]
pub async fn list_audio_devices() -> AppResult<Vec<AudioDevice>> {
    tauri::async_runtime::spawn_blocking(audio::list_devices)
        .await
        .map_err(|error| AppError::Internal(format!("device listing task failed: {error}")))?
}

/// Event with the microphone test's input level (`InputLevel`), ~20 times per second.
pub const MONITOR_LEVEL_EVENT: &str = "monitor-level";
/// Event sent when the microphone test stops on its own; payload `{ message }`.
pub const MONITOR_STOPPED_EVENT: &str = "monitor-stopped";

/// The running microphone test, if any.
#[derive(Default)]
pub struct MonitorState(Mutex<Option<Monitor>>);

/// Payload of [`MONITOR_STOPPED_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct MonitorStopped {
    pub message: String,
}

/// Starts the microphone test on `input_id` (`null` = default), replacing any running one.
///
/// Emits [`MONITOR_LEVEL_EVENT`] while it runs. With `listen`, also plays the microphone on
/// `output_id` (`null` = default); that fails with a "formats differ, needs resampling" audio
/// error when the two devices do not share sample rate and channel count.
#[tauri::command]
pub async fn start_monitor(
    app: AppHandle,
    input_id: Option<String>,
    output_id: Option<String>,
    listen: bool,
) -> AppResult<()> {
    run_blocking("microphone test", move || {
        let state = app.state::<MonitorState>();
        let mut slot = lock_monitor(&state);
        // Stop the old test first: it may hold the same devices.
        *slot = None;
        let emitter = app.clone();
        let monitor = Monitor::start(
            input_id.as_deref(),
            listen.then_some(output_id.as_deref()),
            move |event| emit_monitor_event(&emitter, event),
        )?;
        *slot = Some(monitor);
        Ok(())
    })
    .await
}

/// Stops the microphone test, if one is running.
#[tauri::command]
pub async fn stop_monitor(app: AppHandle) -> AppResult<()> {
    run_blocking("microphone test", move || {
        let monitor = lock_monitor(&app.state::<MonitorState>()).take();
        drop(monitor);
        Ok(())
    })
    .await
}

fn lock_monitor<'a>(state: &'a MonitorState) -> std::sync::MutexGuard<'a, Option<Monitor>> {
    // A panic while holding the lock leaves at worst a stopped monitor behind: keep going.
    state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn emit_monitor_event(app: &AppHandle, event: MonitorEvent) {
    let result = match event {
        MonitorEvent::Level(level) => app.emit::<InputLevel>(MONITOR_LEVEL_EVENT, level),
        MonitorEvent::Stopped { message } => {
            app.emit(MONITOR_STOPPED_EVENT, MonitorStopped { message })
        }
    };
    if let Err(error) = result {
        tracing::warn!(%error, "could not emit a monitor event");
    }
}

/// Runs blocking audio work (opening devices, joining stream threads) off the async runtime.
async fn run_blocking<T: Send + 'static>(
    what: &str,
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| AppError::Internal(format!("{what} task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_info_reports_name_version_and_profile() {
        let info = app_info();

        assert_eq!(info.name, "LiveDub Voice");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        let expected_profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        assert_eq!(info.profile, expected_profile);
    }

    #[test]
    fn app_info_serializes_to_the_panel_shape() {
        let info = app_info();

        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({ "name": info.name, "version": info.version, "profile": info.profile })
        );
    }
}
