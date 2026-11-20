//! Tauri commands exposed to the panel.
//!
//! Every command the panel can `invoke` lives here. Commands that can fail return
//! [`AppResult`](crate::error::AppResult), so errors reach the panel as `{ code, message }`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio::{self, AudioApp, AudioDevice, InputLevel, Monitor, MonitorEvent};
use crate::credentials::validation::{KeyValidation, KeyValidator};
use crate::credentials::{ApiKeyStatus, Credentials};
use crate::error::{AppError, AppResult};
use crate::pipeline::{
    EventSink, PipelineConfig, PipelineEvent, PipelineStatus, TranslationPipeline, WasapiIo,
    incoming, outgoing,
};
use crate::settings::{Settings, SettingsStore};
use crate::translator::{FakeTranslator, OpenAiTranslator, Translator};

/// Product name shown to the user.
pub const APP_NAME: &str = "LiveDub Voice";

/// Basic information about the running app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    /// Build profile: `"debug"` or `"release"`.
    pub profile: String,
    /// Whether `LIVEDUB_FAKE_TRANSLATOR` replaces OpenAI (no API key needed).
    pub fake_translator: bool,
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
            fake_translator: FakeTranslator::from_env().is_some(),
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

/// Validates and saves the settings, returning what was saved. A running incoming translation
/// applies the new original-audio volume at once.
#[tauri::command]
pub fn save_settings(
    store: State<'_, SettingsStore>,
    incoming: State<'_, IncomingState>,
    settings: Settings,
) -> AppResult<Settings> {
    let saved = store.save(settings)?;
    incoming.set_original_volume(saved.incoming.original_volume);
    Ok(saved)
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

/// Lists the apps that have audio sessions (the incoming direction's possible sources), grouped
/// by app: playing ones first, then by name. LiveDub Voice itself is never listed.
#[tauri::command]
pub async fn list_audio_apps() -> AppResult<Vec<AudioApp>> {
    run_blocking("audio app listing", audio::list_audio_apps).await
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

/// Names of the events a direction's pipeline sends to the panel.
struct EventNames {
    /// The pipeline's status (`PipelineStatus`: `{ state, error }`).
    status: &'static str,
    /// The input level (`InputLevel`), ~20 times per second while live.
    level: &'static str,
    /// A delta of the translated text; payload `{ delta }`.
    transcript: &'static str,
    /// The seconds of input audio sent since start; payload `{ seconds }`.
    audio_sent: &'static str,
}

/// Event with the outgoing pipeline's status (`PipelineStatus`: `{ state, error }`).
pub const OUTGOING_STATUS_EVENT: &str = "outgoing-status";
/// Event with the outgoing microphone level (`InputLevel`), ~20 times per second while live.
pub const OUTGOING_LEVEL_EVENT: &str = "outgoing-level";
/// Event with a delta of the translated text; payload `{ delta }`.
pub const OUTGOING_TRANSCRIPT_EVENT: &str = "outgoing-transcript";
/// Event with the seconds of microphone audio sent since start; payload `{ seconds }`.
pub const OUTGOING_AUDIO_SENT_EVENT: &str = "outgoing-audio-sent";

/// Event with the incoming pipeline's status (`PipelineStatus`: `{ state, error }`).
pub const INCOMING_STATUS_EVENT: &str = "incoming-status";
/// Event with the level of the captured apps (`InputLevel`, as the translator hears them, before
/// ducking), ~20 times per second while live.
pub const INCOMING_LEVEL_EVENT: &str = "incoming-level";
/// Event with a delta of the translated text (what the user hears); payload `{ delta }`.
pub const INCOMING_TRANSCRIPT_EVENT: &str = "incoming-transcript";
/// Event with the seconds of app audio sent since start; payload `{ seconds }`.
pub const INCOMING_AUDIO_SENT_EVENT: &str = "incoming-audio-sent";

const OUTGOING_EVENTS: EventNames = EventNames {
    status: OUTGOING_STATUS_EVENT,
    level: OUTGOING_LEVEL_EVENT,
    transcript: OUTGOING_TRANSCRIPT_EVENT,
    audio_sent: OUTGOING_AUDIO_SENT_EVENT,
};

const INCOMING_EVENTS: EventNames = EventNames {
    status: INCOMING_STATUS_EVENT,
    level: INCOMING_LEVEL_EVENT,
    transcript: INCOMING_TRANSCRIPT_EVENT,
    audio_sent: INCOMING_AUDIO_SENT_EVENT,
};

/// One direction's pipeline (at most one) and its latest status.
#[derive(Default)]
pub struct PipelineSlot {
    pipeline: tokio::sync::Mutex<Option<TranslationPipeline>>,
    status: Arc<Mutex<PipelineStatus>>,
}

impl PipelineSlot {
    /// Starts the pipeline unless it already runs. `prepare` reads the settings and checks the
    /// devices; its failure is reported like a start failure.
    async fn start(
        &self,
        events: EventSink,
        prepare: impl Future<Output = AppResult<(PipelineConfig, Arc<dyn Translator>, WasapiIo)>>,
    ) -> AppResult<()> {
        let mut slot = self.pipeline.lock().await;
        if slot
            .as_ref()
            .is_some_and(|pipeline| !pipeline.is_finished())
        {
            return Ok(());
        }
        if let Some(finished) = slot.take() {
            finished.stop().await;
        }
        let (config, translator, io) = match prepare.await {
            Ok(ready) => ready,
            Err(error) => {
                tracing::warn!(code = error.code(), %error, "translation refused to start");
                events(PipelineEvent::Status(PipelineStatus::error(&error)));
                return Err(error);
            }
        };
        // The pipeline reports its own start failures.
        let pipeline = TranslationPipeline::start(config, translator, Arc::new(io), events).await?;
        *slot = Some(pipeline);
        Ok(())
    }

    /// Stops the pipeline, if it runs, and waits until it has stopped.
    async fn stop(&self) {
        // Keep the slot locked while it stops (up to ~10 s finishing the last sentence), so a
        // start cannot open the devices before they are released.
        let mut slot = self.pipeline.lock().await;
        if let Some(pipeline) = slot.take() {
            pipeline.stop().await;
        }
    }

    fn status(&self) -> PipelineStatus {
        lock_status(&self.status).clone()
    }
}

/// The outgoing pipeline (microphone → virtual cable).
#[derive(Default)]
pub struct OutgoingState(PipelineSlot);

/// The incoming pipeline (other apps → headphones).
#[derive(Default)]
pub struct IncomingState(PipelineSlot);

impl IncomingState {
    /// Applies a new original-audio volume to a running incoming translation. Never waits: if
    /// the pipeline is starting or stopping, the new volume applies on its next start.
    fn set_original_volume(&self, percent: u8) {
        if let Ok(slot) = self.0.pipeline.try_lock()
            && let Some(pipeline) = slot.as_ref()
        {
            pipeline.set_original_volume(percent);
        }
    }

    /// Restores the apps a running incoming translation ducked (the app is exiting). Blocking.
    pub fn release_ducking(&self) {
        if let Ok(slot) = self.0.pipeline.try_lock()
            && let Some(pipeline) = slot.as_ref()
        {
            pipeline.release_ducking();
        }
    }
}

/// Where ducked app volumes are remembered until restored (`ducking.json` in the app config dir).
pub struct DuckingJournal(pub PathBuf);

/// Payload of the `*-transcript` events.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptDelta {
    pub delta: String,
}

/// Payload of the `*-audio-sent` events.
#[derive(Debug, Clone, Serialize)]
pub struct AudioSent {
    pub seconds: f64,
}

/// Starts translating the microphone into the virtual cable, with the saved `outgoing` settings.
///
/// Does nothing if it is already running. Fails (and emits an `error` status) without an API key
/// (unless the fake translator is on), without an output device, when a chosen device is not
/// connected, or when the devices or the translator cannot start. An output that is not a known
/// virtual cable is allowed, with a warning in the log. Emits the `outgoing-*` events while it
/// runs, and stops on its own on fatal errors (device unplugged, key rejected, no model access).
#[tauri::command]
pub async fn start_outgoing(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    credentials: State<'_, Credentials>,
    state: State<'_, OutgoingState>,
) -> AppResult<()> {
    let slot = &state.0;
    let events = event_sink(app, Arc::clone(&slot.status), OUTGOING_EVENTS);
    slot.start(events, async {
        let settings = store.get()?.outgoing;
        let translator = translator(&credentials)?;
        let devices = list_audio_devices().await?;
        if !outgoing::check_devices(&settings, &devices)? {
            tracing::warn!("the outgoing output is not a known virtual cable");
        }
        Ok((outgoing::config(&settings), translator, WasapiIo::default()))
    })
    .await
}

/// Stops the outgoing translation, if it is running, and waits until it has stopped.
#[tauri::command]
pub async fn stop_outgoing(state: State<'_, OutgoingState>) -> AppResult<()> {
    state.0.stop().await;
    Ok(())
}

/// The outgoing pipeline's latest status (for a panel that opens while it runs).
#[tauri::command]
pub fn get_outgoing_status(state: State<'_, OutgoingState>) -> PipelineStatus {
    state.0.status()
}

/// Starts translating the source app (or the whole system minus LiveDub Voice) into the
/// headphones, with the saved `incoming` settings, ducking the original audio while it runs.
///
/// Does nothing if it is already running. Fails (and emits an `error` status) without an API key
/// (unless the fake translator is on), when the chosen headphones are not connected, when the
/// chosen app is not running (`source_app_not_running`), or when the devices or the translator
/// cannot start. Emits the `incoming-*` events while it runs, and stops on its own on fatal
/// errors (headphones unplugged, captured app closed, key rejected). The ducked apps get their
/// volume back whenever it stops.
#[tauri::command]
pub async fn start_incoming(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    credentials: State<'_, Credentials>,
    journal: State<'_, DuckingJournal>,
    state: State<'_, IncomingState>,
) -> AppResult<()> {
    let slot = &state.0;
    let events = event_sink(app, Arc::clone(&slot.status), INCOMING_EVENTS);
    slot.start(events, async {
        let settings = store.get()?.incoming;
        let translator = translator(&credentials)?;
        let devices = list_audio_devices().await?;
        incoming::check_devices(&settings, &devices)?;
        let io = WasapiIo::with_ducking_journal(journal.0.clone());
        Ok((incoming::config(&settings), translator, io))
    })
    .await
}

/// Stops the incoming translation, if it is running, and waits until it has stopped (and the
/// original audio is back to its volume).
#[tauri::command]
pub async fn stop_incoming(state: State<'_, IncomingState>) -> AppResult<()> {
    state.0.stop().await;
    Ok(())
}

/// The incoming pipeline's latest status (for a panel that opens while it runs).
#[tauri::command]
pub fn get_incoming_status(state: State<'_, IncomingState>) -> PipelineStatus {
    state.0.status()
}

/// The fake translator when `LIVEDUB_FAKE_TRANSLATOR` is set, OpenAI with the stored key
/// otherwise.
fn translator(credentials: &Credentials) -> AppResult<Arc<dyn Translator>> {
    if let Some(fake) = FakeTranslator::from_env() {
        tracing::info!(mode = ?fake.mode(), "using the fake translator");
        return Ok(Arc::new(fake));
    }
    let key = credentials.load()?.ok_or(AppError::NoApiKey)?;
    Ok(Arc::new(OpenAiTranslator::new(key)))
}

/// Forwards a pipeline's events to the panel, remembering the latest status.
fn event_sink(app: AppHandle, status: Arc<Mutex<PipelineStatus>>, names: EventNames) -> EventSink {
    Arc::new(move |event| {
        let result = match event {
            PipelineEvent::Status(new) => {
                *lock_status(&status) = new.clone();
                app.emit(names.status, new)
            }
            PipelineEvent::Level(level) => app.emit(names.level, level),
            PipelineEvent::TranscriptDelta(delta) => {
                app.emit(names.transcript, TranscriptDelta { delta })
            }
            PipelineEvent::AudioSent { seconds } => {
                app.emit(names.audio_sent, AudioSent { seconds })
            }
        };
        if let Err(error) = result {
            tracing::warn!(%error, event = names.status, "could not emit a pipeline event");
        }
    })
}

fn lock_status(status: &Mutex<PipelineStatus>) -> std::sync::MutexGuard<'_, PipelineStatus> {
    status
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            json!({
                "name": info.name,
                "version": info.version,
                "profile": info.profile,
                "fakeTranslator": info.fake_translator
            })
        );
    }
}
