import { invoke } from "@tauri-apps/api/core";

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

/** Language supported by the translator. */
export type Language = "es" | "en";

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
