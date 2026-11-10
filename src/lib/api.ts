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

/** Outgoing direction: the user's microphone → a virtual cable used as mic by the call/game app. */
export interface OutgoingSettings {
  enabled: boolean;
  /** Language the other participants hear. */
  targetLanguage: Language;
  /** Microphone to capture; `null` means the system default. */
  inputDeviceId: string | null;
  /** Playback device to write the translation to (normally a virtual cable). */
  outputDeviceId: string | null;
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
}

/** Fetches the current settings from the core. */
export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/**
 * Validates and saves the settings, resolving with what was saved.
 *
 * Rejects with an {@link AppError} (code `"invalid_settings"`) when the settings are not valid.
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
export type PipelineState = "idle" | "connecting" | "live" | "error";

/** A pipeline's state, and the error that stopped it (only with `"error"`). */
export interface PipelineStatus {
  state: PipelineState;
  error: AppError | null;
}

/**
 * Starts translating the microphone into the virtual cable with the saved `outgoing` settings.
 * Does nothing if it already runs.
 *
 * Rejects with an {@link AppError}: `"no_api_key"`, `"no_output_device"`, `"device_missing"`,
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

/** Calls `handler` with the seconds of microphone audio sent since start, every second. */
export function onOutgoingAudioSent(
  handler: (seconds: number) => void,
): Promise<UnlistenFn> {
  return listen<{ seconds: number }>("outgoing-audio-sent", (event) =>
    handler(event.payload.seconds),
  );
}
