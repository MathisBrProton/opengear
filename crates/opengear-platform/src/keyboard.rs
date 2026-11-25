//! Active Windows keyboard layout: what each physical key types (VK code + label).
//!
//! Works for any layout Windows knows (AZERTY, QWERTZ, UK, Nordic, BÉPO, Cyrillic…): the label
//! of a key is asked to Windows from its scan code instead of being hard-coded.

use opengear_core::KeyboardLayout;

/// Summary of the keyboard layout currently used by the foreground application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsKeyboardLayout {
    /// Windows language identifier (low word of the HKL), e.g. `0x040C` for French.
    pub lang_id: u16,
    /// First six letters of the top letter row, e.g. `AZERTY`, `QWERTZ`, `QWERTY`.
    pub top_row: String,
    /// Physical standard most keyboards sold for this language use.
    pub suggested_standard: KeyboardLayout,
}

/// Physical standard usually paired with a Windows language.
pub fn standard_for_lang(lang_id: u16) -> KeyboardLayout {
    match lang_id {
        // US / Canadian English, Chinese, Korean, Thai, Hindi: ANSI boards.
        0x0409 | 0x1009 | 0x0404 | 0x0804 | 0x0C04 | 0x1004 | 0x0412 | 0x041E | 0x0439 => {
            KeyboardLayout::Ansi
        }
        0x0411 => KeyboardLayout::Jis,
        0x0416 => KeyboardLayout::Abnt,
        _ => KeyboardLayout::Iso,
    }
}

/// Compose a short keycap label from what a key types without and with Shift.
pub fn compose_label(plain: &str, shifted: &str) -> Option<String> {
    let clean = |s: &str| -> String { s.chars().filter(|c| !c.is_control()).collect() };
    let plain = clean(plain);
    let shifted = clean(shifted);
    if plain.is_empty() && shifted.is_empty() {
        return None;
    }
    // A letter key shows its capital, unless Shift types something else (AZERTY `ù` / `%`).
    if plain.chars().count() == 1
        && plain.chars().all(char::is_alphabetic)
        && (shifted.is_empty() || shifted.to_lowercase() == plain)
    {
        return Some(plain.to_uppercase());
    }
    let digit_first = shifted.chars().all(|c| c.is_ascii_digit()) && !shifted.is_empty();
    let (main, other) = if digit_first {
        (shifted, plain)
    } else {
        (plain, shifted)
    };
    if main.is_empty() {
        return Some(other);
    }
    if other.is_empty() || other.to_lowercase() == main.to_lowercase() {
        Some(main)
    } else {
        Some(format!("{main} {other}"))
    }
}

/// Resolves scan codes against one Windows keyboard layout.
pub struct KeyResolver {
    #[cfg(windows)]
    hkl: windows_sys::Win32::UI::Input::KeyboardAndMouse::HKL,
}

impl KeyResolver {
    /// Layout of the foreground window (the one the user types in), else of this thread.
    #[cfg(windows)]
    pub fn for_foreground() -> Self {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        unsafe {
            let hwnd = GetForegroundWindow();
            let tid = if hwnd.is_null() {
                0
            } else {
                GetWindowThreadProcessId(hwnd, std::ptr::null_mut())
            };
            let mut hkl = GetKeyboardLayout(tid);
            if hkl.is_null() {
                hkl = GetKeyboardLayout(0);
            }
            Self { hkl }
        }
    }

    #[cfg(not(windows))]
    pub fn for_foreground() -> Self {
        Self {}
    }

    pub fn lang_id(&self) -> u16 {
        #[cfg(windows)]
        {
            (self.hkl as usize & 0xFFFF) as u16
        }
        #[cfg(not(windows))]
        {
            0x0409
        }
    }

    /// `(virtual-key code, keycap label)` produced by the key with this set-1 scan code.
    #[cfg(windows)]
    pub fn resolve(&self, scan: u16) -> Option<(u16, String)> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            MAPVK_VSC_TO_VK_EX, MapVirtualKeyExW, ToUnicodeEx,
        };
        // Bit 2: do not change the keyboard state (keeps pending dead keys intact, Win10 1607+).
        const NO_STATE_CHANGE: u32 = 0x4;
        const VK_SHIFT: usize = 0x10;
        unsafe {
            let vk = MapVirtualKeyExW(scan as u32, MAPVK_VSC_TO_VK_EX, self.hkl);
            if vk == 0 {
                return None;
            }
            let type_with = |state: &[u8; 256]| -> String {
                let mut buf = [0u16; 8];
                let n = ToUnicodeEx(
                    vk,
                    scan as u32,
                    state.as_ptr(),
                    buf.as_mut_ptr(),
                    buf.len() as i32,
                    NO_STATE_CHANGE,
                    self.hkl,
                );
                // n < 0: dead key, the buffer still holds its character.
                let len = if n < 0 { 1 } else { n as usize };
                String::from_utf16_lossy(&buf[..len.min(buf.len())])
            };
            let plain = type_with(&[0u8; 256]);
            let mut shift_state = [0u8; 256];
            shift_state[VK_SHIFT] = 0x80;
            let shifted = type_with(&shift_state);
            compose_label(&plain, &shifted).map(|label| (vk as u16, label))
        }
    }

    #[cfg(not(windows))]
    pub fn resolve(&self, _scan: u16) -> Option<(u16, String)> {
        None
    }

    /// Summary of this layout (language, top letter row, usual physical standard).
    pub fn describe(&self) -> OsKeyboardLayout {
        let top_row: String = [0x10u16, 0x11, 0x12, 0x13, 0x14, 0x15]
            .iter()
            .filter_map(|&scan| self.resolve(scan))
            .map(|(_, label)| label.chars().next().unwrap_or('?'))
            .collect();
        let lang_id = self.lang_id();
        OsKeyboardLayout {
            lang_id,
            top_row,
            suggested_standard: standard_for_lang(lang_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_like_keycaps() {
        assert_eq!(compose_label("a", "A").as_deref(), Some("A"));
        assert_eq!(compose_label("&", "1").as_deref(), Some("1 &"));
        assert_eq!(compose_label("1", "!").as_deref(), Some("1 !"));
        assert_eq!(compose_label("ù", "%").as_deref(), Some("ù %"));
        assert_eq!(compose_label("\u{1b}", "\u{1b}"), None);
        assert_eq!(compose_label("é", "2").as_deref(), Some("2 é"));
    }

    #[test]
    fn standard_guess_by_language() {
        assert_eq!(standard_for_lang(0x040C), KeyboardLayout::Iso);
        assert_eq!(standard_for_lang(0x0409), KeyboardLayout::Ansi);
        assert_eq!(standard_for_lang(0x0411), KeyboardLayout::Jis);
    }

    /// Prints the layout of this machine (`cargo test -p opengear-platform -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn print_active_layout() {
        let r = KeyResolver::for_foreground();
        println!("{:?}", r.describe());
        for scan in [0x02u16, 0x10, 0x1E, 0x27, 0x2C, 0x56] {
            println!("scan {scan:#04x} -> {:?}", r.resolve(scan));
        }
    }
}
