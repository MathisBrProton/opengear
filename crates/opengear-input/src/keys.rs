/// Canonical key names mapped to Windows Virtual-Key (VK) codes.
/// Uses standard Windows VK values so AZERTY / QWERTY both work reliably.
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
    ("MINUS", 0xDB),      // ) on AZERTY / [ on US
    ("EQUALS", 0xBB),     // = / +
    ("LBRACKET", 0xDD),   // ^
    ("RBRACKET", 0xBA),   // $
    ("BACKSLASH", 0xDC),  // *
    ("APOSTROPHE", 0xC0), // ù
    ("ISO_HASH", 0xDC),
    ("ISO_BSLASH", 0xE2), // < >
    ("COMMA", 0xBC),
    ("PERIOD", 0xBE),
    ("SLASH", 0xBF),
    ("EXCLAIM", 0xDF),
    ("VOLUME_MUTE", 0xAD),
    ("VOLUME_DOWN", 0xAE),
    ("VOLUME_UP", 0xAF),
    ("MEDIA_NEXT", 0xB0),
    ("MEDIA_PREV", 0xB1),
    ("MEDIA_STOP", 0xB2),
    ("MEDIA_PLAY", 0xB3),
];

pub const ALL_BINDABLE_KEYS: &[&str] = &[
    "ESC", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13",
    "F14", "F15", "F16", "F17", "F18", "F19", "F20", "PRINTSCREEN", "SCROLLLOCK", "PAUSE",
    "INSERT", "HOME", "PAGEUP", "DELETE", "END", "PAGEDOWN", "BACKSPACE", "TAB", "ENTER",
    "CAPSLOCK", "SPACE", "LSHIFT", "RSHIFT", "LCTRL", "RCTRL", "LALT", "RALT", "LWIN", "RWIN",
    "MENU", "LEFT", "UP", "RIGHT", "DOWN", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K",
    "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "0", "1", "2", "3",
    "4", "5", "6", "7", "8", "9", "VOLUME_MUTE", "VOLUME_DOWN", "VOLUME_UP", "MEDIA_NEXT",
    "MEDIA_PREV", "MEDIA_STOP", "MEDIA_PLAY",
];

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
