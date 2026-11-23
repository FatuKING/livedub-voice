//! Whether a shortcut types a character on the user's keyboard layout.
//!
//! Windows treats `AltGr` as `Ctrl+Alt`, so on many layouts a `Ctrl+Alt` combination is how a
//! character is typed (Polish `AltGr+O` = ó, German `AltGr+Q` = @, Hungarian `AltGr+B` = {).
//! Registering it as a hotkey would take that character away from every app, so such a shortcut
//! is never registered. The check asks the layout what the key types with `Ctrl+Alt` (and
//! `Shift`) held, without touching the keyboard state (`ToUnicodeEx` with flag 4).

use super::Hotkey;

/// Whether `hotkey` types a character (or is a dead key) on the current keyboard layout. Only
/// `Ctrl+Alt` combinations without `Win` can.
pub fn types_character(hotkey: &Hotkey) -> bool {
    if !(hotkey.mods.ctrl && hotkey.mods.alt) || hotkey.mods.win {
        return false;
    }
    imp::types_with_altgr(hotkey.key.vk, hotkey.mods.shift)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MAPVK_VK_TO_VSC, MapVirtualKeyExW, ToUnicodeEx, VK_CONTROL, VK_LCONTROL,
        VK_MENU, VK_RMENU, VK_SHIFT,
    };

    /// `ToUnicodeEx` flag: do not change the keyboard state (Windows 10 1607+).
    const NO_STATE_CHANGE: u32 = 0x4;

    pub fn types_with_altgr(vk: u16, shift: bool) -> bool {
        let mut state = [0_u8; 256];
        for key in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU] {
            state[usize::from(key.0)] = 0x80;
        }
        if shift {
            state[usize::from(VK_SHIFT.0)] = 0x80;
        }
        let mut buffer = [0_u16; 8];
        // SAFETY: plain Win32 calls on local buffers; the layout handle comes from Windows.
        let typed = unsafe {
            let layout = GetKeyboardLayout(0);
            let scan = MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC, Some(layout));
            ToUnicodeEx(
                u32::from(vk),
                scan,
                &state,
                &mut buffer,
                NO_STATE_CHANGE,
                Some(layout),
            )
        };
        match usize::try_from(typed) {
            // A dead key: it types with the next key.
            Err(_) => true,
            Ok(count) => buffer[..count.min(buffer.len())]
                .iter()
                .any(|&unit| unit >= 0x20 && unit != 0x7F),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn types_with_altgr(_vk: u16, _shift: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ctrl_alt_combinations_can_type() {
        for text in [
            "F8",
            "Ctrl+KeyO",
            "Alt+KeyO",
            "Ctrl+Alt+Win+KeyO",
            "Ctrl+Shift+KeyO",
        ] {
            assert!(
                !types_character(&Hotkey::parse(text).unwrap()),
                "{text} said to type"
            );
        }
    }

    #[test]
    fn function_keys_never_type() {
        // Whatever the layout of the machine running the test.
        assert!(!types_character(&Hotkey::parse("Ctrl+Alt+F5").unwrap()));
    }
}
