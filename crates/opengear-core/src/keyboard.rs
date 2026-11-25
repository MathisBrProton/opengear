//! Vendor-neutral physical keyboard geometry.
//!
//! Keys are identified by their **physical position** using the W3C `KeyboardEvent.code` names
//! (`KeyQ` is the key right of Tab on every keyboard, whatever its label). What a key *types*
//! depends on the Windows layout and is resolved at runtime (see `opengear-platform::keyboard`);
//! per-device LED maps are attached by the driver crates.

use crate::KeyboardLayout;
use serde::{Deserialize, Serialize};

/// Overall size class of a keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormFactor {
    /// 100 %: F-row, navigation cluster, arrows and numeric keypad.
    Full,
    /// Tenkeyless (≈ 80 %): no numeric keypad.
    Tkl,
    /// 75 %: compact F-row, one column of navigation keys.
    Compact75,
    /// 65 %: no F-row, arrows and one navigation column.
    Compact65,
    /// 60 %: alphanumeric block only.
    Compact60,
}

impl FormFactor {
    pub const ALL: [FormFactor; 5] = [
        Self::Full,
        Self::Tkl,
        Self::Compact75,
        Self::Compact65,
        Self::Compact60,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Tkl => "tkl",
            Self::Compact75 => "75",
            Self::Compact65 => "65",
            Self::Compact60 => "60",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Complet (100 %)",
            Self::Tkl => "Sans pavé numérique (TKL)",
            Self::Compact75 => "Compact 75 %",
            Self::Compact65 => "Compact 65 %",
            Self::Compact60 => "Compact 60 %",
        }
    }
}

/// One physical key, positioned in key units (1.0 = one standard keycap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalKey {
    pub code: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Keys whose meaning depends on the OS keyboard layout, with their PC/AT set-1 scan code.
/// Every other key has a fixed meaning (Escape, F1, arrows…).
const CHAR_KEY_SCANS: &[(&str, u16)] = &[
    ("Backquote", 0x29),
    ("Digit1", 0x02),
    ("Digit2", 0x03),
    ("Digit3", 0x04),
    ("Digit4", 0x05),
    ("Digit5", 0x06),
    ("Digit6", 0x07),
    ("Digit7", 0x08),
    ("Digit8", 0x09),
    ("Digit9", 0x0A),
    ("Digit0", 0x0B),
    ("Minus", 0x0C),
    ("Equal", 0x0D),
    ("KeyQ", 0x10),
    ("KeyW", 0x11),
    ("KeyE", 0x12),
    ("KeyR", 0x13),
    ("KeyT", 0x14),
    ("KeyY", 0x15),
    ("KeyU", 0x16),
    ("KeyI", 0x17),
    ("KeyO", 0x18),
    ("KeyP", 0x19),
    ("BracketLeft", 0x1A),
    ("BracketRight", 0x1B),
    ("KeyA", 0x1E),
    ("KeyS", 0x1F),
    ("KeyD", 0x20),
    ("KeyF", 0x21),
    ("KeyG", 0x22),
    ("KeyH", 0x23),
    ("KeyJ", 0x24),
    ("KeyK", 0x25),
    ("KeyL", 0x26),
    ("Semicolon", 0x27),
    ("Quote", 0x28),
    ("Backslash", 0x2B),
    ("IntlBackslash", 0x56),
    ("KeyZ", 0x2C),
    ("KeyX", 0x2D),
    ("KeyC", 0x2E),
    ("KeyV", 0x2F),
    ("KeyB", 0x30),
    ("KeyN", 0x31),
    ("KeyM", 0x32),
    ("Comma", 0x33),
    ("Period", 0x34),
    ("Slash", 0x35),
];

/// Scan code of a layout-dependent key (`None` for fixed-meaning keys).
pub fn char_key_scan(code: &str) -> Option<u16> {
    CHAR_KEY_SCANS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, s)| *s)
}

/// Label used when the OS layout cannot be queried: US QWERTY for character keys, short French
/// names for the others.
pub fn default_label(code: &str) -> &'static str {
    match code {
        "Escape" => "Échap",
        "Backquote" => "`",
        "Digit1" => "1",
        "Digit2" => "2",
        "Digit3" => "3",
        "Digit4" => "4",
        "Digit5" => "5",
        "Digit6" => "6",
        "Digit7" => "7",
        "Digit8" => "8",
        "Digit9" => "9",
        "Digit0" => "0",
        "Minus" => "-",
        "Equal" => "=",
        "Backspace" => "⌫",
        "Tab" => "Tab",
        "KeyQ" => "Q",
        "KeyW" => "W",
        "KeyE" => "E",
        "KeyR" => "R",
        "KeyT" => "T",
        "KeyY" => "Y",
        "KeyU" => "U",
        "KeyI" => "I",
        "KeyO" => "O",
        "KeyP" => "P",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "CapsLock" => "Verr Maj",
        "KeyA" => "A",
        "KeyS" => "S",
        "KeyD" => "D",
        "KeyF" => "F",
        "KeyG" => "G",
        "KeyH" => "H",
        "KeyJ" => "J",
        "KeyK" => "K",
        "KeyL" => "L",
        "Semicolon" => ";",
        "Quote" => "'",
        "Enter" => "Entrée",
        "ShiftLeft" | "ShiftRight" => "Maj",
        "IntlBackslash" => "<",
        "KeyZ" => "Z",
        "KeyX" => "X",
        "KeyC" => "C",
        "KeyV" => "V",
        "KeyB" => "B",
        "KeyN" => "N",
        "KeyM" => "M",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "ControlLeft" | "ControlRight" => "Ctrl",
        "MetaLeft" | "MetaRight" => "Win",
        "AltLeft" => "Alt",
        "AltRight" => "Alt",
        "ContextMenu" => "Menu",
        "Space" => "Espace",
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
        "PrintScreen" => "Impr",
        "ScrollLock" => "Défil",
        "Pause" => "Pause",
        "Insert" => "Inser",
        "Home" => "Début",
        "PageUp" => "Pg ↑",
        "Delete" => "Suppr",
        "End" => "Fin",
        "PageDown" => "Pg ↓",
        "ArrowUp" => "↑",
        "ArrowDown" => "↓",
        "ArrowLeft" => "←",
        "ArrowRight" => "→",
        "NumLock" => "Verr Num",
        "NumpadDivide" => "/",
        "NumpadMultiply" => "*",
        "NumpadSubtract" => "-",
        "NumpadAdd" => "+",
        "NumpadEnter" => "Entrée",
        "NumpadDecimal" => ".",
        "Numpad0" => "0",
        "Numpad1" => "1",
        "Numpad2" => "2",
        "Numpad3" => "3",
        "Numpad4" => "4",
        "Numpad5" => "5",
        "Numpad6" => "6",
        "Numpad7" => "7",
        "Numpad8" => "8",
        "Numpad9" => "9",
        "MediaStop" => "Stop",
        "MediaTrackPrevious" => "Préc.",
        "MediaPlayPause" => "Lecture",
        "MediaTrackNext" => "Suiv.",
        "AudioVolumeMute" => "Muet",
        "AudioVolumeUp" => "Vol +",
        "AudioVolumeDown" => "Vol -",
        _ => "?",
    }
}

/// Geometry used for a physical standard. JIS and ABNT add keys this generator does not model
/// yet; they fall back to the ISO block, which shares their Enter / left-Shift shape.
fn is_ansi(std: KeyboardLayout) -> bool {
    matches!(std, KeyboardLayout::Ansi)
}

struct Builder {
    keys: Vec<PhysicalKey>,
}

impl Builder {
    /// Lay `items` left to right from `x`; an empty code is a gap of that width.
    fn row(&mut self, x: f32, y: f32, items: &[(&'static str, f32)]) -> f32 {
        let mut cx = x;
        for &(code, w) in items {
            if !code.is_empty() {
                self.keys.push(PhysicalKey {
                    code,
                    x: cx,
                    y,
                    w,
                    h: 1.0,
                });
            }
            cx += w;
        }
        cx
    }

    fn key(&mut self, code: &'static str, x: f32, y: f32, w: f32, h: f32) {
        self.keys.push(PhysicalKey { code, x, y, w, h });
    }

    /// Alphanumeric block (5 rows, 15 units wide) starting at `y`.
    /// `top_left` is `Backquote` on full boards and `Escape` on 60 / 65 % boards.
    /// `right_shift` / `bottom` let compact boards make room for arrows.
    fn main_block(
        &mut self,
        y: f32,
        std: KeyboardLayout,
        top_left: &'static str,
        right_shift: f32,
        bottom: &[(&'static str, f32)],
    ) {
        self.row(
            0.0,
            y,
            &[
                (top_left, 1.0),
                ("Digit1", 1.0),
                ("Digit2", 1.0),
                ("Digit3", 1.0),
                ("Digit4", 1.0),
                ("Digit5", 1.0),
                ("Digit6", 1.0),
                ("Digit7", 1.0),
                ("Digit8", 1.0),
                ("Digit9", 1.0),
                ("Digit0", 1.0),
                ("Minus", 1.0),
                ("Equal", 1.0),
                ("Backspace", 2.0),
            ],
        );
        const QWERTY_ROW: [(&str, f32); 12] = [
            ("KeyQ", 1.0),
            ("KeyW", 1.0),
            ("KeyE", 1.0),
            ("KeyR", 1.0),
            ("KeyT", 1.0),
            ("KeyY", 1.0),
            ("KeyU", 1.0),
            ("KeyI", 1.0),
            ("KeyO", 1.0),
            ("KeyP", 1.0),
            ("BracketLeft", 1.0),
            ("BracketRight", 1.0),
        ];
        let x = self.row(0.0, y + 1.0, &[("Tab", 1.5)]);
        let x = self.row(x, y + 1.0, &QWERTY_ROW);
        if is_ansi(std) {
            self.row(x, y + 1.0, &[("Backslash", 1.5)]);
        } else {
            // ISO Enter spans two rows.
            self.key("Enter", 13.75, y + 1.0, 1.25, 2.0);
        }
        let x = self.row(
            0.0,
            y + 2.0,
            &[
                ("CapsLock", 1.75),
                ("KeyA", 1.0),
                ("KeyS", 1.0),
                ("KeyD", 1.0),
                ("KeyF", 1.0),
                ("KeyG", 1.0),
                ("KeyH", 1.0),
                ("KeyJ", 1.0),
                ("KeyK", 1.0),
                ("KeyL", 1.0),
                ("Semicolon", 1.0),
                ("Quote", 1.0),
            ],
        );
        if is_ansi(std) {
            self.row(x, y + 2.0, &[("Enter", 2.25)]);
        } else {
            self.row(x, y + 2.0, &[("Backslash", 1.0)]);
        }
        let x = if is_ansi(std) {
            self.row(0.0, y + 3.0, &[("ShiftLeft", 2.25)])
        } else {
            self.row(0.0, y + 3.0, &[("ShiftLeft", 1.25), ("IntlBackslash", 1.0)])
        };
        self.row(
            x,
            y + 3.0,
            &[
                ("KeyZ", 1.0),
                ("KeyX", 1.0),
                ("KeyC", 1.0),
                ("KeyV", 1.0),
                ("KeyB", 1.0),
                ("KeyN", 1.0),
                ("KeyM", 1.0),
                ("Comma", 1.0),
                ("Period", 1.0),
                ("Slash", 1.0),
                ("ShiftRight", right_shift),
            ],
        );
        self.row(0.0, y + 4.0, bottom);
    }
}

const BOTTOM_FULL: [(&str, f32); 8] = [
    ("ControlLeft", 1.25),
    ("MetaLeft", 1.25),
    ("AltLeft", 1.25),
    ("Space", 6.25),
    ("AltRight", 1.25),
    ("MetaRight", 1.25),
    ("ContextMenu", 1.25),
    ("ControlRight", 1.25),
];

const BOTTOM_COMPACT: [(&str, f32); 7] = [
    ("ControlLeft", 1.25),
    ("MetaLeft", 1.25),
    ("AltLeft", 1.25),
    ("Space", 6.25),
    ("AltRight", 1.0),
    ("ContextMenu", 1.0),
    ("ControlRight", 1.0),
];

/// Generic geometry for a form factor and physical standard, plus its `(width, height)`.
pub fn physical_layout(form: FormFactor, std: KeyboardLayout) -> (Vec<PhysicalKey>, f32, f32) {
    let mut b = Builder { keys: Vec::new() };
    let (w, h) = match form {
        FormFactor::Full | FormFactor::Tkl => {
            b.row(
                0.0,
                0.0,
                &[
                    ("Escape", 1.0),
                    ("", 1.0),
                    ("F1", 1.0),
                    ("F2", 1.0),
                    ("F3", 1.0),
                    ("F4", 1.0),
                    ("", 0.5),
                    ("F5", 1.0),
                    ("F6", 1.0),
                    ("F7", 1.0),
                    ("F8", 1.0),
                    ("", 0.5),
                    ("F9", 1.0),
                    ("F10", 1.0),
                    ("F11", 1.0),
                    ("F12", 1.0),
                    ("", 0.25),
                    ("PrintScreen", 1.0),
                    ("ScrollLock", 1.0),
                    ("Pause", 1.0),
                ],
            );
            b.main_block(1.25, std, "Backquote", 2.75, &BOTTOM_FULL);
            b.row(
                15.25,
                1.25,
                &[("Insert", 1.0), ("Home", 1.0), ("PageUp", 1.0)],
            );
            b.row(
                15.25,
                2.25,
                &[("Delete", 1.0), ("End", 1.0), ("PageDown", 1.0)],
            );
            b.key("ArrowUp", 16.25, 4.25, 1.0, 1.0);
            b.row(
                15.25,
                5.25,
                &[("ArrowLeft", 1.0), ("ArrowDown", 1.0), ("ArrowRight", 1.0)],
            );
            if form == FormFactor::Full {
                b.row(
                    18.5,
                    1.25,
                    &[
                        ("NumLock", 1.0),
                        ("NumpadDivide", 1.0),
                        ("NumpadMultiply", 1.0),
                        ("NumpadSubtract", 1.0),
                    ],
                );
                b.row(
                    18.5,
                    2.25,
                    &[("Numpad7", 1.0), ("Numpad8", 1.0), ("Numpad9", 1.0)],
                );
                b.key("NumpadAdd", 21.5, 2.25, 1.0, 2.0);
                b.row(
                    18.5,
                    3.25,
                    &[("Numpad4", 1.0), ("Numpad5", 1.0), ("Numpad6", 1.0)],
                );
                b.row(
                    18.5,
                    4.25,
                    &[("Numpad1", 1.0), ("Numpad2", 1.0), ("Numpad3", 1.0)],
                );
                b.key("NumpadEnter", 21.5, 4.25, 1.0, 2.0);
                b.row(18.5, 5.25, &[("Numpad0", 2.0), ("NumpadDecimal", 1.0)]);
                (22.5, 6.25)
            } else {
                (18.25, 6.25)
            }
        }
        FormFactor::Compact75 => {
            b.row(
                0.0,
                0.0,
                &[
                    ("Escape", 1.0),
                    ("", 0.25),
                    ("F1", 1.0),
                    ("F2", 1.0),
                    ("F3", 1.0),
                    ("F4", 1.0),
                    ("", 0.25),
                    ("F5", 1.0),
                    ("F6", 1.0),
                    ("F7", 1.0),
                    ("F8", 1.0),
                    ("", 0.25),
                    ("F9", 1.0),
                    ("F10", 1.0),
                    ("F11", 1.0),
                    ("F12", 1.0),
                    ("", 0.25),
                    ("PrintScreen", 1.0),
                    ("Delete", 1.0),
                ],
            );
            compact_with_arrows(&mut b, 1.0, std, "Backquote", "Home");
            (16.0, 6.0)
        }
        FormFactor::Compact65 => {
            compact_with_arrows(&mut b, 0.0, std, "Escape", "Delete");
            (16.0, 5.0)
        }
        FormFactor::Compact60 => {
            b.main_block(0.0, std, "Escape", 2.75, &BOTTOM_FULL);
            (15.0, 5.0)
        }
    };
    (b.keys, w, h)
}

/// 65 / 75 % block: alphanumerics, a right-hand navigation column and inverted-T arrows.
fn compact_with_arrows(
    b: &mut Builder,
    y: f32,
    std: KeyboardLayout,
    top_left: &'static str,
    first_nav: &'static str,
) {
    b.main_block(y, std, top_left, 1.75, &BOTTOM_COMPACT);
    b.key(first_nav, 15.0, y, 1.0, 1.0);
    b.key("PageUp", 15.0, y + 1.0, 1.0, 1.0);
    b.key("PageDown", 15.0, y + 2.0, 1.0, 1.0);
    b.key("ArrowUp", 14.0, y + 3.0, 1.0, 1.0);
    b.key("End", 15.0, y + 3.0, 1.0, 1.0);
    b.row(
        13.0,
        y + 4.0,
        &[("ArrowLeft", 1.0), ("ArrowDown", 1.0), ("ArrowRight", 1.0)],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn overlaps(a: &PhysicalKey, b: &PhysicalKey) -> bool {
        const E: f32 = 1e-3;
        a.x < b.x + b.w - E && b.x < a.x + a.w - E && a.y < b.y + b.h - E && b.y < a.y + a.h - E
    }

    #[test]
    fn every_layout_is_consistent() {
        for form in FormFactor::ALL {
            for std in [KeyboardLayout::Ansi, KeyboardLayout::Iso] {
                let (keys, w, h) = physical_layout(form, std);
                let mut codes = HashSet::new();
                for (i, k) in keys.iter().enumerate() {
                    assert!(
                        codes.insert(k.code),
                        "{form:?}/{std:?}: {} en double",
                        k.code
                    );
                    assert_ne!(default_label(k.code), "?", "libellé manquant : {}", k.code);
                    assert!(k.x >= 0.0 && k.y >= 0.0, "{} hors cadre", k.code);
                    assert!(
                        k.x + k.w <= w + 1e-3 && k.y + k.h <= h + 1e-3,
                        "{form:?}/{std:?}: {} déborde",
                        k.code
                    );
                    for other in &keys[i + 1..] {
                        assert!(
                            !overlaps(k, other),
                            "{form:?}/{std:?}: {} chevauche {}",
                            k.code,
                            other.code
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn key_counts_match_standard_boards() {
        let count = |f, s| physical_layout(f, s).0.len();
        assert_eq!(count(FormFactor::Full, KeyboardLayout::Ansi), 104);
        assert_eq!(count(FormFactor::Full, KeyboardLayout::Iso), 105);
        assert_eq!(count(FormFactor::Tkl, KeyboardLayout::Ansi), 87);
        assert_eq!(count(FormFactor::Tkl, KeyboardLayout::Iso), 88);
        assert_eq!(count(FormFactor::Compact60, KeyboardLayout::Ansi), 61);
        assert_eq!(count(FormFactor::Compact60, KeyboardLayout::Iso), 62);
    }

    #[test]
    fn iso_has_the_extra_key_and_ansi_does_not() {
        let has = |s, code| {
            physical_layout(FormFactor::Tkl, s)
                .0
                .iter()
                .any(|k| k.code == code)
        };
        assert!(has(KeyboardLayout::Iso, "IntlBackslash"));
        assert!(!has(KeyboardLayout::Ansi, "IntlBackslash"));
        assert_eq!(char_key_scan("KeyQ"), Some(0x10));
        assert_eq!(char_key_scan("Escape"), None);
        assert_eq!(FormFactor::from_id("tkl"), Some(FormFactor::Tkl));
    }
}
