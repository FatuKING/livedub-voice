//! Tauri commands exposed to the panel.
//!
//! Every command the panel can `invoke` lives here. Commands that can fail return
//! [`AppResult`](crate::error::AppResult), so errors reach the panel as `{ code, message }`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio::{self, AudioApp, AudioDevice, InputLevel, Monitor, MonitorEvent};
use crate::credentials::validation::{KeyValidation, KeyValidator};
use crate::credentials::{ApiKeyStatus, Credentials};
use crate::error::{AppError, AppResult};
use crate::hotkeys::{self, Apply, HotkeyStatus, Hotkeys, PluginRegistrar};
use crate::pipeline::voice::VoiceActivity;
use crate::pipeline::{
    Direction, EventSink, PipelineEvent, PipelineManager, PipelineStatus, StartPlan, WasapiIo,
    incoming, outgoing,
};
use crate::settings::{Settings, SettingsStore};
use crate::translator::{FakeTranslator, OpenAiTranslator, Translator};
use crate::tray::{self, TrayLabels};
use crate::usage::{UsageMeter, UsageNotice, UsageNotify, UsageSnapshot, metered_sink};

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

/// Validates and saves the settings, returning what was saved. `trayNoticeShown` and
/// `onboardingCompleted` keep the core's value (see [`SettingsStore::save_from_panel`]).
///
/// A running incoming translation applies the new original-audio volume at once, the spending
/// cap applies at once (a `usage-updated` event follows), and the keyboard shortcuts are
/// registered again at once; every other change applies the next time its direction starts (see
/// [`crate::pipeline::manager`]). `closeQuitsApp` applies to the next close.
///
/// A shortcut the user changed that Windows refuses (`hotkey_unavailable`) or that types a
/// character on the keyboard layout (`hotkey_types_character`) fails the save: nothing is saved
/// and the previous shortcuts stay registered.
#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    pipelines: State<'_, PipelineManager>,
    meter: State<'_, Arc<UsageMeter>>,
    hotkeys: State<'_, Hotkeys>,
    settings: Settings,
) -> AppResult<Settings> {
    settings.validate()?;
    let registrar = PluginRegistrar(&app);
    hotkeys.apply(&registrar, hotkeys::plan(&settings), Apply::Strict)?;
    let saved = match store.save_from_panel(settings) {
        Ok(saved) => saved,
        Err(error) => {
            // Back to the shortcuts of the settings still in force.
            if let Ok(current) = store.get() {
                let _ = hotkeys.apply(&registrar, hotkeys::plan(&current), Apply::Lenient);
            }
            return Err(error);
        }
    };
    pipelines.set_original_volume(saved.incoming.original_volume);
    meter.set_cap(saved.spending_cap);
    emit_usage(&app, &meter);
    Ok(saved)
}

/// Event with the usage (`UsageSnapshot`): whenever it changes, about once a second per running
/// direction.
pub const USAGE_UPDATED_EVENT: &str = "usage-updated";
/// Event sent when the spending cap is reached and the running translations are being stopped;
/// payload `CapStatus`.
pub const SPENDING_CAP_REACHED_EVENT: &str = "spending-cap-reached";

/// Usage of the current sessions, today, this month and per day, plus the spending cap status.
#[tauri::command]
pub fn get_usage(meter: State<'_, Arc<UsageMeter>>) -> UsageSnapshot {
    meter.snapshot()
}

/// Keeps translating over the spending cap for the rest of today (local time). Returns the new
/// usage snapshot.
#[tauri::command]
pub async fn override_spending_cap(
    app: AppHandle,
    meter: State<'_, Arc<UsageMeter>>,
) -> AppResult<UsageSnapshot> {
    meter.override_today();
    tracing::info!("spending cap overridden for today");
    let flushing = Arc::clone(meter.inner());
    run_blocking("usage history", move || Ok(flushing.flush()?)).await?;
    emit_usage(&app, &meter);
    Ok(meter.snapshot())
}

fn emit_usage(app: &AppHandle, meter: &UsageMeter) {
    if let Err(error) = app.emit(USAGE_UPDATED_EVENT, meter.snapshot()) {
        tracing::warn!(%error, "could not emit the usage");
    }
}

/// Writes the usage history off the async runtime, logging a failure.
fn flush_usage_in_background(meter: Arc<UsageMeter>) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = meter.flush() {
            tracing::warn!(%error, "could not write the usage history");
        }
    });
}

/// Turns the meter's notices into panel events, disk writes and, when the cap is reached, a
/// graceful stop of both directions through the manager.
fn usage_notify(app: AppHandle, meter: Arc<UsageMeter>) -> UsageNotify {
    Arc::new(move |notice| match notice {
        UsageNotice::Updated => emit_usage(&app, &meter),
        UsageNotice::SessionEnded => {
            emit_usage(&app, &meter);
            flush_usage_in_background(Arc::clone(&meter));
        }
        UsageNotice::CapReached(status) => {
            if let Err(error) = app.emit(SPENDING_CAP_REACHED_EVENT, status) {
                tracing::warn!(%error, "could not emit the spending cap event");
            }
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                app.state::<PipelineManager>().stop_all().await;
            });
        }
    })
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
    /// The pipeline's status (`PipelineStatus`: `{ state, error, reconnect }`).
    status: &'static str,
    /// The input level (`InputLevel`), ~20 times per second while live.
    level: &'static str,
    /// A delta of the translated text; payload `{ delta }`.
    transcript: &'static str,
    /// The seconds of input audio sent since start; payload `{ seconds }`.
    audio_sent: &'static str,
    /// The voice indicator (`VoiceActivity`: `{ speaking, sending }`), on every change.
    voice: &'static str,
    /// The direction stopped itself after no speech for the auto-off time; payload
    /// `{ minutes }`.
    auto_off: &'static str,
}

/// Event with the outgoing pipeline's status (`PipelineStatus`: `{ state, error, reconnect }`).
pub const OUTGOING_STATUS_EVENT: &str = "outgoing-status";
/// Event with the outgoing microphone level (`InputLevel`), ~20 times per second while live.
pub const OUTGOING_LEVEL_EVENT: &str = "outgoing-level";
/// Event with a delta of the translated text; payload `{ delta }`.
pub const OUTGOING_TRANSCRIPT_EVENT: &str = "outgoing-transcript";
/// Event with the seconds of microphone audio sent since start; payload `{ seconds }`.
pub const OUTGOING_AUDIO_SENT_EVENT: &str = "outgoing-audio-sent";
/// Event with the outgoing voice indicator (`VoiceActivity`), on every change.
pub const OUTGOING_VOICE_EVENT: &str = "outgoing-voice";
/// Event sent when the outgoing translation stops after no speech for the auto-off time;
/// payload `{ minutes }`.
pub const OUTGOING_AUTO_OFF_EVENT: &str = "outgoing-auto-off";

/// Event with the incoming pipeline's status (`PipelineStatus`: `{ state, error, reconnect }`).
pub const INCOMING_STATUS_EVENT: &str = "incoming-status";
/// Event with the level of the captured apps (`InputLevel`, as the translator hears them, before
/// ducking), ~20 times per second while live.
pub const INCOMING_LEVEL_EVENT: &str = "incoming-level";
/// Event with a delta of the translated text (what the user hears); payload `{ delta }`.
pub const INCOMING_TRANSCRIPT_EVENT: &str = "incoming-transcript";
/// Event with the seconds of app audio sent since start; payload `{ seconds }`.
pub const INCOMING_AUDIO_SENT_EVENT: &str = "incoming-audio-sent";
/// Event with the incoming voice indicator (`VoiceActivity`), on every change.
pub const INCOMING_VOICE_EVENT: &str = "incoming-voice";
/// Event sent when the incoming translation stops after no speech for the auto-off time;
/// payload `{ minutes }`.
pub const INCOMING_AUTO_OFF_EVENT: &str = "incoming-auto-off";

const OUTGOING_EVENTS: EventNames = EventNames {
    status: OUTGOING_STATUS_EVENT,
    level: OUTGOING_LEVEL_EVENT,
    transcript: OUTGOING_TRANSCRIPT_EVENT,
    audio_sent: OUTGOING_AUDIO_SENT_EVENT,
    voice: OUTGOING_VOICE_EVENT,
    auto_off: OUTGOING_AUTO_OFF_EVENT,
};

const INCOMING_EVENTS: EventNames = EventNames {
    status: INCOMING_STATUS_EVENT,
    level: INCOMING_LEVEL_EVENT,
    transcript: INCOMING_TRANSCRIPT_EVENT,
    audio_sent: INCOMING_AUDIO_SENT_EVENT,
    voice: INCOMING_VOICE_EVENT,
    auto_off: INCOMING_AUTO_OFF_EVENT,
};

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

/// Payload of the `*-auto-off` events.
#[derive(Debug, Clone, Serialize)]
pub struct AutoOff {
    /// The auto-off time that passed without speech.
    pub minutes: u64,
}

/// Starts translating the microphone into the virtual cable, with the saved `outgoing` settings.
///
/// Does nothing if it is already running. Fails (and emits an `error` status) without an API key
/// (unless the fake translator is on), without an output device, when a chosen device is not
/// connected, or when the devices or the translator cannot start. An output that is not a known
/// virtual cable is allowed, with a warning in the log. Emits the `outgoing-*` events while it
/// runs; reconnects on its own when the connection drops, and stops on fatal errors (device
/// unplugged, key rejected, no quota, no model access) or when reconnecting fails.
///
/// Refused with `spending_cap_reached` (no status event) while the spending cap is reached and
/// not overridden for today.
#[tauri::command]
pub async fn start_outgoing(app: AppHandle) -> AppResult<()> {
    start_translation(&app, Direction::Outgoing).await
}

/// Stops the outgoing translation, if it is running, and waits until it has stopped.
#[tauri::command]
pub async fn stop_outgoing(pipelines: State<'_, PipelineManager>) -> AppResult<()> {
    pipelines.stop(Direction::Outgoing).await;
    Ok(())
}

/// The outgoing pipeline's latest status (for a panel that opens while it runs).
#[tauri::command]
pub fn get_outgoing_status(pipelines: State<'_, PipelineManager>) -> PipelineStatus {
    pipelines.status(Direction::Outgoing)
}

/// Starts translating the source app (or the whole system minus LiveDub Voice) into the
/// headphones, with the saved `incoming` settings, ducking the original audio while it runs.
///
/// Does nothing if it is already running. Fails (and emits an `error` status) without an API key
/// (unless the fake translator is on), when the chosen headphones are not connected, when the
/// chosen app is not running (`source_app_not_running`), or when the devices or the translator
/// cannot start. Emits the `incoming-*` events while it runs; reconnects on its own when the
/// connection drops (the apps stay ducked meanwhile), and stops on fatal errors (headphones
/// unplugged, captured app closed, key rejected, no quota) or when reconnecting fails. The
/// ducked apps get their volume back whenever it stops.
///
/// Refused with `spending_cap_reached` (no status event) while the spending cap is reached and
/// not overridden for today.
#[tauri::command]
pub async fn start_incoming(app: AppHandle) -> AppResult<()> {
    start_translation(&app, Direction::Incoming).await
}

/// Starts `direction` with the saved settings (see [`start_outgoing`] and [`start_incoming`]);
/// also used by the tray menu.
pub async fn start_translation(app: &AppHandle, direction: Direction) -> AppResult<()> {
    let meter = app.state::<Arc<UsageMeter>>();
    meter.check_start()?;
    let names = match direction {
        Direction::Incoming => INCOMING_EVENTS,
        Direction::Outgoing => OUTGOING_EVENTS,
    };
    let events = metered_events(app.clone(), meter.inner(), direction, names);
    let store = app.state::<SettingsStore>();
    let credentials = app.state::<Credentials>();
    let journal = app.state::<DuckingJournal>().0.clone();
    let prepare = async {
        let settings = store.get()?;
        let translator = translator(&credentials)?;
        let devices = list_audio_devices().await?;
        let plan = match direction {
            Direction::Outgoing => {
                if !outgoing::check_devices(&settings.outgoing, &devices)? {
                    tracing::warn!("the outgoing output is not a known virtual cable");
                }
                StartPlan {
                    config: outgoing::config(
                        &settings.outgoing,
                        &settings.voice_activity,
                        app.state::<Hotkeys>().talk_key(),
                    ),
                    translator,
                    io: Arc::new(WasapiIo::default()),
                }
            }
            Direction::Incoming => {
                incoming::check_devices(&settings.incoming, &devices)?;
                StartPlan {
                    config: incoming::config(&settings.incoming, &settings.voice_activity),
                    translator,
                    io: Arc::new(WasapiIo::with_ducking_journal(journal)),
                }
            }
        };
        Ok(plan)
    };
    app.state::<PipelineManager>()
        .start(direction, events, prepare)
        .await
}

/// Stops the incoming translation, if it is running, and waits until it has stopped (and the
/// original audio is back to its volume).
#[tauri::command]
pub async fn stop_incoming(pipelines: State<'_, PipelineManager>) -> AppResult<()> {
    pipelines.stop(Direction::Incoming).await;
    Ok(())
}

/// The incoming pipeline's latest status (for a panel that opens while it runs).
#[tauri::command]
pub fn get_incoming_status(pipelines: State<'_, PipelineManager>) -> PipelineStatus {
    pipelines.status(Direction::Incoming)
}

/// The tray menu's strings, in the panel's language. The panel sends them at startup and when
/// the app language changes.
#[tauri::command]
pub fn set_tray_labels(app: AppHandle, labels: TrayLabels) {
    tray::set_labels(&app, labels);
}

/// Hides the window to the tray (the translations keep running), remembering that the one-off
/// tray notice was shown.
#[tauri::command]
pub fn hide_to_tray(app: AppHandle, store: State<'_, SettingsStore>) -> AppResult<()> {
    store.update(|settings| settings.tray_notice_shown = true)?;
    tray::hide_window(&app);
    Ok(())
}

/// Remembers that the first-run setup was finished or skipped, so it is not shown again.
/// Returns the saved settings.
#[tauri::command]
pub fn complete_onboarding(store: State<'_, SettingsStore>) -> AppResult<Settings> {
    store.update(|settings| settings.onboarding_completed = true)
}

/// The keyboard shortcuts that are set but not working (Windows refused them, or they type a
/// character on the keyboard layout), and whether they are suspended.
#[tauri::command]
pub fn get_hotkey_status(hotkeys: State<'_, Hotkeys>) -> HotkeyStatus {
    hotkeys.status()
}

/// Turns every keyboard shortcut off while the panel records a new one (so the combination
/// reaches the panel instead of firing), and back on.
#[tauri::command]
pub fn set_hotkeys_suspended(app: AppHandle, hotkeys: State<'_, Hotkeys>, suspended: bool) {
    hotkeys.set_suspended(&PluginRegistrar(&app), suspended);
}

/// Checks a shortcut the panel recorded: fails with `invalid_settings` when it is not a valid
/// shortcut, or `hotkey_types_character` when it types a character on the keyboard layout.
#[tauri::command]
pub fn check_hotkey(shortcut: String) -> AppResult<()> {
    let hotkey = hotkeys::Hotkey::parse(&shortcut)
        .map_err(|error| AppError::InvalidSettings(error.to_string()))?;
    if hotkeys::layout::types_character(&hotkey) {
        return Err(AppError::HotkeyTypesCharacter(hotkey.to_string()));
    }
    Ok(())
}

/// Quits LiveDub Voice: both translations stop (gracefully, see [`shut_down_pipelines`]) and
/// the app exits.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    tracing::info!("quit from the panel");
    app.exit(0);
}

/// Longest the app waits, when it exits, for both translations to close their sessions,
/// release the devices and restore the ducked apps.
pub const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Stops both translations for exit (see [`PipelineManager::shutdown`]), then makes sure no app
/// is left ducked even if that ran out of time. Blocking: call it from outside the async
/// runtime (the main thread's exit handler).
pub fn shut_down_pipelines(app: &AppHandle) {
    let pipelines = app.state::<PipelineManager>();
    let pipelines: &PipelineManager = pipelines.inner();
    let journal = &app.state::<DuckingJournal>().0;
    // On a thread of its own: `block_on` must never run inside the async runtime.
    let in_time = std::thread::scope(|scope| {
        scope
            .spawn(|| tauri::async_runtime::block_on(pipelines.shutdown(EXIT_TIMEOUT)))
            .join()
            .unwrap_or(false)
    });
    if !in_time {
        pipelines.release_ducking();
    }
    // Never leave the captured apps ducked (also covers a ducker that could not finish).
    if let Err(error) = audio::ducking::recover(journal) {
        tracing::warn!(%error, "could not restore the ducked apps on exit");
    }
    if let Err(error) = app.state::<Arc<UsageMeter>>().flush() {
        tracing::warn!(%error, "could not write the usage history on exit");
    }
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

/// Meters a direction's events (see [`crate::usage`]) and forwards them to the panel.
fn metered_events(
    app: AppHandle,
    meter: &Arc<UsageMeter>,
    direction: Direction,
    names: EventNames,
) -> EventSink {
    let notify = usage_notify(app.clone(), Arc::clone(meter));
    metered_sink(
        Arc::clone(meter),
        direction,
        event_sink(app, direction, names),
        notify,
    )
}

/// Forwards a pipeline's events to the panel (and status changes to the tray). On `NoSpeech`
/// (auto-off) it stops the direction through the manager and tells the panel why.
fn event_sink(app: AppHandle, direction: Direction, names: EventNames) -> EventSink {
    Arc::new(move |event| {
        let result = match event {
            PipelineEvent::Status(status) => {
                tray::refresh(&app);
                app.emit(names.status, status)
            }
            PipelineEvent::Level(level) => app.emit(names.level, level),
            PipelineEvent::TranscriptDelta(delta) => {
                app.emit(names.transcript, TranscriptDelta { delta })
            }
            PipelineEvent::AudioSent { seconds } => {
                app.emit(names.audio_sent, AudioSent { seconds })
            }
            PipelineEvent::Voice(activity) => app.emit::<VoiceActivity>(names.voice, activity),
            PipelineEvent::NoSpeech { after } => {
                tracing::info!(
                    direction = direction.name(),
                    minutes = after.as_secs() / 60,
                    "auto-off: stopping after no speech"
                );
                let stopping = app.clone();
                tauri::async_runtime::spawn(async move {
                    stopping.state::<PipelineManager>().stop(direction).await;
                });
                app.emit(
                    names.auto_off,
                    AutoOff {
                        minutes: after.as_secs() / 60,
                    },
                )
            }
        };
        if let Err(error) = result {
            tracing::warn!(%error, event = names.status, "could not emit a pipeline event");
        }
    })
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
