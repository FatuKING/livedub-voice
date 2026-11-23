import { Keyboard, X } from "lucide-react";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useT } from "../i18n/context";
import { checkHotkey, isAppError, setHotkeysSuspended } from "../lib/api";
import { formatShortcut, recordKeyPress } from "../lib/hotkeys";
import type { Direction } from "../lib/languages";
import "./ShortcutField.css";

export interface ShortcutFieldProps {
  label: ReactNode;
  hint?: ReactNode;
  /** The shortcut (`"Ctrl+Alt+KeyI"`), or `null` for none. */
  value: string | null;
  onChange: (value: string | null) => void;
  /**
   * Checks a recorded shortcut against the rest of the form (for example: another action uses
   * it). Returns the message to show, or `null` when it is fine.
   */
  validate?: (value: string) => string | null;
  /** A problem with the current shortcut, shown under the field (for example: Windows refused it). */
  problem?: string | null;
  /** Color of the recording state. Neutral when omitted. */
  direction?: Direction;
}

/**
 * Records a global keyboard shortcut: click the field and press the combination. Escape cancels;
 * the clear button removes it.
 *
 * While recording, the core turns every shortcut off (`setHotkeysSuspended`), so a combination
 * that is already registered reaches the panel instead of firing. A key that types needs Ctrl,
 * Alt or Win, and the core checks that the combination does not type a character on the
 * keyboard layout (AltGr).
 */
function ShortcutField({
  label,
  hint,
  value,
  onChange,
  validate,
  problem = null,
  direction,
}: ShortcutFieldProps) {
  const t = useT();
  const id = useId();
  const hintId = `${id}-hint`;
  const messageId = `${id}-message`;
  const [recording, setRecording] = useState(false);
  const [pressed, setPressed] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const recordingRef = useRef(false);

  // Never leave the shortcuts off when the field goes away mid-recording.
  useEffect(
    () => () => {
      if (recordingRef.current)
        setHotkeysSuspended(false).catch(() => undefined);
    },
    [],
  );

  function start() {
    if (recordingRef.current || checking) return;
    recordingRef.current = true;
    setRecording(true);
    setPressed("");
    setError(null);
    setHotkeysSuspended(true).catch(() => undefined);
  }

  function stop() {
    if (!recordingRef.current) return;
    recordingRef.current = false;
    setRecording(false);
    setPressed("");
    setHotkeysSuspended(false).catch(() => undefined);
  }

  async function accept(shortcut: string) {
    const formatted = formatShortcut(shortcut);
    const conflict = validate?.(shortcut) ?? null;
    if (conflict) {
      setError(conflict);
      return;
    }
    setChecking(true);
    try {
      await checkHotkey(shortcut);
      onChange(shortcut);
    } catch (reason: unknown) {
      if (isAppError(reason) && reason.code === "hotkey_types_character") {
        setError(
          t("settings.hotkeys.problems.typesCharacter", {
            shortcut: formatted,
          }),
        );
      } else {
        setError(
          t("settings.hotkeys.problems.unsupportedKey", {
            shortcut: formatted,
          }),
        );
      }
    } finally {
      setChecking(false);
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (!recordingRef.current) return;
    const bare =
      !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (bare && event.key === "Tab") {
      // Leave the field with the keyboard as usual.
      stop();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (bare && event.key === "Escape") {
      stop();
      return;
    }
    const recorded = recordKeyPress(event);
    if (recorded.kind === "modifiers") {
      setPressed(recorded.text);
      return;
    }
    stop();
    if (recorded.kind === "invalid") {
      setError(
        t(`settings.hotkeys.problems.${recorded.problem}`, {
          shortcut: formatShortcut(recorded.text),
        }),
      );
      return;
    }
    void accept(recorded.value);
  }

  const shown = recording
    ? pressed
      ? formatShortcut(pressed)
      : t("settings.hotkeys.recording")
    : value
      ? formatShortcut(value)
      : t("settings.hotkeys.none");
  const message = error ?? problem;
  const describedBy = [hint && hintId, message && messageId]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={[
        "ld-shortcut",
        direction && `ld-shortcut--${direction}`,
        recording && "ld-shortcut--recording",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <label className="ld-shortcut__label" htmlFor={id}>
        {label}
      </label>
      <div className="ld-shortcut__row">
        <button
          type="button"
          id={id}
          className={[
            "ld-shortcut__input",
            !value && !recording && "ld-shortcut__input--empty",
          ]
            .filter(Boolean)
            .join(" ")}
          aria-describedby={describedBy || undefined}
          aria-pressed={recording}
          disabled={checking}
          onClick={() => (recording ? stop() : start())}
          onKeyDown={handleKeyDown}
          onBlur={stop}
        >
          <Keyboard
            className="ld-shortcut__icon"
            size={16}
            aria-hidden="true"
          />
          <span className="ld-shortcut__value" aria-live="polite">
            {shown}
          </span>
          {!recording && (
            <span className="ld-shortcut__action">
              {t("settings.hotkeys.change")}
            </span>
          )}
        </button>
        <button
          type="button"
          className="ld-shortcut__clear"
          aria-label={t("settings.hotkeys.clear")}
          title={t("settings.hotkeys.clear")}
          disabled={value === null || recording}
          onClick={() => {
            setError(null);
            onChange(null);
          }}
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>
      {hint && (
        <p className="ld-shortcut__hint" id={hintId}>
          {hint}
        </p>
      )}
      {message && (
        <p className="ld-shortcut__message" id={messageId} role="alert">
          {message}
        </p>
      )}
    </div>
  );
}

export default ShortcutField;
