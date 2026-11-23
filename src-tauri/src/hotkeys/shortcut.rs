//! Keyboard shortcuts as the settings store them: `"Ctrl+Alt+KeyI"`.
//!
//! A shortcut is zero or more modifiers (`Ctrl`, `Alt`, `Shift`, `Win`, in that order) and one
//! key, named like the browser's `KeyboardEvent.code` (`KeyI`, `Digit1`, `F8`, `Space`,
//! `Numpad0`, …): the physical key, so the panel's recorder writes exactly what it saw. Only keys
//! Windows can register as a hotkey (`RegisterHotKey`) are accepted; that rules out a modifier on
//! its own (just `Ctrl`) and mouse buttons.
//!
//! A key that types something (a letter, a digit, punctuation, Space, Enter, the arrows, …)
//! needs `Ctrl`, `Alt` or `Win`: a global hotkey swallows its key everywhere, so a bare `T` (or
//! `Shift+T`) would stop the user from typing it in every app. Function keys, `Pause`,
//! `ScrollLock` and `Insert` may be used alone.

use std::fmt;

use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

/// Modifier keys of a shortcut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Mods {
    /// Whether any modifier is held.
    pub fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }

    /// Whether the combination keeps a typing key from typing (Shift alone does not).
    fn blocks_typing(self) -> bool {
        self.ctrl || self.alt || self.win
    }
}

/// Whether a key may be used without a modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyClass {
    /// Types something or edits text: needs `Ctrl`, `Alt` or `Win`.
    Typing,
    /// Does nothing in most apps (function keys, Pause, Scroll Lock, Insert): may be used alone.
    Free,
}

/// A key a shortcut can use: its `KeyboardEvent.code` name, the plugin's code, its Windows
/// virtual-key code and whether it may be used alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInfo {
    pub name: &'static str,
    pub code: Code,
    pub vk: u16,
    class: KeyClass,
}

macro_rules! keys {
    ($($name:ident = $vk:expr, $class:ident;)*) => {
        /// Every key a shortcut can use.
        pub const KEYS: &[KeyInfo] = &[
            $(KeyInfo { name: stringify!($name), code: Code::$name, vk: $vk, class: KeyClass::$class },)*
        ];
    };
}

keys! {
    KeyA = 0x41, Typing; KeyB = 0x42, Typing; KeyC = 0x43, Typing; KeyD = 0x44, Typing;
    KeyE = 0x45, Typing; KeyF = 0x46, Typing; KeyG = 0x47, Typing; KeyH = 0x48, Typing;
    KeyI = 0x49, Typing; KeyJ = 0x4A, Typing; KeyK = 0x4B, Typing; KeyL = 0x4C, Typing;
    KeyM = 0x4D, Typing; KeyN = 0x4E, Typing; KeyO = 0x4F, Typing; KeyP = 0x50, Typing;
    KeyQ = 0x51, Typing; KeyR = 0x52, Typing; KeyS = 0x53, Typing; KeyT = 0x54, Typing;
    KeyU = 0x55, Typing; KeyV = 0x56, Typing; KeyW = 0x57, Typing; KeyX = 0x58, Typing;
    KeyY = 0x59, Typing; KeyZ = 0x5A, Typing;
    Digit0 = 0x30, Typing; Digit1 = 0x31, Typing; Digit2 = 0x32, Typing; Digit3 = 0x33, Typing;
    Digit4 = 0x34, Typing; Digit5 = 0x35, Typing; Digit6 = 0x36, Typing; Digit7 = 0x37, Typing;
    Digit8 = 0x38, Typing; Digit9 = 0x39, Typing;
    Semicolon = 0xBA, Typing; Equal = 0xBB, Typing; Comma = 0xBC, Typing; Minus = 0xBD, Typing;
    Period = 0xBE, Typing; Slash = 0xBF, Typing; Backquote = 0xC0, Typing;
    BracketLeft = 0xDB, Typing; Backslash = 0xDC, Typing; BracketRight = 0xDD, Typing;
    Quote = 0xDE, Typing;
    Space = 0x20, Typing; Tab = 0x09, Typing; Enter = 0x0D, Typing; Backspace = 0x08, Typing;
    Delete = 0x2E, Typing; Home = 0x24, Typing; End = 0x23, Typing; PageUp = 0x21, Typing;
    PageDown = 0x22, Typing; ArrowLeft = 0x25, Typing; ArrowUp = 0x26, Typing;
    ArrowRight = 0x27, Typing; ArrowDown = 0x28, Typing;
    Numpad0 = 0x60, Typing; Numpad1 = 0x61, Typing; Numpad2 = 0x62, Typing;
    Numpad3 = 0x63, Typing; Numpad4 = 0x64, Typing; Numpad5 = 0x65, Typing;
    Numpad6 = 0x66, Typing; Numpad7 = 0x67, Typing; Numpad8 = 0x68, Typing;
    Numpad9 = 0x69, Typing; NumpadMultiply = 0x6A, Typing; NumpadAdd = 0x6B, Typing;
    NumpadSubtract = 0x6D, Typing; NumpadDecimal = 0x6E, Typing; NumpadDivide = 0x6F, Typing;
    Insert = 0x2D, Free; Pause = 0x13, Free; ScrollLock = 0x91, Free;
    F1 = 0x70, Free; F2 = 0x71, Free; F3 = 0x72, Free; F4 = 0x73, Free; F5 = 0x74, Free;
    F6 = 0x75, Free; F7 = 0x76, Free; F8 = 0x77, Free; F9 = 0x78, Free; F10 = 0x79, Free;
    F11 = 0x7A, Free; F12 = 0x7B, Free; F13 = 0x7C, Free; F14 = 0x7D, Free; F15 = 0x7E, Free;
    F16 = 0x7F, Free; F17 = 0x80, Free; F18 = 0x81, Free; F19 = 0x82, Free; F20 = 0x83, Free;
    F21 = 0x84, Free; F22 = 0x85, Free; F23 = 0x86, Free; F24 = 0x87, Free;
}

/// A keyboard shortcut (see the [module docs](self)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: Mods,
    pub key: KeyInfo,
}

/// Why a shortcut string is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutError {
    #[error("empty shortcut")]
    Empty,
    #[error("unknown key or modifier \"{0}\"")]
    UnknownKey(String),
    #[error("\"{0}\" is repeated")]
    Repeated(String),
    #[error("a shortcut needs a key besides the modifiers")]
    NoKey,
    #[error("only one key besides the modifiers is allowed")]
    TwoKeys,
    #[error("{0} needs Ctrl, Alt or Win (it types or edits text)")]
    NeedsModifier(String),
}

impl Hotkey {
    /// Parses a shortcut (`"Ctrl+Alt+KeyI"`; names are not case-sensitive and the modifiers may
    /// come in any order, before the key).
    pub fn parse(text: &str) -> Result<Self, ShortcutError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ShortcutError::Empty);
        }
        let mut mods = Mods::default();
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            let flag = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Some(&mut mods.ctrl),
                "alt" => Some(&mut mods.alt),
                "shift" => Some(&mut mods.shift),
                "win" | "super" | "meta" => Some(&mut mods.win),
                _ => None,
            };
            if let Some(flag) = flag {
                if key.is_some() {
                    // Modifiers come first: "KeyI+Ctrl" is not a shortcut.
                    return Err(ShortcutError::UnknownKey(part.to_owned()));
                }
                if *flag {
                    return Err(ShortcutError::Repeated(part.to_owned()));
                }
                *flag = true;
                continue;
            }
            let found = KEYS
                .iter()
                .find(|info| info.name.eq_ignore_ascii_case(part))
                .ok_or_else(|| ShortcutError::UnknownKey(part.to_owned()))?;
            if key.replace(*found).is_some() {
                return Err(ShortcutError::TwoKeys);
            }
        }
        let key = key.ok_or(ShortcutError::NoKey)?;
        let hotkey = Self { mods, key };
        if key.class == KeyClass::Typing && !mods.blocks_typing() {
            return Err(ShortcutError::NeedsModifier(hotkey.to_string()));
        }
        Ok(hotkey)
    }

    /// The plugin's shortcut, to register it.
    pub fn shortcut(&self) -> Shortcut {
        let mut modifiers = Modifiers::empty();
        modifiers.set(Modifiers::CONTROL, self.mods.ctrl);
        modifiers.set(Modifiers::ALT, self.mods.alt);
        modifiers.set(Modifiers::SHIFT, self.mods.shift);
        modifiers.set(Modifiers::SUPER, self.mods.win);
        Shortcut::new((!modifiers.is_empty()).then_some(modifiers), self.key.code)
    }
}

impl fmt::Display for Hotkey {
    /// The canonical form the settings store: modifiers in a fixed order, then the key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (held, name) in [
            (self.mods.ctrl, "Ctrl"),
            (self.mods.alt, "Alt"),
            (self.mods.shift, "Shift"),
            (self.mods.win, "Win"),
        ] {
            if held {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(self.key.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<String, ShortcutError> {
        Hotkey::parse(text).map(|hotkey| hotkey.to_string())
    }

    #[test]
    fn parses_and_prints_the_canonical_form() {
        assert_eq!(parse("Ctrl+Alt+KeyI").unwrap(), "Ctrl+Alt+KeyI");
        // Any order and case in, one form out.
        assert_eq!(parse(" alt + control + keyi ").unwrap(), "Ctrl+Alt+KeyI");
        assert_eq!(parse("Win+Shift+Digit5").unwrap(), "Shift+Win+Digit5");
        assert_eq!(parse("F8").unwrap(), "F8");
        assert_eq!(parse("Shift+F13").unwrap(), "Shift+F13");
        assert_eq!(parse("ScrollLock").unwrap(), "ScrollLock");
    }

    #[test]
    fn every_key_round_trips() {
        for info in KEYS {
            let text = format!("Ctrl+Alt+{}", info.name);
            assert_eq!(parse(&text).unwrap(), text);
        }
    }

    #[test]
    fn rejects_what_is_not_a_shortcut() {
        assert_eq!(parse(""), Err(ShortcutError::Empty));
        assert_eq!(parse("Ctrl+Alt"), Err(ShortcutError::NoKey));
        assert_eq!(parse("Ctrl"), Err(ShortcutError::NoKey));
        assert_eq!(parse("Ctrl+KeyA+KeyB"), Err(ShortcutError::TwoKeys));
        assert_eq!(
            parse("Ctrl+Ctrl+KeyA"),
            Err(ShortcutError::Repeated("Ctrl".into()))
        );
        assert_eq!(
            parse("Ctrl+Mouse4"),
            Err(ShortcutError::UnknownKey("Mouse4".into()))
        );
        assert_eq!(
            parse("KeyI+Ctrl"),
            Err(ShortcutError::UnknownKey("Ctrl".into()))
        );
        // Escape and the modifier keys themselves are not offered.
        assert!(parse("Ctrl+Escape").is_err());
        assert!(parse("Ctrl+ShiftLeft").is_err());
    }

    #[test]
    fn typing_keys_need_ctrl_alt_or_win() {
        for bare in [
            "KeyT",
            "Digit1",
            "Space",
            "Shift+KeyT",
            "Shift+Enter",
            "Numpad5",
        ] {
            assert!(
                matches!(parse(bare), Err(ShortcutError::NeedsModifier(_))),
                "{bare} accepted"
            );
        }
        for ok in ["Ctrl+KeyT", "Alt+Digit1", "Win+Space", "Ctrl+Shift+Enter"] {
            assert!(parse(ok).is_ok(), "{ok} refused");
        }
    }

    #[test]
    fn converts_to_the_plugins_shortcut() {
        let hotkey = Hotkey::parse("Ctrl+Alt+KeyI").unwrap();
        assert_eq!(
            hotkey.shortcut(),
            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyI)
        );
        assert_eq!(
            Hotkey::parse("F8").unwrap().shortcut(),
            Shortcut::new(None, Code::F8)
        );
    }
}
