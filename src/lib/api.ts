import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Error returned by a failing Tauri command.
 *
 * Mirrors the serialization of `AppError` in `src-tauri/src/error.rs`.
 */
export interface AppError {
  /** Stable, machine-readable error code (for example `"io"` or `"internal"`). */
  code: string;
  /** Human-readable description of the error. */
  message: string;
}

/** Basic information about the running app, as returned by the `app_info` command. */
export interface AppInfo {
  name: string;
  version: string;
  profile: "debug" | "release";
  /** Whether `LIVEDUB_FAKE_TRANSLATOR` replaces OpenAI (no API key needed). */
  fakeTranslator: boolean;
}

/** Returns true when `value` has the shape of an {@link AppError}. */
export function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).code === "string" &&
    typeof (value as Record<string, unknown>).message === "string"
  );
}

/** Fetches the app name, version and build profile from the core. */
export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

/** Panel language setting: follow the OS, or a fixed language. */
export type UiLanguage = "system" | "en" | "es";

/**
 * Target language of the translator (its API code). The full list, in display order, is
 * `LANGUAGES` in `lib/languages.ts`.
 */
export type Language =
  | "es"
  | "en"
  | "pt"
  | "fr"
  | "ja"
  | "ru"
  | "zh"
  | "de"
  | "ko"
  | "hi"
  | "id"
  | "vi"
  | "it";

/** Audio captured by the incoming direction. */
export type SourceApp =
  /** Every app on the system except LiveDub Voice itself. */
  | { kind: "allExceptSelf" }
  /** A specific process (and its children), by executable name, e.g. `Discord.exe`. */
  | { kind: "process"; name: string };

/** Incoming direction: other participants → the user's headphones. */
export interface IncomingSettings {
  enabled: boolean;
  /** Language the user hears. */
  targetLanguage: Language;
  sourceApp: SourceApp;
  /** Playback device for the translation; `null` means the system default. */
  outputDeviceId: string | null;
  /** Volume of the original audio mixed under the translation, 0–100. */
  originalVolume: number;
}

/**
 * What the outgoing direction sends while it runs: what voice detection hears, or only while the
 * push-to-talk key is held.
 */
export type TalkMode = "voiceDetection" | "pushToTalk";

/** Outgoing direction: the user's microphone → a virtual cable used as mic by the call/game app. */
export interface OutgoingSettings {
  enabled: boolean;
  /** Language the other participants hear. */
  targetLanguage: Language;
  /** Microphone to capture; `null` means the system default. */
  inputDeviceId: string | null;
  /** Playback device to write the translation to (normally a virtual cable). */
  outputDeviceId: string | null;
  /** Voice detection, or push-to-talk (with the `pushToTalk` shortcut). */
  talkMode: TalkMode;
}

/** In-app spending cap, in US dollars; `null` = no cap for that period. Off by default. */
export interface SpendingCapSettings {
  /** Most to spend per local calendar day. */
  dailyUsd: number | null;
  /** Most to spend per local calendar month. */
  monthlyUsd: number | null;
}

/** Local voice detection, for both directions. */
export interface VoiceActivitySettings {
  /** Stop sending audio to OpenAI while nobody speaks. On by default. */
  pauseWhenSilent: boolean;
  /** Stop a direction after this many minutes without speech (1–240); `null` = never. */
  autoOffMinutes: number | null;
}

/**
 * Something a global keyboard shortcut does. Mirrors `HotkeyAction` in
 * `src-tauri/src/hotkeys/mod.rs`.
 */
export type HotkeyAction =
  "toggleIncoming" | "toggleOutgoing" | "toggleBoth" | "pushToTalk";

/** Global keyboard shortcuts. */
export interface HotkeySettings {
  /**
   * Each action's shortcut (`"Ctrl+Alt+KeyI"`: modifiers, then a `KeyboardEvent.code`); `null` =
   * none. A missing action uses its default.
   */
  bindings: Partial<Record<HotkeyAction, string | null>>;
  /** A short, quiet sound in the headphones when a shortcut turns a direction on or off. */
  soundCues: boolean;
}

/**
 * User settings, as stored in `settings.json`.
 *
 * Mirrors `Settings` in `src-tauri/src/settings/mod.rs`.
 */
export interface Settings {
  /** File format version, for future migrations. */
  version: number;
  /** Language of the panel itself; `"system"` follows the Windows display language. */
  uiLanguage: UiLanguage;
  incoming: IncomingSettings;
  outgoing: OutgoingSettings;
  spendingCap: SpendingCapSettings;
  voiceActivity: VoiceActivitySettings;
  hotkeys: HotkeySettings;
  /** Closing the window quits the app (off = it hides to the tray and keeps translating). */
  closeQuitsApp: boolean;
  /** The one-off tray notice was shown. Owned by the core: saving never turns it back off. */
  trayNoticeShown: boolean;
  /**
   * The first-run setup was finished or skipped. Owned by the core: saving never changes it;
   * {@link completeOnboarding} sets it.
   */
  onboardingCompleted: boolean;
}

/** Fetches the current settings from the core. */
export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/**
 * Validates and saves the settings, resolving with what was saved. The keyboard shortcuts are
 * registered again at once.
 *
 * Rejects with an {@link AppError}: `"invalid_settings"` when the settings are not valid,
 * `"hotkey_unavailable"` when Windows refuses a changed shortcut, or `"hotkey_types_character"`
 * when a changed shortcut types a character on the keyboard layout (nothing is saved then).
 */
export function saveSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("save_settings", { settings });
}

/**
 * What the panel may know about the stored OpenAI API key. The key itself never comes back.
 *
 * Mirrors `ApiKeyStatus` in `src-tauri/src/credentials/mod.rs`.
 */
export interface ApiKeyStatus {
  hasKey: boolean;
  /** Last four characters of the stored key; `null` when no key is set. */
  last4: string | null;
}

/**
 * Result of checking a key against OpenAI.
 *
 * Mirrors `KeyValidation` in `src-tauri/src/credentials/validation.rs`.
 */
export type KeyValidation =
  /** The key works and can use the model. */
  | { kind: "valid" }
  /** OpenAI rejected the key (HTTP 401). */
  | { kind: "invalidKey" }
  /** The key is valid but the account cannot use the model (HTTP 403/404). */
  | { kind: "noModelAccess" }
  /** Too many requests (HTTP 429). */
  | { kind: "rateLimited" }
  /** OpenAI could not be reached (no network or timeout). */
  | { kind: "offline" }
  /** Any other answer; `status` is the HTTP status when a response arrived. */
  | { kind: "unexpected"; status: number | null };

/** Result of {@link saveApiKey}: the key is stored even when the check failed. */
export interface SaveApiKeyResult {
  status: ApiKeyStatus;
  validation: KeyValidation;
}

/** Whether an API key is stored, and its last four characters. */
export function getApiKeyStatus(): Promise<ApiKeyStatus> {
  return invoke<ApiKeyStatus>("get_api_key_status");
}

/**
 * Stores the key in Windows Credential Manager and checks it against OpenAI.
 *
 * Rejects with an {@link AppError} (code `"invalid_api_key"`) when the key does not look like an
 * OpenAI key, or `"credential_store"` when it cannot be stored.
 */
export function saveApiKey(key: string): Promise<SaveApiKeyResult> {
  return invoke<SaveApiKeyResult>("save_api_key", { key });
}

/** Checks the stored key against OpenAI again. Rejects with code `"no_api_key"` if none is set. */
export function validateApiKey(): Promise<KeyValidation> {
  return invoke<KeyValidation>("validate_api_key");
}

/** Deletes the stored key, resolving with the new (empty) status. */
export function deleteApiKey(): Promise<ApiKeyStatus> {
  return invoke<ApiKeyStatus>("delete_api_key");
}

/**
 * An active audio endpoint.
 *
 * Mirrors `AudioDevice` in `src-tauri/src/audio/devices.rs`.
 */
export interface AudioDevice {
  /** Stable endpoint id; this is what settings store. */
  id: string;
  /** Friendly name, e.g. "Speakers (Realtek(R) Audio)". */
  name: string;
  /** `input` records (microphones, cable outputs); `output` plays (speakers, cable inputs). */
  kind: "input" | "output";
  /** Whether this is the system default device of its kind. */
  isDefault: boolean;
  /** Whether the name matches a known virtual audio cable (VB-CABLE, VoiceMeeter, …). */
  isVirtualCable: boolean;
}

/**
 * Lists the active audio input and output devices: inputs first, then outputs; the default
 * device of each kind first, then by name.
 *
 * Rejects with an {@link AppError} (code `"audio"`) when Windows cannot list them.
 */
export function listAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_audio_devices");
}

/**
 * An app that has audio sessions: a possible source for the incoming translation.
 *
 * Mirrors `AudioApp` in `src-tauri/src/audio/apps.rs`.
 */
export interface AudioApp {
  /** Executable of the app's root process, e.g. `Discord.exe`; what `SourceApp` stores. */
  processName: string;
  /** Name to show, e.g. "Google Chrome". */
  displayName: string;
  /** Root of the app's process tree (what gets captured). */
  pid: number;
  /** Processes of the app that own audio sessions (often its child processes). */
  sessionPids: number[];
  /** Whether the app is playing audio right now. */
  active: boolean;
}

/**
 * Lists the apps playing audio (or that have recently), grouped by app with their helper
 * processes: playing ones first, then by name. LiveDub Voice itself is never listed.
 *
 * Rejects with an {@link AppError} (code `"audio"`) when Windows cannot list them.
 */
export function listAudioApps(): Promise<AudioApp[]> {
  return invoke<AudioApp[]>("list_audio_apps");
}

/**
 * Input level of the microphone test: linear amplitude, 0 (silence) to 1 (full scale).
 *
 * Mirrors `InputLevel` in `src-tauri/src/audio/level.rs`.
 */
export interface InputLevel {
  rms: number;
  peak: number;
}

/**
 * Starts the microphone test on `inputId` (`null` = system default), replacing a running one.
 *
 * With `listen`, also plays the microphone on `outputId` (`null` = default). Rejects with an
 * {@link AppError} (code `"audio"`) when a device cannot be opened, or when the two devices use
 * different sample rates or channel counts ("formats differ, needs resampling").
 */
export function startMonitor(
  inputId: string | null,
  outputId: string | null,
  listen: boolean,
): Promise<void> {
  return invoke<void>("start_monitor", { inputId, outputId, listen });
}

/** Stops the microphone test, if one is running. */
export function stopMonitor(): Promise<void> {
  return invoke<void>("stop_monitor");
}

/** Calls `handler` with the microphone test's level, ~20 times per second. */
export function onMonitorLevel(
  handler: (level: InputLevel) => void,
): Promise<UnlistenFn> {
  return listen<InputLevel>("monitor-level", (event) => handler(event.payload));
}

/** Calls `handler` when the microphone test stops on its own (for example a device was unplugged). */
export function onMonitorStopped(
  handler: (message: string) => void,
): Promise<UnlistenFn> {
  return listen<{ message: string }>("monitor-stopped", (event) =>
    handler(event.payload.message),
  );
}

/** What a translation pipeline is doing. Mirrors `PipelineState` in `src-tauri/src/pipeline/mod.rs`. */
export type PipelineState =
  "idle" | "connecting" | "live" | "reconnecting" | "stopping" | "error";

/** Which reconnect attempt runs. Mirrors `ReconnectProgress` in `src-tauri/src/pipeline/mod.rs`. */
export interface ReconnectProgress {
  /** 1-based. */
  attempt: number;
  maxAttempts: number;
}

/**
 * A pipeline's state, the error that stopped it (only with `"error"`) and the reconnect attempt
 * (only with `"reconnecting"`).
 */
export interface PipelineStatus {
  state: PipelineState;
  error: AppError | null;
  reconnect?: ReconnectProgress | null;
}

/**
 * The voice indicator of a running direction. Mirrors `VoiceActivity` in
 * `src-tauri/src/pipeline/voice.rs`.
 */
export interface VoiceActivity {
  /** Someone is speaking. */
  speaking: boolean;
  /** Audio is being sent to OpenAI (speech and the second after it, or everything when pausing is off). */
  sending: boolean;
}

/**
 * Starts translating the microphone into the virtual cable with the saved `outgoing` settings.
 * Does nothing if it already runs.
 *
 * Rejects with an {@link AppError}: `"spending_cap_reached"` (the cap is reached; nothing
 * starts), `"no_api_key"`, `"no_output_device"`, `"device_missing"`,
 * `"audio"`, or a translator code such as `"openai_auth"` or `"network"`.
 */
export function startOutgoing(): Promise<void> {
  return invoke<void>("start_outgoing");
}

/** Stops the outgoing translation (if it runs), resolving once it has stopped. */
export function stopOutgoing(): Promise<void> {
  return invoke<void>("stop_outgoing");
}

/** The outgoing translation's latest status. */
export function getOutgoingStatus(): Promise<PipelineStatus> {
  return invoke<PipelineStatus>("get_outgoing_status");
}

/** Calls `handler` each time the outgoing translation's status changes. */
export function onOutgoingStatus(
  handler: (status: PipelineStatus) => void,
): Promise<UnlistenFn> {
  return listen<PipelineStatus>("outgoing-status", (event) =>
    handler(event.payload),
  );
}

/** Calls `handler` with the outgoing microphone level, ~20 times per second while live. */
export function onOutgoingLevel(
  handler: (level: InputLevel) => void,
): Promise<UnlistenFn> {
  return listen<InputLevel>("outgoing-level", (event) =>
    handler(event.payload),
  );
}

/** Calls `handler` with each new piece of translated text (what the others hear). */
export function onOutgoingTranscript(
  handler: (delta: string) => void,
): Promise<UnlistenFn> {
  return listen<{ delta: string }>("outgoing-transcript", (event) =>
    handler(event.payload.delta),
  );
}

/** Calls `handler` when the outgoing voice indicator changes. */
export function onOutgoingVoice(
  handler: (activity: VoiceActivity) => void,
): Promise<UnlistenFn> {
  return listen<VoiceActivity>("outgoing-voice", (event) =>
    handler(event.payload),
  );
}

/** Calls `handler` when the outgoing translation turns itself off after no speech. */
export function onOutgoingAutoOff(
  handler: (minutes: number) => void,
): Promise<UnlistenFn> {
  return listen<{ minutes: number }>("outgoing-auto-off", (event) =>
    handler(event.payload.minutes),
  );
}

/** Calls `handler` with the seconds of microphone audio sent since start, every second. */
export function onOutgoingAudioSent(
  handler: (seconds: number) => void,
): Promise<UnlistenFn> {
  return listen<{ seconds: number }>("outgoing-audio-sent", (event) =>
    handler(event.payload.seconds),
  );
}

/**
 * Starts translating the source app (or the whole system minus LiveDub Voice) into the
 * headphones with the saved `incoming` settings, ducking the original audio while it runs. Does
 * nothing if it already runs.
 *
 * Rejects with an {@link AppError}: `"spending_cap_reached"` (the cap is reached; nothing
 * starts), `"no_api_key"`, `"device_missing"`,
 * `"source_app_not_running"`, `"audio"`, or a translator code such as `"openai_auth"`.
 */
export function startIncoming(): Promise<void> {
  return invoke<void>("start_incoming");
}

/** Stops the incoming translation (if it runs), resolving once it has stopped. */
export function stopIncoming(): Promise<void> {
  return invoke<void>("stop_incoming");
}

/** The incoming translation's latest status. */
export function getIncomingStatus(): Promise<PipelineStatus> {
  return invoke<PipelineStatus>("get_incoming_status");
}

/** Calls `handler` each time the incoming translation's status changes. */
export function onIncomingStatus(
  handler: (status: PipelineStatus) => void,
): Promise<UnlistenFn> {
  return listen<PipelineStatus>("incoming-status", (event) =>
    handler(event.payload),
  );
}

/**
 * Calls `handler` with the level of the captured audio (as the translator hears it, before
 * ducking), ~20 times per second while live.
 */
export function onIncomingLevel(
  handler: (level: InputLevel) => void,
): Promise<UnlistenFn> {
  return listen<InputLevel>("incoming-level", (event) =>
    handler(event.payload),
  );
}

/** Calls `handler` with each new piece of translated text (what the user hears). */
export function onIncomingTranscript(
  handler: (delta: string) => void,
): Promise<UnlistenFn> {
  return listen<{ delta: string }>("incoming-transcript", (event) =>
    handler(event.payload.delta),
  );
}

/** Calls `handler` when the incoming voice indicator changes. */
export function onIncomingVoice(
  handler: (activity: VoiceActivity) => void,
): Promise<UnlistenFn> {
  return listen<VoiceActivity>("incoming-voice", (event) =>
    handler(event.payload),
  );
}

/** Calls `handler` when the incoming translation turns itself off after no speech. */
export function onIncomingAutoOff(
  handler: (minutes: number) => void,
): Promise<UnlistenFn> {
  return listen<{ minutes: number }>("incoming-auto-off", (event) =>
    handler(event.payload.minutes),
  );
}

/** Calls `handler` with the seconds of app audio sent since start, every second. */
export function onIncomingAudioSent(
  handler: (seconds: number) => void,
): Promise<UnlistenFn> {
  return listen<{ seconds: number }>("incoming-audio-sent", (event) =>
    handler(event.payload.seconds),
  );
}

/** Which spending cap a {@link CapStatus} is about. */
export type CapPeriod = "daily" | "monthly";

/**
 * How close the estimated spend is to the spending cap: `off` (no cap), `ok`, `warning` (80 % or
 * more) or `reached` (100 % or more).
 */
export type CapLevel = "off" | "ok" | "warning" | "reached";

/**
 * The spending cap closest to being reached. Mirrors `CapStatus` in
 * `src-tauri/src/usage/mod.rs`.
 */
export interface CapStatus {
  dailyUsd: number | null;
  monthlyUsd: number | null;
  level: CapLevel;
  /** The cap `level` is about; `null` when no cap is set. */
  period: CapPeriod | null;
  /** Estimated spend of that period (today or this month). */
  spentUsd: number;
  limitUsd: number | null;
  /** The user chose to keep translating over the cap for the rest of today. */
  overriddenToday: boolean;
}

/** One direction's current (or last) session. */
export interface SessionUsage {
  active: boolean;
  seconds: number;
  costUsd: number;
}

/** Usage over a period (today, this month). */
export interface PeriodUsage {
  incomingSeconds: number;
  outgoingSeconds: number;
  costUsd: number;
}

/** One day of the usage history. */
export interface DayUsage {
  /** Local calendar day, `YYYY-MM-DD`. */
  date: string;
  incomingSeconds: number;
  outgoingSeconds: number;
  costUsd: number;
}

/**
 * Usage metered on this computer (an estimate, not the OpenAI bill). Mirrors `UsageSnapshot` in
 * `src-tauri/src/usage/mod.rs`.
 */
export interface UsageSnapshot {
  pricePerMinuteUsd: number;
  session: { incoming: SessionUsage; outgoing: SessionUsage };
  today: PeriodUsage;
  month: PeriodUsage;
  /** Days with usage, newest first. */
  days: DayUsage[];
  cap: CapStatus;
}

/** The usage of the current sessions, today, this month and per day, and the cap status. */
export function getUsage(): Promise<UsageSnapshot> {
  return invoke<UsageSnapshot>("get_usage");
}

/** Keeps translating over the spending cap for the rest of today, resolving with the new usage. */
export function overrideSpendingCap(): Promise<UsageSnapshot> {
  return invoke<UsageSnapshot>("override_spending_cap");
}

/** Calls `handler` whenever the usage changes (about once a second while translating). */
export function onUsageUpdated(
  handler: (usage: UsageSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<UsageSnapshot>("usage-updated", (event) =>
    handler(event.payload),
  );
}

/** Calls `handler` when the spending cap is reached and the translations are being stopped. */
export function onSpendingCapReached(
  handler: (status: CapStatus) => void,
): Promise<UnlistenFn> {
  return listen<CapStatus>("spending-cap-reached", (event) =>
    handler(event.payload),
  );
}

/**
 * The tray menu's strings, in the panel's language. Mirrors `TrayLabels` in
 * `src-tauri/src/tray.rs`.
 */
export interface TrayLabels {
  open: string;
  quit: string;
  incoming: string;
  outgoing: string;
  states: Record<PipelineState, string>;
}

/** Sends the tray menu's strings to the core (at startup and when the panel language changes). */
export function setTrayLabels(labels: TrayLabels): Promise<void> {
  return invoke<void>("set_tray_labels", { labels });
}

/** Hides the window to the tray (translations keep running) and remembers the notice was shown. */
export function hideToTray(): Promise<void> {
  return invoke<void>("hide_to_tray");
}

/** Remembers that the first-run setup was finished or skipped, resolving with the saved settings. */
export function completeOnboarding(): Promise<Settings> {
  return invoke<Settings>("complete_onboarding");
}

/** Quits LiveDub Voice: both translations stop gracefully and the app exits. */
export function quitApp(): Promise<void> {
  return invoke<void>("quit_app");
}

/** Calls `handler` the first time the window is closed, to explain that the app keeps running in the tray. */
export function onCloseToTrayNotice(handler: () => void): Promise<UnlistenFn> {
  return listen<null>("close-to-tray-notice", () => handler());
}

/** Why a shortcut that is set does not work. Mirrors `ProblemReason` in `src-tauri/src/hotkeys/mod.rs`. */
export type HotkeyProblemReason = "unavailable" | "typesCharacter";

/** A shortcut that is set but not registered. */
export interface HotkeyProblem {
  action: HotkeyAction;
  shortcut: string;
  reason: HotkeyProblemReason;
}

/** The shortcuts that do not work, and whether they are paused while one is being recorded. */
export interface HotkeyStatus {
  problems: HotkeyProblem[];
  suspended: boolean;
}

/** Which shortcuts are set but not working (Windows refused them, or they type a character). */
export function getHotkeyStatus(): Promise<HotkeyStatus> {
  return invoke<HotkeyStatus>("get_hotkey_status");
}

/**
 * Turns every shortcut off while a new one is being recorded (so the combination reaches the
 * panel instead of firing), and back on.
 */
export function setHotkeysSuspended(suspended: boolean): Promise<void> {
  return invoke<void>("set_hotkeys_suspended", { suspended });
}

/**
 * Checks a recorded shortcut with the core. Rejects with `"hotkey_types_character"` when it types a
 * character on the keyboard layout (AltGr), or `"invalid_settings"` when it is not valid.
 */
export function checkHotkey(shortcut: string): Promise<void> {
  return invoke<void>("check_hotkey", { shortcut });
}

/** Calls `handler` when the push-to-talk key goes down (`true`) or up (`false`). */
export function onPushToTalk(
  handler: (held: boolean) => void,
): Promise<UnlistenFn> {
  return listen<{ held: boolean }>("push-to-talk", (event) =>
    handler(event.payload.held),
  );
}
