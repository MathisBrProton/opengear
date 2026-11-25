/// Canonical key names mapped to Windows Virtual-Key (VK) codes.
///
/// The names are **stable identifiers persisted in user profiles** (bindings, macros): never
/// rename one. Some OEM names come from the French layout where they were first used
/// (`LBRACKET` is VK 0xDD); the UI never shows them, it shows the label Windows gives the key.
pub const KEY_TABLE: &[(&str, u16)] = &[
    ("ESC", 0x1B),
    ("F1", 0x70),
    ("F2", 0x71),
    ("F3", 0x72),
    ("F4", 0x73),
    ("F5", 0x74),
    ("F6", 0x75),
    ("F7", 0x76),
    ("F8", 0x77),
    ("F9", 0x78),
    ("F10", 0x79),
    ("F11", 0x7A),
    ("F12", 0x7B),
    ("F13", 0x7C),
    ("F14", 0x7D),
    ("F15", 0x7E),
    ("F16", 0x7F),
    ("F17", 0x80),
    ("F18", 0x81),
    ("F19", 0x82),
    ("F20", 0x83),
    ("F21", 0x84),
    ("F22", 0x85),
    ("F23", 0x86),
    ("F24", 0x87),
    ("PRINTSCREEN", 0x2C),
    ("SCROLLLOCK", 0x91),
    ("PAUSE", 0x13),
    ("INSERT", 0x2D),
    ("HOME", 0x24),
    ("PAGEUP", 0x21),
    ("DELETE", 0x2E),
    ("END", 0x23),
    ("PAGEDOWN", 0x22),
    ("BACKSPACE", 0x08),
    ("TAB", 0x09),
    ("ENTER", 0x0D),
    ("CAPSLOCK", 0x14),
    ("SPACE", 0x20),
    ("LSHIFT", 0xA0),
    ("RSHIFT", 0xA1),
    ("LCTRL", 0xA2),
    ("RCTRL", 0xA3),
    ("LALT", 0xA4),
    ("RALT", 0xA5),
    ("LWIN", 0x5B),
    ("RWIN", 0x5C),
    ("MENU", 0x5D),
    ("LEFT", 0x25),
    ("UP", 0x26),
    ("RIGHT", 0x27),
    ("DOWN", 0x28),
    ("0", 0x30),
    ("1", 0x31),
    ("2", 0x32),
    ("3", 0x33),
    ("4", 0x34),
    ("5", 0x35),
    ("6", 0x36),
    ("7", 0x37),
    ("8", 0x38),
    ("9", 0x39),
    ("A", 0x41),
    ("B", 0x42),
    ("C", 0x43),
    ("D", 0x44),
    ("E", 0x45),
    ("F", 0x46),
    ("G", 0x47),
    ("H", 0x48),
    ("I", 0x49),
    ("J", 0x4A),
    ("K", 0x4B),
    ("L", 0x4C),
    ("M", 0x4D),
    ("N", 0x4E),
    ("O", 0x4F),
    ("P", 0x50),
    ("Q", 0x51),
    ("R", 0x52),
    ("S", 0x53),
    ("T", 0x54),
    ("U", 0x55),
    ("V", 0x56),
    ("W", 0x57),
    ("X", 0x58),
    ("Y", 0x59),
    ("Z", 0x5A),
    ("OEM_7", 0xDE),      // ² on AZERTY
    ("OEM_4", 0xDB),      // ) on AZERTY / [ on US
    ("MINUS", 0xBD),      // - / _ (VK_OEM_MINUS)
    ("EQUALS", 0xBB),     // = / +
    ("LBRACKET", 0xDD),   // ^
    ("RBRACKET", 0xBA),   // $
    ("BACKSLASH", 0xDC),  // *
    ("APOSTROPHE", 0xC0), // ù
    ("ISO_BSLASH", 0xE2), // < >
    ("COMMA", 0xBC),
    ("PERIOD", 0xBE),
    ("SLASH", 0xBF),
    ("EXCLAIM", 0xDF),
    ("NUMLOCK", 0x90),
    ("NUM0", 0x60),
    ("NUM1", 0x61),
    ("NUM2", 0x62),
    ("NUM3", 0x63),
    ("NUM4", 0x64),
    ("NUM5", 0x65),
    ("NUM6", 0x66),
    ("NUM7", 0x67),
    ("NUM8", 0x68),
    ("NUM9", 0x69),
    ("NUM_MUL", 0x6A),
    ("NUM_ADD", 0x6B),
    ("NUM_SUB", 0x6D),
    ("NUM_DEC", 0x6E),
    ("NUM_DIV", 0x6F),
    ("VOLUME_MUTE", 0xAD),
    ("VOLUME_DOWN", 0xAE),
    ("VOLUME_UP", 0xAF),
    ("MEDIA_NEXT", 0xB0),
    ("MEDIA_PREV", 0xB1),
    ("MEDIA_STOP", 0xB2),
    ("MEDIA_PLAY", 0xB3),
];

pub const ALL_BINDABLE_KEYS: &[&str] = &[
    "ESC",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "F13",
    "F14",
    "F15",
    "F16",
    "F17",
    "F18",
    "F19",
    "F20",
    "PRINTSCREEN",
    "SCROLLLOCK",
    "PAUSE",
    "INSERT",
    "HOME",
    "PAGEUP",
    "DELETE",
    "END",
    "PAGEDOWN",
    "BACKSPACE",
    "TAB",
    "ENTER",
    "CAPSLOCK",
    "SPACE",
    "LSHIFT",
    "RSHIFT",
    "LCTRL",
    "RCTRL",
    "LALT",
    "RALT",
    "LWIN",
    "RWIN",
    "MENU",
    "LEFT",
    "UP",
    "RIGHT",
    "DOWN",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "MINUS",
    "EQUALS",
    "OEM_4",
    "LBRACKET",
    "RBRACKET",
    "BACKSLASH",
    "APOSTROPHE",
    "OEM_7",
    "EXCLAIM",
    "ISO_BSLASH",
    "COMMA",
    "PERIOD",
    "SLASH",
    "NUMLOCK",
    "NUM0",
    "NUM1",
    "NUM2",
    "NUM3",
    "NUM4",
    "NUM5",
    "NUM6",
    "NUM7",
    "NUM8",
    "NUM9",
    "NUM_MUL",
    "NUM_ADD",
    "NUM_SUB",
    "NUM_DEC",
    "NUM_DIV",
    "VOLUME_MUTE",
    "VOLUME_DOWN",
    "VOLUME_UP",
    "MEDIA_NEXT",
    "MEDIA_PREV",
    "MEDIA_STOP",
    "MEDIA_PLAY",
];

/// VK code of a physical key (W3C `code`) whose meaning does not depend on the Windows layout.
pub fn fixed_vk_for_code(code: &str) -> Option<u16> {
    let fkey = code
        .strip_prefix('F')
        .and_then(|n| n.parse::<u16>().ok())
        .filter(|n| (1..=24).contains(n));
    if let Some(n) = fkey {
        return Some(0x6F + n);
    }
    let numpad_digit = code
        .strip_prefix("Numpad")
        .and_then(|n| n.parse::<u16>().ok())
        .filter(|n| *n <= 9);
    if let Some(n) = numpad_digit {
        return Some(0x60 + n);
    }
    Some(match code {
        "Escape" => 0x1B,
        "PrintScreen" => 0x2C,
        "ScrollLock" => 0x91,
        "Pause" => 0x13,
        "Backspace" => 0x08,
        "Tab" => 0x09,
        "CapsLock" => 0x14,
        "Enter" | "NumpadEnter" => 0x0D,
        "ShiftLeft" => 0xA0,
        "ShiftRight" => 0xA1,
        "ControlLeft" => 0xA2,
        "ControlRight" => 0xA3,
        "AltLeft" => 0xA4,
        "AltRight" => 0xA5,
        "MetaLeft" => 0x5B,
        "MetaRight" => 0x5C,
        "ContextMenu" => 0x5D,
        "Space" => 0x20,
        "Insert" => 0x2D,
        "Home" => 0x24,
        "PageUp" => 0x21,
        "Delete" => 0x2E,
        "End" => 0x23,
        "PageDown" => 0x22,
        "ArrowUp" => 0x26,
        "ArrowDown" => 0x28,
        "ArrowLeft" => 0x25,
        "ArrowRight" => 0x27,
        "NumLock" => 0x90,
        "NumpadDivide" => 0x6F,
        "NumpadMultiply" => 0x6A,
        "NumpadSubtract" => 0x6D,
        "NumpadAdd" => 0x6B,
        "NumpadDecimal" => 0x6E,
        "MediaStop" => 0xB2,
        "MediaTrackPrevious" => 0xB1,
        "MediaPlayPause" => 0xB3,
        "MediaTrackNext" => 0xB0,
        "AudioVolumeMute" => 0xAD,
        "AudioVolumeDown" => 0xAE,
        "AudioVolumeUp" => 0xAF,
        _ => return None,
    })
}

/// VK code a layout-dependent key produces with the US QWERTY layout (fallback when Windows
/// cannot be queried).
pub fn us_vk_for_code(code: &str) -> Option<u16> {
    if let Some(c) = code.strip_prefix("Key").filter(|c| c.len() == 1) {
        return Some(c.as_bytes()[0] as u16);
    }
    if let Some(c) = code.strip_prefix("Digit").filter(|c| c.len() == 1) {
        return Some(c.as_bytes()[0] as u16);
    }
    Some(match code {
        "Backquote" => 0xC0,
        "Minus" => 0xBD,
        "Equal" => 0xBB,
        "BracketLeft" => 0xDB,
        "BracketRight" => 0xDD,
        "Backslash" => 0xDC,
        "Semicolon" => 0xBA,
        "Quote" => 0xDE,
        "Comma" => 0xBC,
        "Period" => 0xBE,
        "Slash" => 0xBF,
        "IntlBackslash" => 0xE2,
        _ => return None,
    })
}

pub fn key_name_to_vk(name: &str) -> Option<u16> {
    let upper = name.trim().to_ascii_uppercase();
    KEY_TABLE
        .iter()
        .find(|(k, _)| *k == upper)
        .map(|(_, vk)| *vk)
}

pub fn vk_to_key_name(vk: u16) -> Option<&'static str> {
    KEY_TABLE
        .iter()
        .find(|(_, code)| *code == vk)
        .map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn key_table_has_unique_names_and_vks() {
        let mut names = HashSet::new();
        let mut vks = HashSet::new();
        for &(name, vk) in KEY_TABLE {
            assert!(names.insert(name), "duplicate key name: {name}");
            assert!(vks.insert(vk), "duplicate VK code: 0x{vk:02X} ({name})");
        }
        assert_eq!(key_name_to_vk("minus"), Some(0xBD));
        assert_eq!(key_name_to_vk("num5"), Some(0x65));
        assert_eq!(vk_to_key_name(0xDC), Some("BACKSLASH"));
        for name in ALL_BINDABLE_KEYS {
            assert!(
                key_name_to_vk(name).is_some(),
                "touche assignable inconnue : {name}"
            );
        }
        assert_eq!(fixed_vk_for_code("F12"), Some(0x7B));
        assert_eq!(fixed_vk_for_code("Numpad7"), Some(0x67));
        assert_eq!(fixed_vk_for_code("KeyQ"), None);
        assert_eq!(us_vk_for_code("KeyQ"), Some(0x51));
        assert_eq!(us_vk_for_code("Quote"), Some(0xDE));
    }
}
