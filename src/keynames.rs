//! Key names that do not depend on the Windows display language.
//!
//! Windows' own key names (`GetKeyNameTextW`) are translated: a French Windows calls numpad 7
//! "7 (PAVE NUM.)". Hotkeys are named from their virtual-key code instead, so they read the
//! same everywhere: "Num 7", "Ctrl + Shift + F8".

/// What goes between the keys of a combination: "Ctrl + F8".
pub const JOIN: &str = " + ";

/// The name of the key with this virtual-key code, or `None` for a key this table does not
/// know (the caller then falls back to the name Windows gave when the key was assigned).
pub fn key_name(vk: u32) -> Option<String> {
    let fixed = match vk {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0C => "Clear",
        0x0D => "Enter",
        0x13 => "Pause",
        0x14 => "Caps Lock",
        0x1B => "Esc",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2C => "Print Screen",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5D => "Menu",
        0x6A => "Num *",
        0x6B => "Num +",
        0x6C => "Num ,",
        0x6D => "Num -",
        0x6E => "Num .",
        0x6F => "Num /",
        0x90 => "Num Lock",
        0x91 => "Scroll Lock",
        0xA6 => "Browser Back",
        0xA7 => "Browser Forward",
        0xA8 => "Browser Refresh",
        0xA9 => "Browser Stop",
        0xAA => "Browser Search",
        0xAB => "Browser Favorites",
        0xAC => "Browser Home",
        0xAD => "Volume Mute",
        0xAE => "Volume Down",
        0xAF => "Volume Up",
        0xB0 => "Next Track",
        0xB1 => "Previous Track",
        0xB2 => "Media Stop",
        0xB3 => "Play/Pause",
        0xB4 => "Mail",
        0xB5 => "Media Select",
        0xB6 => "App 1",
        0xB7 => "App 2",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.to_string());
    }
    match vk {
        // Digits and letters: the code is the ASCII character.
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from),
        0x60..=0x69 => Some(format!("Num {}", vk - 0x60)),
        0x70..=0x87 => Some(format!("F{}", vk - 0x6F)),
        // Punctuation keys differ per keyboard layout (not per display language): show the
        // character printed on the key.
        0xBA..=0xC0 | 0xDB..=0xDF | 0xE2 => layout_char(vk).or_else(|| us_char(vk)),
        _ => None,
    }
}

/// A key combination as its parts, modifiers first: `["Ctrl", "Shift", "F8"]`.
pub fn parts(ctrl: bool, shift: bool, alt: bool, win: bool, key: String) -> Vec<String> {
    let mut out = Vec::with_capacity(5);
    for (on, name) in [(ctrl, "Ctrl"), (shift, "Shift"), (alt, "Alt"), (win, "Win")] {
        if on {
            out.push(name.to_string());
        }
    }
    out.push(key);
    out
}

/// The key part of a label written by an older version ("Ctrl+Num 1" gives "Num 1").
pub fn strip_modifiers(label: &str) -> &str {
    let mut s = label.trim();
    loop {
        let rest = ["Ctrl", "Shift", "Alt", "Win"].iter().find_map(|m| {
            s.strip_prefix(m)
                .and_then(|r| r.trim_start().strip_prefix('+'))
        });
        match rest {
            // "Ctrl+" on its own would leave nothing: then the "+" was the key.
            Some(r) if !r.trim().is_empty() => s = r.trim(),
            _ => return s,
        }
    }
}

/// The character the current keyboard layout puts on a punctuation key.
#[cfg(windows)]
fn layout_char(vk: u32) -> Option<String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_CHAR, MapVirtualKeyW};
    // The top bit marks a dead key (an accent waiting for a letter); the character is below.
    let c = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0x7FFF_FFFF;
    char::from_u32(c)
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .map(|c| c.to_uppercase().collect())
}

#[cfg(not(windows))]
fn layout_char(_vk: u32) -> Option<String> {
    None
}

/// Punctuation keys on a US keyboard, for when the layout gives no character.
fn us_char(vk: u32) -> Option<String> {
    let c = match vk {
        0xBA => ";",
        0xBB => "=",
        0xBC => ",",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        0xE2 => "<",
        _ => return None,
    };
    Some(c.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_the_key_code_not_from_windows() {
        let name = |vk| key_name(vk).unwrap();
        assert_eq!(name(0x67), "Num 7");
        assert_eq!(name(0x60), "Num 0");
        assert_eq!(name(0x6E), "Num .");
        assert_eq!(name(0x77), "F8");
        assert_eq!(name(0x70), "F1");
        assert_eq!(name(0x87), "F24");
        assert_eq!(name(0x41), "A");
        assert_eq!(name(0x35), "5");
        assert_eq!(name(0x22), "Page Down");
        assert_eq!(name(0x20), "Space");
        assert_eq!(name(0x13), "Pause");
        assert_eq!(
            key_name(0xFF),
            None,
            "unknown keys fall back to the saved label"
        );
        // Punctuation always has some name, whatever the layout.
        assert!(!name(0xBA).is_empty());
    }

    #[test]
    fn combinations_list_the_modifiers_first() {
        assert_eq!(
            parts(true, true, false, false, "F8".into()).join(JOIN),
            "Ctrl + Shift + F8"
        );
        assert_eq!(parts(false, false, false, false, "Num 7".into()), ["Num 7"]);
        assert_eq!(
            parts(true, true, true, true, "A".into()),
            ["Ctrl", "Shift", "Alt", "Win", "A"]
        );
    }

    #[test]
    fn old_labels_give_up_their_key_name() {
        assert_eq!(strip_modifiers("Ctrl+Num 1"), "Num 1");
        assert_eq!(strip_modifiers("Ctrl + Shift + F8"), "F8");
        assert_eq!(strip_modifiers("7 (PAVE NUM.)"), "7 (PAVE NUM.)");
        assert_eq!(strip_modifiers("Ctrl+Num +"), "Num +");
        assert_eq!(strip_modifiers("Alt++"), "+");
        assert_eq!(
            strip_modifiers("Shift"),
            "Shift",
            "a key that is called Shift"
        );
        assert_eq!(strip_modifiers(""), "");
    }
}
