use opengear_core::KeyboardLayout;

/// Total LED indices in Corsair K70 RGB TKL Bragi frame (matches ckb-next LED_CASE_K(P_K70_TKL, 193)).
pub const K70_TKL_LED_COUNT: usize = 193;

#[derive(Debug, Clone, Copy)]
pub struct KeyboardLayoutDesc {
    pub name: &'static str,
    pub led_count: usize,
    pub keys: &'static [KeyDef],
}

/// Look up physical LED/key geometry by Corsair `(vid, pid)`.
pub fn layout_for_device(vid: u16, pid: u16) -> Option<KeyboardLayoutDesc> {
    if vid == crate::proto::CORSAIR_VID
        && matches!(
            pid,
            crate::proto::PID_K70_RGB_TKL | crate::proto::PID_K70_RGB_TKL_CS
        )
    {
        return Some(KeyboardLayoutDesc {
            name: "Corsair K70 RGB TKL",
            led_count: K70_TKL_LED_COUNT,
            keys: K70_TKL_KEYS,
        });
    }
    None
}

/// Map a hardware Bragi LED index back to its W3C physical `key_code` (for legacy profile migration).
pub fn key_code_for_led_index(vid: u16, pid: u16, led_index: usize) -> Option<&'static str> {
    let keys = layout_for_device(vid, pid)
        .map(|d| d.keys)
        .unwrap_or(K70_TKL_KEYS);
    keys.iter()
        .find(|k| k.led_index == led_index)
        .and_then(|k| k.code())
}

#[derive(Debug, Clone, Copy)]
pub struct KeyDef {
    /// Canonical key identifier used for bindings/macros (e.g. "ESC", "A", "LCTRL", "LOGO_L")
    pub id: &'static str,
    /// Label displayed when AZERTY (ISO FR) mode is active
    pub label_azerty: &'static str,
    /// Label displayed when QWERTY (ANSI US) mode is active
    pub label_qwerty: &'static str,
    /// Bragi LED index (0..193)
    pub led_index: usize,
    /// Visual grid coordinates (in key-unit coordinates, x in 0.0..18.5, y in 0.0..7.2)
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Whether this entry corresponds to a real remappable keyboard key (false for logos)
    pub remappable: bool,
}

impl KeyDef {
    /// Physical position of this key (W3C `KeyboardEvent.code`), `None` for LED-only zones and
    /// keys the OS never sees (logos, profile / brightness buttons, Fn).
    pub fn code(&self) -> Option<&'static str> {
        code_for_key_id(self.id)
    }

    pub fn label(&self, azerty: bool) -> &'static str {
        if azerty {
            self.label_azerty
        } else {
            self.label_qwerty
        }
    }

    pub fn norm_x(&self) -> f32 {
        ((self.x + self.width * 0.5) / 18.25).clamp(0.0, 1.0)
    }

    pub fn norm_y(&self) -> f32 {
        ((self.y + self.height * 0.5) / 7.2).clamp(0.0, 1.0)
    }
}

/// Corsair key identifier -> W3C physical code. The Corsair ids name the French (AZERTY)
/// legend of each position (`A` is the key right of Tab), hence the apparent swaps.
fn code_for_key_id(id: &str) -> Option<&'static str> {
    Some(match id {
        "MEDIA_STOP" => "MediaStop",
        "MEDIA_PREV" => "MediaTrackPrevious",
        "MEDIA_PLAY" => "MediaPlayPause",
        "MEDIA_NEXT" => "MediaTrackNext",
        "VOLUME_MUTE" => "AudioVolumeMute",
        "ESC" => "Escape",
        "F1" => "F1",
        "F2" => "F2",
        "F3" => "F3",
        "F4" => "F4",
        "F5" => "F5",
        "F6" => "F6",
        "F7" => "F7",
        "F8" => "F8",
        "F9" => "F9",
        "F10" => "F10",
        "F11" => "F11",
        "F12" => "F12",
        "PRINTSCREEN" => "PrintScreen",
        "SCROLLLOCK" => "ScrollLock",
        "PAUSE" => "Pause",
        "OEM_7" => "Backquote",
        "1" => "Digit1",
        "2" => "Digit2",
        "3" => "Digit3",
        "4" => "Digit4",
        "5" => "Digit5",
        "6" => "Digit6",
        "7" => "Digit7",
        "8" => "Digit8",
        "9" => "Digit9",
        "0" => "Digit0",
        "MINUS" => "Minus",
        "EQUALS" => "Equal",
        "BACKSPACE" => "Backspace",
        "INSERT" => "Insert",
        "HOME" => "Home",
        "PAGEUP" => "PageUp",
        "TAB" => "Tab",
        "A" => "KeyQ",
        "Z" => "KeyW",
        "E" => "KeyE",
        "R" => "KeyR",
        "T" => "KeyT",
        "Y" => "KeyY",
        "U" => "KeyU",
        "I" => "KeyI",
        "O" => "KeyO",
        "P" => "KeyP",
        "LBRACKET" => "BracketLeft",
        "RBRACKET" => "BracketRight",
        "BACKSLASH" | "ISO_HASH" => "Backslash",
        "DELETE" => "Delete",
        "END" => "End",
        "PAGEDOWN" => "PageDown",
        "CAPSLOCK" => "CapsLock",
        "Q" => "KeyA",
        "S" => "KeyS",
        "D" => "KeyD",
        "F" => "KeyF",
        "G" => "KeyG",
        "H" => "KeyH",
        "J" => "KeyJ",
        "K" => "KeyK",
        "L" => "KeyL",
        "M" => "Semicolon",
        "APOSTROPHE" => "Quote",
        "ENTER" => "Enter",
        "LSHIFT" => "ShiftLeft",
        "ISO_BSLASH" => "IntlBackslash",
        "W" => "KeyZ",
        "X" => "KeyX",
        "C" => "KeyC",
        "V" => "KeyV",
        "B" => "KeyB",
        "N" => "KeyN",
        "COMMA" => "KeyM",
        "PERIOD" => "Comma",
        "SLASH" => "Period",
        "EXCLAIM" => "Slash",
        "RSHIFT" => "ShiftRight",
        "UP" => "ArrowUp",
        "LCTRL" => "ControlLeft",
        "LWIN" => "MetaLeft",
        "LALT" => "AltLeft",
        "SPACE" => "Space",
        "RALT" => "AltRight",
        "MENU" => "ContextMenu",
        "RCTRL" => "ControlRight",
        "LEFT" => "ArrowLeft",
        "DOWN" => "ArrowDown",
        "RIGHT" => "ArrowRight",
        _ => return None,
    })
}

/// Keys of `desc` as physically present on a board of this standard. The LED frame covers both
/// ANSI and ISO variants; drawing both would show keys the user does not have.
pub fn keys_for_layout(desc: &KeyboardLayoutDesc, layout: KeyboardLayout) -> Vec<KeyDef> {
    let ansi = matches!(layout, KeyboardLayout::Ansi);
    desc.keys
        .iter()
        .filter(|k| match k.id {
            "BACKSLASH" => ansi,
            "ISO_HASH" | "ISO_BSLASH" => !ansi,
            _ => true,
        })
        .map(|k| {
            let mut k = *k;
            match (k.id, ansi) {
                ("LSHIFT", true) => k.width = 2.25,
                ("ENTER", true) => {
                    k.x = 12.75;
                    k.width = 2.25;
                }
                ("ENTER", false) => {
                    k.x = 13.75;
                    k.y -= 1.0;
                    k.width = 1.25;
                    k.height = 2.0;
                }
                _ => {}
            }
            k
        })
        .collect()
}

const fn k(
    id: &'static str,
    az: &'static str,
    qw: &'static str,
    led: usize,
    x: f32,
    y: f32,
    w: f32,
) -> KeyDef {
    KeyDef {
        id,
        label_azerty: az,
        label_qwerty: qw,
        led_index: led,
        x,
        y,
        width: w,
        height: 1.0,
        remappable: true,
    }
}

const fn led_only(
    id: &'static str,
    label: &'static str,
    led: usize,
    x: f32,
    y: f32,
    w: f32,
) -> KeyDef {
    KeyDef {
        id,
        label_azerty: label,
        label_qwerty: label,
        led_index: led,
        x,
        y,
        width: w,
        height: 0.72,
        remappable: false,
    }
}

/// Exact Corsair K70 RGB TKL Champion Series physical & LED index map
/// (indices cross-verified from OpenRGB `corsair_tkl_values` + `corsair_K70_TKL_cs_layout`).
pub const K70_TKL_KEYS: &[KeyDef] = &[
    // Row 0: Dedicated Media bar + Corsair Logo + Profile/Brightness/WinLock/Mute
    KeyDef {
        id: "MEDIA_STOP",
        label_azerty: "Stop",
        label_qwerty: "Stop",
        led_index: 123,
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 0.72,
        remappable: true,
    },
    KeyDef {
        id: "MEDIA_PREV",
        label_azerty: "Prev",
        label_qwerty: "Prev",
        led_index: 126,
        x: 1.05,
        y: 0.0,
        width: 1.0,
        height: 0.72,
        remappable: true,
    },
    KeyDef {
        id: "MEDIA_PLAY",
        label_azerty: "Play",
        label_qwerty: "Play",
        led_index: 124,
        x: 2.1,
        y: 0.0,
        width: 1.0,
        height: 0.72,
        remappable: true,
    },
    KeyDef {
        id: "MEDIA_NEXT",
        label_azerty: "Next",
        label_qwerty: "Next",
        led_index: 125,
        x: 3.15,
        y: 0.0,
        width: 1.0,
        height: 0.72,
        remappable: true,
    },
    led_only("LOGO_L", "Logo L", 1, 7.2, 0.0, 1.3),
    led_only("LOGO_R", "Logo R", 3, 8.6, 0.0, 1.3),
    led_only("BTN_PROFILE", "Prof", 128, 12.1, 0.0, 0.9),
    led_only("BTN_LIGHT", "Lum", 113, 13.05, 0.0, 0.9),
    led_only("BTN_WINLOCK", "Lock", 114, 14.0, 0.0, 0.9),
    KeyDef {
        id: "VOLUME_MUTE",
        label_azerty: "Mute",
        label_qwerty: "Mute",
        led_index: 102,
        x: 15.25,
        y: 0.0,
        width: 1.1,
        height: 0.72,
        remappable: true,
    },
    // Row 1: Function row (y = 1.05)
    k("ESC", "Echap", "Esc", 41, 0.0, 1.05, 1.0),
    k("F1", "F1", "F1", 58, 2.0, 1.05, 1.0),
    k("F2", "F2", "F2", 59, 3.0, 1.05, 1.0),
    k("F3", "F3", "F3", 60, 4.0, 1.05, 1.0),
    k("F4", "F4", "F4", 61, 5.0, 1.05, 1.0),
    k("F5", "F5", "F5", 62, 6.5, 1.05, 1.0),
    k("F6", "F6", "F6", 63, 7.5, 1.05, 1.0),
    k("F7", "F7", "F7", 64, 8.5, 1.05, 1.0),
    k("F8", "F8", "F8", 65, 9.5, 1.05, 1.0),
    k("F9", "F9", "F9", 66, 11.0, 1.05, 1.0),
    k("F10", "F10", "F10", 67, 12.0, 1.05, 1.0),
    k("F11", "F11", "F11", 68, 13.0, 1.05, 1.0),
    k("F12", "F12", "F12", 69, 14.0, 1.05, 1.0),
    k("PRINTSCREEN", "Impr", "PrtSc", 70, 15.25, 1.05, 1.0),
    k("SCROLLLOCK", "Arret", "ScrLk", 71, 16.25, 1.05, 1.0),
    k("PAUSE", "Pause", "Pause", 72, 17.25, 1.05, 1.0),
    // Row 2: Number row (y = 2.25)
    k("OEM_7", "²", "`", 53, 0.0, 2.25, 1.0),
    k("1", "& 1", "1", 30, 1.0, 2.25, 1.0),
    k("2", "é 2", "2", 31, 2.0, 2.25, 1.0),
    k("3", "\" 3", "3", 32, 3.0, 2.25, 1.0),
    k("4", "' 4", "4", 33, 4.0, 2.25, 1.0),
    k("5", "( 5", "5", 34, 5.0, 2.25, 1.0),
    k("6", "- 6", "6", 35, 6.0, 2.25, 1.0),
    k("7", "è 7", "7", 36, 7.0, 2.25, 1.0),
    k("8", "_ 8", "8", 37, 8.0, 2.25, 1.0),
    k("9", "ç 9", "9", 38, 9.0, 2.25, 1.0),
    k("0", "à 0", "0", 39, 10.0, 2.25, 1.0),
    k("MINUS", ")", "-", 45, 11.0, 2.25, 1.0),
    k("EQUALS", "=", "=", 46, 12.0, 2.25, 1.0),
    k("BACKSPACE", "Ret.Arr", "Bksp", 42, 13.0, 2.25, 2.0),
    k("INSERT", "Inser", "Ins", 73, 15.25, 2.25, 1.0),
    k("HOME", "Deb", "Home", 74, 16.25, 2.25, 1.0),
    k("PAGEUP", "Pg+", "PgUp", 75, 17.25, 2.25, 1.0),
    // Row 3: AZERTY / QWERTY top alpha row (y = 3.25)
    k("TAB", "Tab", "Tab", 43, 0.0, 3.25, 1.5),
    k("A", "A", "Q", 20, 1.5, 3.25, 1.0),
    k("Z", "Z", "W", 26, 2.5, 3.25, 1.0),
    k("E", "E", "E", 8, 3.5, 3.25, 1.0),
    k("R", "R", "R", 21, 4.5, 3.25, 1.0),
    k("T", "T", "T", 23, 5.5, 3.25, 1.0),
    k("Y", "Y", "Y", 28, 6.5, 3.25, 1.0),
    k("U", "U", "U", 24, 7.5, 3.25, 1.0),
    k("I", "I", "I", 12, 8.5, 3.25, 1.0),
    k("O", "O", "O", 18, 9.5, 3.25, 1.0),
    k("P", "P", "P", 19, 10.5, 3.25, 1.0),
    k("LBRACKET", "^", "[", 47, 11.5, 3.25, 1.0),
    k("RBRACKET", "$", "]", 48, 12.5, 3.25, 1.0),
    k("BACKSLASH", "*", "\\", 49, 13.5, 3.25, 1.5),
    k("DELETE", "Suppr", "Del", 76, 15.25, 3.25, 1.0),
    k("END", "Fin", "End", 77, 16.25, 3.25, 1.0),
    k("PAGEDOWN", "Pg-", "PgDn", 78, 17.25, 3.25, 1.0),
    // Row 4: Home row (y = 4.25)
    k("CAPSLOCK", "Verr.Maj", "Caps", 57, 0.0, 4.25, 1.75),
    k("Q", "Q", "A", 4, 1.75, 4.25, 1.0),
    k("S", "S", "S", 22, 2.75, 4.25, 1.0),
    k("D", "D", "D", 7, 3.75, 4.25, 1.0),
    k("F", "F", "F", 9, 4.75, 4.25, 1.0),
    k("G", "G", "G", 10, 5.75, 4.25, 1.0),
    k("H", "H", "H", 11, 6.75, 4.25, 1.0),
    k("J", "J", "J", 13, 7.75, 4.25, 1.0),
    k("K", "K", "K", 14, 8.75, 4.25, 1.0),
    k("L", "L", "L", 15, 9.75, 4.25, 1.0),
    k("M", "M", ";", 51, 10.75, 4.25, 1.0),
    k("APOSTROPHE", "ù", "'", 52, 11.75, 4.25, 1.0),
    k("ISO_HASH", "*", "#", 50, 12.75, 4.25, 0.9),
    k("ENTER", "Entrée", "Enter", 40, 13.65, 4.25, 1.35),
    // Row 5: Bottom alpha row (y = 5.25)
    k("LSHIFT", "Shift", "Shift", 106, 0.0, 5.25, 1.25),
    k("ISO_BSLASH", "< >", "\\", 100, 1.25, 5.25, 1.0),
    k("W", "W", "Z", 29, 2.25, 5.25, 1.0),
    k("X", "X", "X", 27, 3.25, 5.25, 1.0),
    k("C", "C", "C", 6, 4.25, 5.25, 1.0),
    k("V", "V", "V", 25, 5.25, 5.25, 1.0),
    k("B", "B", "B", 5, 6.25, 5.25, 1.0),
    k("N", "N", "N", 17, 7.25, 5.25, 1.0),
    k("COMMA", ",", "M", 16, 8.25, 5.25, 1.0),
    k("PERIOD", ";", ",", 54, 9.25, 5.25, 1.0),
    k("SLASH", ":", ".", 55, 10.25, 5.25, 1.0),
    k("EXCLAIM", "!", "/", 56, 11.25, 5.25, 1.0),
    k("RSHIFT", "Shift D", "RShift", 110, 12.25, 5.25, 2.75),
    k("UP", "↑", "↑", 82, 16.25, 5.25, 1.0),
    // Row 6: Modifiers + Space + Arrows (y = 6.25)
    k("LCTRL", "Ctrl", "Ctrl", 105, 0.0, 6.25, 1.25),
    k("LWIN", "Win", "Win", 108, 1.25, 6.25, 1.25),
    k("LALT", "Alt", "Alt", 107, 2.5, 6.25, 1.25),
    k("SPACE", "Espace", "Space", 44, 3.75, 6.25, 6.25),
    k("RALT", "AltGr", "RAlt", 111, 10.0, 6.25, 1.25),
    k("FN", "Fn", "Fn", 122, 11.25, 6.25, 1.25),
    k("MENU", "Menu", "Menu", 101, 12.5, 6.25, 1.25),
    k("RCTRL", "Ctrl D", "RCtrl", 109, 13.75, 6.25, 1.25),
    k("LEFT", "←", "←", 80, 15.25, 6.25, 1.0),
    k("DOWN", "↓", "↓", 81, 16.25, 6.25, 1.0),
    k("RIGHT", "→", "→", 79, 17.25, 6.25, 1.0),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn k70_tkl_leds_are_unique_and_within_bounds() {
        let mut seen = HashSet::new();
        for key in K70_TKL_KEYS {
            assert!(
                key.led_index < K70_TKL_LED_COUNT,
                "LED {} out of bounds for {}",
                key.led_index,
                key.id
            );
            assert!(
                seen.insert(key.led_index),
                "Duplicate LED index {} on {}",
                key.led_index,
                key.id
            );
            assert!((0.0..=1.0).contains(&key.norm_x()));
            assert!((0.0..=1.0).contains(&key.norm_y()));
        }
        assert!(layout_for_device(0x1B1C, 0x1B73).is_some());
        assert!(layout_for_device(0x1B1C, 0xFFFF).is_none());
    }

    #[test]
    fn every_os_visible_key_has_a_physical_code() {
        let no_code = [
            "LOGO_L",
            "LOGO_R",
            "BTN_PROFILE",
            "BTN_LIGHT",
            "BTN_WINLOCK",
            "FN",
        ];
        for key in K70_TKL_KEYS {
            assert_eq!(
                key.code().is_none(),
                no_code.contains(&key.id),
                "{}",
                key.id
            );
        }
        let desc = layout_for_device(0x1B1C, 0x1B73).unwrap();
        for layout in [KeyboardLayout::Ansi, KeyboardLayout::Iso] {
            let keys = keys_for_layout(&desc, layout);
            let mut codes = HashSet::new();
            for k in keys.iter().filter_map(|k| k.code()) {
                assert!(codes.insert(k), "{layout:?}: {k} en double");
            }
            assert_eq!(
                codes.contains("IntlBackslash"),
                layout == KeyboardLayout::Iso
            );
        }
    }
}
