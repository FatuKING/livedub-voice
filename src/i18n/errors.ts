import { isAppError } from "../lib/api";
import { formatShortcut } from "../lib/hotkeys";
import { en } from "./en";
import { errorDetail, type Translate } from "./translate";
import type { MessageKey } from "./types";

type ErrorCode = keyof typeof en.errors;

function isKnownCode(code: string): code is ErrorCode {
  return Object.prototype.hasOwnProperty.call(en.errors, code);
}

/**
 * User-facing text for a failed core command.
 *
 * An `AppError` with a known `code` gets the translated message (with the core's detail, which
 * stays in English, in `{detail}`); an unknown code falls back to the core's message as is.
 */
export function translateError(reason: unknown, t: Translate): string {
  if (!isAppError(reason)) {
    return reason instanceof Error ? reason.message : String(reason);
  }
  if (!isKnownCode(reason.code)) return reason.message;
  const key: MessageKey = `errors.${reason.code}`;
  const detail = errorDetail(reason.message);
  // Shortcut errors hold the shortcut ("Ctrl+Alt+KeyI"): show it as the user reads it.
  return t(key, {
    detail: reason.code.startsWith("hotkey_") ? formatShortcut(detail) : detail,
  });
}

/** A failed action: the message to show (with an `{error}` placeholder) and why it failed. */
export interface Failure {
  key: MessageKey;
  reason: unknown;
}

/** Text for a {@link Failure}, translated at render time so it follows language changes. */
export function failureText(failure: Failure, t: Translate): string {
  return t(failure.key, { error: translateError(failure.reason, t) });
}
