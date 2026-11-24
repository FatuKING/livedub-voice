import type { HotkeyAction, HotkeySettings } from "./api";

/**
 * Global keyboard shortcuts, as the panel records and shows them.
 *
 * A shortcut is stored as `"Ctrl+Alt+KeyI"`: modifiers (`Ctrl`, `Alt`, `Shift`, `Win`, in that
 * order) then one key named like `KeyboardEvent.code`. These rules mirror
 * `src-tauri/src/hotkeys/shortcut.rs`, which has the last word when settings are saved.
 */

/** Every action, in the order Settings lists them. */
export const HOTKEY_ACTIONS: readonly HotkeyAction[] = [
  "toggleIncoming",
  "toggleOutgoing",
  "toggleBoth",
  "pushToTalk",
];

/** The shortcut each action gets by default (mirrors `HotkeyAction::default_binding`). */
export const DEFAULT_BINDINGS: Record<HotkeyAction, string | null> = {
  toggleIncoming: "Ctrl+Alt+KeyI",
  toggleOutgoing: "Ctrl+Alt+KeyO",
  toggleBoth: "Ctrl+Alt+KeyB",
  pushToTalk: "F8",
};

/** Keys that may be used without Ctrl, Alt or Win (they do not type anything). */
const FREE_KEYS = new Set([
  "Insert",
  "Pause",
  "ScrollLock",
  ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
]);

/** Keys that type or edit text: they need Ctrl, Alt or Win. */
const TYPING_KEYS = new Set([
  ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").map((letter) => `Key${letter}`),
  ...Array.from({ length: 10 }, (_, i) => `Digit${i}`),
  ...Array.from({ length: 10 }, (_, i) => `Numpad${i}`),
  "Semicolon",
  "Equal",
  "Comma",
  "Minus",
  "Period",
  "Slash",
  "Backquote",
  "BracketLeft",
  "Backslash",
  "BracketRight",
  "Quote",
  "Space",
  "Tab",
  "Enter",
  "Backspace",
  "Delete",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowLeft",
  "ArrowUp",
  "ArrowRight",
  "ArrowDown",
  "NumpadMultiply",
  "NumpadAdd",
  "NumpadSubtract",
  "NumpadDecimal",
  "NumpadDivide",
]);

/** `KeyboardEvent.code` of the modifier keys themselves. */
const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
]);

/** The part of a keyboard event a shortcut is made of. */
export interface KeyPress {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** What a key press means for a recorder. */
export type Recorded =
  /** Only modifiers so far: keep waiting (the text shows them). */
  | { kind: "modifiers"; text: string }
  /** A complete shortcut, valid for the core. */
  | { kind: "shortcut"; value: string }
  /** A complete combination that cannot be a shortcut. */
  | { kind: "invalid"; problem: ShortcutProblem; text: string };

/** Why a combination cannot be a shortcut. */
export type ShortcutProblem =
  /** Windows cannot register this key (Escape, Caps Lock, media keys, …). */
  | "unsupportedKey"
  /** A key that types needs Ctrl, Alt or Win. */
  | "needsModifier";

function modifierPrefix(press: KeyPress): string {
  return [
    press.ctrlKey && "Ctrl",
    press.altKey && "Alt",
    press.shiftKey && "Shift",
    press.metaKey && "Win",
  ]
    .filter(Boolean)
    .map((name) => `${name}+`)
    .join("");
}

/** Whether `code` is a key a shortcut can use. */
export function isSupportedKey(code: string): boolean {
  return FREE_KEYS.has(code) || TYPING_KEYS.has(code);
}

/** Turns a key press into a shortcut (see {@link Recorded}). */
export function recordKeyPress(press: KeyPress): Recorded {
  const prefix = modifierPrefix(press);
  if (MODIFIER_CODES.has(press.code)) {
    return { kind: "modifiers", text: prefix };
  }
  const value = `${prefix}${press.code}`;
  const problem = shortcutProblem(value);
  return problem
    ? { kind: "invalid", problem, text: value }
    : { kind: "shortcut", value };
}

/** Why `value` cannot be a shortcut, or `null` when it can. */
export function shortcutProblem(value: string): ShortcutProblem | null {
  const parts = value.split("+");
  const key = parts[parts.length - 1];
  if (!isSupportedKey(key)) return "unsupportedKey";
  const mods = parts.slice(0, -1);
  const blocksTyping = mods.some(
    (mod) => mod === "Ctrl" || mod === "Alt" || mod === "Win",
  );
  if (TYPING_KEYS.has(key) && !blocksTyping) return "needsModifier";
  return null;
}

/** `action`'s shortcut in `settings` (its default when not stored; `null` = none). */
export function bindingOf(
  settings: HotkeySettings,
  action: HotkeyAction,
): string | null {
  const stored = settings.bindings[action];
  return stored === undefined ? DEFAULT_BINDINGS[action] : stored;
}

/** The action other than `action` that already uses `value`, if any. */
export function conflictOf(
  settings: HotkeySettings,
  action: HotkeyAction,
  value: string,
): HotkeyAction | null {
  return (
    HOTKEY_ACTIONS.find(
      (other) => other !== action && bindingOf(settings, other) === value,
    ) ?? null
  );
}

/** How a key is shown: `KeyI` → `I`, `Digit1` → `1`, `Numpad1` → `Num 1`, `ArrowUp` → `↑`. */
function keyLabel(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  const named: Record<string, string> = {
    ArrowLeft: "←",
    ArrowUp: "↑",
    ArrowRight: "→",
    ArrowDown: "↓",
    Backquote: "`",
    Minus: "-",
    Equal: "=",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    Comma: ",",
    Period: ".",
    Slash: "/",
    ScrollLock: "Scroll Lock",
    PageUp: "Page Up",
    PageDown: "Page Down",
    NumpadMultiply: "Num *",
    NumpadAdd: "Num +",
    NumpadSubtract: "Num -",
    NumpadDecimal: "Num .",
    NumpadDivide: "Num /",
  };
  if (named[code]) return named[code];
  if (code.startsWith("Numpad")) return `Num ${code.slice(6)}`;
  return code;
}

/** A shortcut as the user reads it: `"Ctrl+Alt+KeyI"` → `"Ctrl + Alt + I"`. */
export function formatShortcut(value: string): string {
  const parts = value.split("+").filter(Boolean);
  const last = parts.length - 1;
  return parts
    .map((part, index) =>
      index === last && !value.endsWith("+") ? keyLabel(part) : part,
    )
    .join(" + ");
}

/** A copy of the default shortcut of every action. */
export function defaultBindings(): Record<HotkeyAction, string | null> {
  return { ...DEFAULT_BINDINGS };
}
