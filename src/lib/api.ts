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
