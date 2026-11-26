use crate::features::FEAT_ONBOARD_PROFILES;
use crate::hidpp::{HidppError, HidppTransport};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Standard CRC-16/CCITT-FALSE used by Logitech HID++ 2.0 `0x8100` onboard flash sectors.
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardMemoryDesc {
    pub memory_model_id: u8,
    pub profile_format_id: u8,
    pub macro_format_id: u8,
    pub profile_count: u8,
    pub rom_profile_count: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
    pub has_g_shift: bool,
    pub has_dpi_shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButtonAction {
    /// Standard HID mouse button (1=Left, 2=Right, 3=Middle, 4=Back, 5=Forward, ...)
    MouseButton(u8),
    /// HID keyboard shortcut (USB HID modifier mask + USB HID usage keycode)
    KeyboardKey { modifiers: u8, hid_key: u8 },
    /// HID Consumer Control media key (e.g. 0x00CD Play/Pause, 0x00E9 Vol+, 0x00EA Vol-, 0x00E2 Mute)
    ConsumerControl(u16),
    /// Logitech special action (0x90): DPI+/DPI-/Cycle/Shift/Profile/G-Shift/Battery
    Special(u8),
    /// Onboard hardware macro pointer in flash (`[0x00, sector_lo, offset_hi, offset_lo]`)
    OnboardMacro { sector: u8, offset: u16 },
    /// Button disabled (0xFF)
    Disabled,
    /// Unmodified raw 4-byte binding (preserves macros or vendor-specific bindings)
    Raw([u8; 4]),
}

/// A single step in a Logitech HID++ 2.0 `0x8100` onboard hardware macro.
pub use opengear_core::onboard::OnboardMacroStep;

/// Encode a single portable macro step into HID++ 2.0 `0x8100` macro bytecode
/// (verified from libratbag `hidpp20.h`).
pub(crate) fn encode_macro_step(step: OnboardMacroStep, buf: &mut Vec<u8>) {
    match step {
        OnboardMacroStep::KeyPress(hid_usage) => {
            buf.push(0x20);
            buf.push(hid_usage);
        }
        OnboardMacroStep::KeyRelease(hid_usage) => {
            buf.push(0x21);
            buf.push(hid_usage);
        }
        OnboardMacroStep::MousePress(btn_1based) => {
            let mask = 1u16 << (btn_1based.clamp(1, 16) - 1);
            let b = mask.to_be_bytes();
            buf.push(0x22);
            buf.push(b[0]);
            buf.push(b[1]);
        }
        OnboardMacroStep::MouseRelease(btn_1based) => {
            let mask = 1u16 << (btn_1based.clamp(1, 16) - 1);
            let b = mask.to_be_bytes();
            buf.push(0x23);
            buf.push(b[0]);
            buf.push(b[1]);
        }
        OnboardMacroStep::DelayMs(ms) => {
            let b = ms.clamp(1, 30000).to_be_bytes();
            buf.push(0x40);
            buf.push(b[0]);
            buf.push(b[1]);
        }
    }
}

impl MouseButtonAction {
    pub fn referenced_macro_sector(self) -> Option<u16> {
        match self {
            Self::OnboardMacro { sector, .. } => Some(sector as u16),
            // Repeat (0x01) / Toggle (0x02) onboard macros use the same sector byte at [1].
            Self::Raw([0x00..=0x02, sector, _, _]) if sector > 0 && sector < 0x80 => {
                Some(sector as u16)
            }
            _ => None,
        }
    }

    pub fn decode(raw: [u8; 4]) -> Self {
        match raw[0] {
            0x80 => match raw[1] {
                0x01 => {
                    let mask = u16::from_be_bytes([raw[2], raw[3]]);
                    if mask == 0 {
                        Self::Disabled
                    } else {
                        let btn = (mask.trailing_zeros() + 1) as u8;
                        Self::MouseButton(btn)
                    }
                }
                0x02 => Self::KeyboardKey {
                    modifiers: raw[2],
                    hid_key: raw[3],
                },
                0x03 => Self::ConsumerControl(u16::from_be_bytes([raw[2], raw[3]])),
                _ => Self::Raw(raw),
            },
            0x90 if raw[2] == 0 && raw[3] == 0 => Self::Special(raw[1]),
            0x90 => Self::Raw(raw),
            0x00 if raw[1] > 0 && raw[1] < 0x80 => Self::OnboardMacro {
                sector: raw[1],
                offset: u16::from_be_bytes([raw[2], raw[3]]),
            },
            0xFF => Self::Disabled,
            _ => Self::Raw(raw),
        }
    }

    pub fn encode(self) -> [u8; 4] {
        match self {
            Self::MouseButton(btn) => {
                let b = btn.clamp(1, 16);
                let mask = 1u16 << (b - 1);
                let bytes = mask.to_be_bytes();
                [0x80, 0x01, bytes[0], bytes[1]]
            }
            Self::KeyboardKey { modifiers, hid_key } => [0x80, 0x02, modifiers, hid_key],
            Self::ConsumerControl(code) => {
                let bytes = code.to_be_bytes();
                [0x80, 0x03, bytes[0], bytes[1]]
            }
            Self::Special(code) => [0x90, code, 0x00, 0x00],
            Self::OnboardMacro { sector, offset } => {
                let b = offset.to_be_bytes();
                [0x00, sector, b[0], b[1]]
            }
            Self::Disabled => [0xFF, 0xFF, 0xFF, 0xFF],
            Self::Raw(r) => r,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::MouseButton(1) => "Clic Gauche".into(),
            Self::MouseButton(2) => "Clic Droit".into(),
            Self::MouseButton(3) => "Clic Molette".into(),
            Self::MouseButton(4) => "Précédent (M4)".into(),
            Self::MouseButton(5) => "Suivant (M5)".into(),
            Self::MouseButton(n) => format!("Bouton Souris {n}"),
            Self::KeyboardKey { modifiers, hid_key } => {
                let mut parts = Vec::new();
                if (modifiers & 0x01) != 0 {
                    parts.push("Ctrl");
                }
                if (modifiers & 0x02) != 0 {
                    parts.push("Shift");
                }
                if (modifiers & 0x04) != 0 {
                    parts.push("Alt");
                }
                if (modifiers & 0x08) != 0 {
                    parts.push("Win");
                }
                let kname = hid_usage_name(hid_key);
                if parts.is_empty() {
                    format!("Touche {kname}")
                } else {
                    format!("{}+{kname}", parts.join("+"))
                }
            }
            Self::ConsumerControl(0x00CD) => "Média : Lecture / Pause".into(),
            Self::ConsumerControl(0x00E9) => "Média : Volume +".into(),
            Self::ConsumerControl(0x00EA) => "Média : Volume -".into(),
            Self::ConsumerControl(0x00E2) => "Média : Muet".into(),
            Self::ConsumerControl(0x00B5) => "Média : Piste suivante".into(),
            Self::ConsumerControl(0x00B6) => "Média : Piste précédente".into(),
            Self::ConsumerControl(c) => format!("Média 0x{c:04X}"),
            Self::Special(0x03) => "DPI Suivant (DPI +)".into(),
            Self::Special(0x04) => "DPI Précédent (DPI -)".into(),
            Self::Special(0x05) => "Cycle DPI".into(),
            Self::Special(0x06) => "DPI par défaut".into(),
            Self::Special(0x07) => "DPI Shift (Sniper)".into(),
            Self::Special(0x08) => "Profil Suivant".into(),
            Self::Special(0x09) => "Profil Précédent".into(),
            Self::Special(0x0A) => "Cycle Profil".into(),
            Self::Special(0x0B) => "G-Shift".into(),
            Self::Special(0x0C) => "Niveau Batterie".into(),
            Self::Special(c) => format!("Spécial Logitech (0x{c:02X})"),
            Self::OnboardMacro { sector, offset } => {
                format!("Macro Flash (sec 0x{sector:02X}+{offset})")
            }
            Self::Disabled => "Désactivé".into(),
            Self::Raw(r) => format!(
                "Macro / Brut {:02X}{:02X}{:02X}{:02X}",
                r[0], r[1], r[2], r[3]
            ),
        }
    }

    /// Common G HUB quick-assignable actions for dropdown selectors.
    pub const PRESET_ACTIONS: &'static [(&'static str, MouseButtonAction)] = &[
        ("Clic Gauche", Self::MouseButton(1)),
        ("Clic Droit", Self::MouseButton(2)),
        ("Clic Molette", Self::MouseButton(3)),
        ("Précédent (M4)", Self::MouseButton(4)),
        ("Suivant (M5)", Self::MouseButton(5)),
        ("Cycle DPI", Self::Special(0x05)),
        ("DPI Suivant (+)", Self::Special(0x03)),
        ("DPI Précédent (-)", Self::Special(0x04)),
        ("DPI Shift (Sniper)", Self::Special(0x07)),
        ("Cycle Profil", Self::Special(0x0A)),
        ("G-Shift", Self::Special(0x0B)),
        ("Média : Lecture / Pause", Self::ConsumerControl(0x00CD)),
        ("Média : Volume +", Self::ConsumerControl(0x00E9)),
        ("Média : Volume -", Self::ConsumerControl(0x00EA)),
        ("Média : Muet", Self::ConsumerControl(0x00E2)),
        (
            "Touche : Espace",
            Self::KeyboardKey {
                modifiers: 0,
                hid_key: 0x2C,
            },
        ),
        (
            "Touche : F13 (Push-to-Talk)",
            Self::KeyboardKey {
                modifiers: 0,
                hid_key: 0x68,
            },
        ),
        (
            "Touche : F14",
            Self::KeyboardKey {
                modifiers: 0,
                hid_key: 0x69,
            },
        ),
        (
            "Touche : Ctrl+C",
            Self::KeyboardKey {
                modifiers: 0x01,
                hid_key: 0x06,
            },
        ),
        (
            "Touche : Ctrl+V",
            Self::KeyboardKey {
                modifiers: 0x01,
                hid_key: 0x19,
            },
        ),
        ("Désactivé", Self::Disabled),
    ];
}

fn hid_usage_name(code: u8) -> String {
    match code {
        0x04..=0x1D => {
            let ch = (b'A' + (code - 0x04)) as char;
            ch.to_string()
        }
        0x1E..=0x26 => format!("{}", code - 0x1D),
        0x27 => "0".into(),
        0x28 => "Entrée".into(),
        0x29 => "Échap".into(),
        0x2B => "Tab".into(),
        0x2C => "Espace".into(),
        0x3A..=0x45 => format!("F{}", code - 0x39),
        0x68..=0x73 => format!("F{}", code - 0x68 + 13),
        _ => format!("0x{code:02X}"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardProfile {
    /// 1-based slot number (1..=5)
    pub slot_index: u8,
    pub sector_addr: u16,
    pub from_rom: bool,
    pub enabled: bool,
    pub crc_valid: bool,
    pub name: String,
    pub report_rate_hz: u16,
    pub default_dpi_idx: u8,
    pub switched_dpi_idx: u8,
    pub dpi_stages: [u16; 5],
    pub angle_snapping: bool,
    pub buttons: Vec<MouseButtonAction>,
    pub gshift_buttons: Vec<MouseButtonAction>,
    #[serde(skip)]
    pub raw_sector: Vec<u8>,
}

impl OnboardProfile {
    /// Read onboard memory descriptor (`0x8100` fn `0x00`).
    pub fn read_desc(t: &HidppTransport) -> Result<OnboardMemoryDesc, HidppError> {
        let d = t.request_feature(FEAT_ONBOARD_PROFILES, 0x00, &[])?;
        if d.len() < 11 {
            return Err(HidppError::Timeout);
        }
        let raw_sector_size = u16::from_be_bytes([d[7], d[8]]);
        // Note: G PRO X Superlight reports sector_size = 255 (0x00FF) — MUST NOT round up to 256!
        let sector_size = if raw_sector_size < 64 {
            256
        } else {
            raw_sector_size
        };
        let mech = d[9];
        Ok(OnboardMemoryDesc {
            memory_model_id: d[0],
            profile_format_id: d[1],
            macro_format_id: d[2],
            profile_count: d[3],
            rom_profile_count: d[4],
            button_count: d[5].min(16),
            sector_count: d[6],
            sector_size,
            has_g_shift: (mech & 0x03) == 0x02,
            has_dpi_shift: ((mech & 0x0C) >> 2) == 0x02,
        })
    }

    /// Read a full flash or ROM sector from the mouse (`0x8100` fn `0x50`).
    pub fn read_sector(
        t: &HidppTransport,
        sector: u16,
        sector_size: u16,
    ) -> Result<Vec<u8>, HidppError> {
        let sz = sector_size as usize;
        if sz < 16 {
            return Err(HidppError::Flash(format!(
                "taille de secteur invalide ({sz} octets < 16)"
            )));
        }
        let mut data = vec![0u8; sz];
        let sec_be = sector.to_be_bytes();
        let mut offset = 0usize;
        while offset < sz {
            let read_off = if sz - offset < 16 { sz - 16 } else { offset };
            let off_be = (read_off as u16).to_be_bytes();
            let chunk = t.request_feature(
                FEAT_ONBOARD_PROFILES,
                0x50,
                &[sec_be[0], sec_be[1], off_be[0], off_be[1]],
            )?;
            if chunk.len() < 16 {
                return Err(HidppError::Timeout);
            }
            data[read_off..read_off + 16].copy_from_slice(&chunk[..16]);
            if read_off + 16 >= sz {
                break;
            }
            offset += 16;
        }
        Ok(data)
    }

    /// Write a full sector with CRC-16 CCITT to onboard flash (`0x60` -> `0x70` x N -> `0x80`).
    /// Always issues `0x80` (`MEMORY_WRITE_END`) if `0x70` fails mid-sector so flash state stays clean.
    pub fn write_sector(
        t: &HidppTransport,
        sector: u16,
        data: &mut [u8],
    ) -> Result<(), HidppError> {
        let sz = data.len();
        if sz < 16 {
            return Err(HidppError::Protocol(0x02, "Secteur trop petit"));
        }
        let crc = crc16_ccitt(&data[..sz - 2]);
        let crc_be = crc.to_be_bytes();
        data[sz - 2] = crc_be[0];
        data[sz - 1] = crc_be[1];

        let sec_be = sector.to_be_bytes();
        let cnt_be = (sz as u16).to_be_bytes();
        t.request_feature(
            FEAT_ONBOARD_PROFILES,
            0x60,
            &[sec_be[0], sec_be[1], 0x00, 0x00, cnt_be[0], cnt_be[1]],
        )?;

        for chunk in data.chunks(16) {
            let mut pkt = [0xFFu8; 16];
            pkt[..chunk.len()].copy_from_slice(chunk);
            if let Err(e) = t.request_feature(FEAT_ONBOARD_PROFILES, 0x70, &pkt) {
                let _ = t.request_feature(FEAT_ONBOARD_PROFILES, 0x80, &[]);
                return Err(e);
            }
        }

        t.request_feature(FEAT_ONBOARD_PROFILES, 0x80, &[])?;
        Ok(())
    }

    /// Read directory sector `0x0000` and all onboard profiles (falling back to ROM `0x0101+` if empty).
    pub fn read_all(
        t: &HidppTransport,
    ) -> Result<(OnboardMemoryDesc, Vec<OnboardProfile>), HidppError> {
        let desc = Self::read_desc(t)?;
        let dir_raw = Self::read_sector(t, 0x0000, desc.sector_size).ok();
        let dir_crc_ok = dir_raw
            .as_ref()
            .map(|d| Self::verify_crc(d))
            .unwrap_or(false);

        let mut dir_entries: Vec<(u16, bool)> = Vec::new();
        if dir_crc_ok && let Some(ref d) = dir_raw {
            for i in 0..desc.profile_count as usize {
                let base = i * 4;
                if base + 3 >= d.len() {
                    break;
                }
                let addr = u16::from_be_bytes([d[base], d[base + 1]]);
                if addr == 0xFFFF {
                    break;
                }
                let enabled = d[base + 2] != 0;
                dir_entries.push((addr, enabled));
            }
        }
        debug!(
            "0x8100 desc: {} profils, {} boutons, sector_size={}, dir_crc_ok={}, entries={:04X?}",
            desc.profile_count, desc.button_count, desc.sector_size, dir_crc_ok, dir_entries
        );

        let mut profiles = Vec::new();
        for i in 0..desc.profile_count as usize {
            let slot = (i + 1) as u8;
            let (addr, enabled) = dir_entries.get(i).copied().unwrap_or((slot as u16, i == 0));

            let mut parsed: Option<OnboardProfile> = None;
            if dir_crc_ok
                && addr != 0
                && addr < 0x0100
                && let Ok(raw) = Self::read_sector(t, addr, desc.sector_size)
                && Self::verify_crc(&raw)
            {
                parsed = Some(Self::parse_sector(
                    slot,
                    addr,
                    false,
                    enabled,
                    true,
                    &raw,
                    desc.button_count,
                ));
            }

            if parsed.is_none() {
                // Fallback to factory ROM profile (0x0101..0x010N)
                let rom_idx = if (i as u8) < desc.rom_profile_count {
                    i as u16 + 1
                } else {
                    1
                };
                let rom_addr = 0x0100 + rom_idx;
                if let Ok(raw) = Self::read_sector(t, rom_addr, desc.sector_size) {
                    parsed = Some(Self::parse_sector(
                        slot,
                        rom_addr,
                        true,
                        enabled,
                        true,
                        &raw,
                        desc.button_count,
                    ));
                }
            }

            if let Some(p) = parsed {
                profiles.push(p);
            }
        }

        Ok((desc, profiles))
    }

    pub fn verify_crc(raw: &[u8]) -> bool {
        if raw.len() < 4 {
            return false;
        }
        let expected = crc16_ccitt(&raw[..raw.len() - 2]);
        let stored = u16::from_be_bytes([raw[raw.len() - 2], raw[raw.len() - 1]]);
        expected == stored
    }

    fn parse_sector(
        slot_index: u8,
        sector_addr: u16,
        from_rom: bool,
        enabled: bool,
        crc_valid: bool,
        raw: &[u8],
        button_count: u8,
    ) -> Self {
        let rr_ms = raw.first().copied().unwrap_or(1).max(1);
        let report_rate_hz = 1000 / rr_ms as u16;
        let default_dpi_idx = raw.get(1).copied().unwrap_or(0).min(4);
        let switched_dpi_idx = raw.get(2).copied().unwrap_or(0).min(4);

        let mut dpi_stages = [400, 800, 1600, 3200, 6400];
        for (i, stage) in dpi_stages.iter_mut().enumerate() {
            let off = 3 + i * 2;
            if off + 1 < raw.len() {
                let v = u16::from_le_bytes([raw[off], raw[off + 1]]);
                if (MIN_DPI..=MAX_DPI).contains(&v) {
                    *stage = v;
                }
            }
        }

        let angle_snapping = raw.get(17).copied().unwrap_or(0) != 0;

        let n_btn = (button_count as usize).min(16);
        let mut buttons = Vec::with_capacity(n_btn);
        let mut gshift_buttons = Vec::with_capacity(n_btn);
        for i in 0..n_btn {
            let b_off = 32 + i * 4;
            if b_off + 3 < raw.len() {
                buttons.push(MouseButtonAction::decode([
                    raw[b_off],
                    raw[b_off + 1],
                    raw[b_off + 2],
                    raw[b_off + 3],
                ]));
            }
            let g_off = 96 + i * 4;
            if g_off + 3 < raw.len() {
                gshift_buttons.push(MouseButtonAction::decode([
                    raw[g_off],
                    raw[g_off + 1],
                    raw[g_off + 2],
                    raw[g_off + 3],
                ]));
            }
        }

        let name = if raw.len() >= 208 {
            decode_profile_name(&raw[160..208], slot_index)
        } else {
            format!("Profil {slot_index}")
        };

        Self {
            slot_index,
            sector_addr,
            from_rom,
            enabled,
            crc_valid,
            name,
            report_rate_hz,
            default_dpi_idx,
            switched_dpi_idx,
            dpi_stages,
            angle_snapping,
            buttons,
            gshift_buttons,
            raw_sector: raw.to_vec(),
        }
    }

    /// Write the edited fields of this profile into `buf` (a full profile sector), leaving every
    /// other byte (LED settings, vendor metadata…) untouched.
    pub(crate) fn patch_into(&self, buf: &mut [u8]) {
        let rr_ms = match self.report_rate_hz {
            0..=180 => 8u8,
            181..=350 => 4,
            351..=750 => 2,
            _ => 1,
        };
        buf[0] = rr_ms;
        buf[1] = self.default_dpi_idx.min(4);
        buf[2] = self.switched_dpi_idx.min(4);
        for (i, &dpi) in self.dpi_stages.iter().enumerate() {
            // Same bounds as parse_sector: a 44 000 DPI stage read from the mouse is kept as-is.
            let le = dpi.clamp(MIN_DPI, MAX_DPI).to_le_bytes();
            buf[3 + i * 2] = le[0];
            buf[3 + i * 2 + 1] = le[1];
        }
        buf[17] = u8::from(self.angle_snapping);
        for (i, &act) in self.buttons.iter().take(16).enumerate() {
            let off = 32 + i * 4;
            if off + 4 <= buf.len() {
                buf[off..off + 4].copy_from_slice(&act.encode());
            }
        }
        for (i, &act) in self.gshift_buttons.iter().take(16).enumerate() {
            let off = 96 + i * 4;
            if off + 4 <= buf.len() {
                buf[off..off + 4].copy_from_slice(&act.encode());
            }
        }
    }

    /// Flash this profile to the mouse, safely:
    /// 1. the directory (sector `0x0000`) must be valid or blank — a corrupt one is never touched;
    /// 2. the target is the user sector the directory already assigns to this slot (never a
    ///    sector referenced by another profile);
    /// 3. the current target and directory contents are backed up to disk BEFORE any write
    ///    (the flash is aborted if the backup fails);
    /// 4. every write is verified byte-for-byte by reading it back; on failure the previous
    ///    content is written back automatically.
    pub fn flash_to_mouse(
        &mut self,
        t: &HidppTransport,
        desc: &OnboardMemoryDesc,
        all_enabled_flags: &[bool],
    ) -> Result<(), HidppError> {
        validate_supported_profile_format(desc)?;
        let sz = desc.sector_size as usize;
        if sz < 112 {
            return Err(HidppError::Flash(format!(
                "taille de secteur {sz} octets trop petite pour un profil"
            )));
        }
        if self.slot_index == 0 || self.slot_index > desc.profile_count {
            return Err(HidppError::Flash(format!(
                "slot de profil {} invalide",
                self.slot_index
            )));
        }

        // 1. Directory
        let dir_raw = Self::read_sector(t, 0x0000, desc.sector_size)?;
        let dir_state = DirectoryState::classify(&dir_raw);
        if dir_state == DirectoryState::Corrupt {
            return Err(HidppError::Flash(
                "répertoire des profils (secteur 0x0000) corrompu — aucune écriture effectuée. \
                 Resynchronise la souris avec G HUB avant de réessayer."
                    .into(),
            ));
        }
        let entries = if dir_state == DirectoryState::Valid {
            parse_directory(&dir_raw, desc.profile_count)
        } else {
            Vec::new()
        };

        // 2. Target sector
        let target = choose_profile_sector(self.slot_index, &entries, desc)?;

        // 3. Backups (durable, unique names) of what is on the mouse right now
        let current = Self::read_sector(t, target, desc.sector_size)?;
        let current_valid = Self::verify_crc(&current);
        let backup_path = save_sector_backup(target, &current, "profil")?;
        save_sector_backup(0x0000, &dir_raw, "repertoire")?;

        // Base content: the bytes this profile was loaded from (may be a ROM copy), else the
        // current target if it holds a valid profile. Never build a profile from blank flash.
        let base = if self.raw_sector.len() == sz {
            self.raw_sector.clone()
        } else if current_valid {
            current.clone()
        } else {
            return Err(HidppError::Flash(
                "contenu de base du profil introuvable (recharge les profils)".into(),
            ));
        };
        let mut new_sector = base;
        self.patch_into(&mut new_sector);

        // 4. Write + byte-exact verification, with automatic rollback
        if let Err(e) = write_and_verify(t, target, &mut new_sector, desc.sector_size) {
            let rollback = if current_valid {
                let mut old = current.clone();
                match write_and_verify(t, target, &mut old, desc.sector_size) {
                    Ok(()) => "ancien contenu restauré".to_string(),
                    Err(re) => format!("restauration automatique échouée : {re}"),
                }
            } else {
                "secteur précédemment vide, rien à restaurer".to_string()
            };
            return Err(HidppError::Flash(format!(
                "écriture du profil #{} (secteur 0x{target:04X}) échouée : {e} — {rollback}. Sauvegarde : {}",
                self.slot_index,
                backup_path.display()
            )));
        }

        // 5. Directory update (only if needed)
        let other_valid: Vec<bool> = if dir_state == DirectoryState::Blank {
            (1..=desc.profile_count as u16)
                .map(|s| {
                    s != target
                        && Self::read_sector(t, s, desc.sector_size)
                            .map(|raw| Self::verify_crc(&raw))
                            .unwrap_or(false)
                })
                .collect()
        } else {
            Vec::new()
        };
        if let Some(mut new_dir) = build_directory(
            &dir_raw,
            dir_state,
            desc,
            self.slot_index,
            target,
            all_enabled_flags,
            &other_valid,
        )? && let Err(e) = write_and_verify(t, 0x0000, &mut new_dir, desc.sector_size)
        {
            let dir_rb = if dir_state == DirectoryState::Valid {
                let mut old = dir_raw.clone();
                match write_and_verify(t, 0x0000, &mut old, desc.sector_size) {
                    Ok(()) => "répertoire d'origine restauré".to_string(),
                    Err(re) => format!("restauration du répertoire échouée : {re}"),
                }
            } else {
                "répertoire précédemment vide".to_string()
            };
            if current_valid {
                let mut old_prof = current.clone();
                let _ = write_and_verify(t, target, &mut old_prof, desc.sector_size);
            }
            return Err(HidppError::Flash(format!(
                "mise à jour du répertoire échouée : {e} — {dir_rb}"
            )));
        }

        self.raw_sector = new_sector;
        self.sector_addr = target;
        self.from_rom = false;
        self.enabled = true;
        self.crc_valid = true;
        info!(
            "Profil embarqué #{} flashé & vérifié octet par octet (secteur 0x{:04X}, {} Hz, DPIs={:?}, sauvegarde {})",
            self.slot_index,
            target,
            self.report_rate_hz,
            self.dpi_stages,
            backup_path.display()
        );
        Ok(())
    }

    /// Write a sector previously saved by OpenGear back to the mouse (verified byte-for-byte).
    /// Only user sectors (and the directory) are accepted, and the backup must carry a valid CRC.
    pub fn restore_sector_backup(
        t: &HidppTransport,
        desc: &OnboardMemoryDesc,
        sector: u16,
        bytes: &[u8],
    ) -> Result<(), HidppError> {
        if bytes.len() != desc.sector_size as usize {
            return Err(HidppError::Flash(format!(
                "sauvegarde de {} octets, secteur de {} octets attendu",
                bytes.len(),
                desc.sector_size
            )));
        }
        if sector != 0x0000 && !is_user_sector(sector, desc) {
            return Err(HidppError::Flash(format!(
                "secteur 0x{sector:04X} hors zone utilisateur"
            )));
        }
        if !Self::verify_crc(bytes) && !is_blank(bytes) {
            return Err(HidppError::Flash(
                "sauvegarde invalide (CRC incorrect)".into(),
            ));
        }
        let mut buf = bytes.to_vec();
        if is_blank(&buf) {
            Self::write_sector(t, sector, &mut buf)
        } else {
            write_and_verify(t, sector, &mut buf, desc.sector_size)
        }
    }

    pub fn write_macro_sector(
        t: &HidppTransport,
        desc: &OnboardMemoryDesc,
        macro_sector: u16,
        steps: &[OnboardMacroStep],
        replace_existing: bool,
    ) -> Result<MouseButtonAction, HidppError> {
        validate_supported_macro_format(desc)?;
        let sz = desc.sector_size as usize;
        if macro_sector <= desc.profile_count as u16 || macro_sector >= desc.sector_count as u16 {
            return Err(HidppError::Protocol(
                0x03,
                "Secteur macro hors de la zone libre flash",
            ));
        }
        let mut bytecode = Vec::new();
        for &st in steps {
            encode_macro_step(st, &mut bytecode);
        }
        bytecode.push(0xFF); // HIDPP20_MACRO_END

        if bytecode.len() > sz.saturating_sub(2) {
            return Err(HidppError::Protocol(
                0x03,
                "Macro trop longue pour un secteur flash",
            ));
        }

        // The chosen sector must still be free (blank) or already hold one of OpenGear's own
        // macros being replaced: never overwrite unknown data (e.g. a G HUB macro).
        let current = Self::read_sector(t, macro_sector, desc.sector_size)?;
        if !is_blank(&current) {
            if !replace_existing {
                return Err(HidppError::Flash(format!(
                    "secteur macro 0x{macro_sector:04X} déjà occupé — écriture refusée"
                )));
            }
            save_sector_backup(macro_sector, &current, "macro")?;
        }

        let mut sector_buf = vec![0xFFu8; sz];
        sector_buf[..bytecode.len()].copy_from_slice(&bytecode);
        write_and_verify(t, macro_sector, &mut sector_buf, desc.sector_size)?;
        info!(
            "Macro embarquée flashée dans secteur 0x{:04X} ({} octets, CRC OK)",
            macro_sector,
            bytecode.len()
        );
        Ok(MouseButtonAction::OnboardMacro {
            sector: (macro_sector & 0xFF) as u8,
            offset: 0,
        })
    }
}

/// Validate that `profile_format_id` matches the known Superlight / G PRO v1..v4 binary layout
/// handled by `parse_sector` / `patch_into`, refusing silent corruption on newer incompatible formats.
pub(crate) fn validate_supported_profile_format(
    desc: &OnboardMemoryDesc,
) -> Result<(), HidppError> {
    if !(1..=4).contains(&desc.profile_format_id) {
        return Err(HidppError::Flash(format!(
            "format de profil embarqué non supporté (profile_format_id={}, memory_model_id={}) — écriture refusée par sécurité",
            desc.profile_format_id, desc.memory_model_id
        )));
    }
    Ok(())
}

/// Validate that `macro_format_id` matches the supported HID++ 2.0 `0x8100` macro bytecode format.
pub(crate) fn validate_supported_macro_format(desc: &OnboardMemoryDesc) -> Result<(), HidppError> {
    if !(1..=2).contains(&desc.macro_format_id) {
        return Err(HidppError::Flash(format!(
            "format de macro embarquée non supporté (macro_format_id={}) — écriture refusée par sécurité",
            desc.macro_format_id
        )));
    }
    Ok(())
}

/// DPI bounds accepted when reading and re-writing a profile (HERO 2 sensors reach 44 000 DPI).
const MIN_DPI: u16 = 50;
const MAX_DPI: u16 = 44_000;

/// State of the onboard profile directory (sector `0x0000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectoryState {
    /// CRC-valid directory.
    Valid,
    /// Never written (erased flash): may be initialised.
    Blank,
    /// Non-blank with a bad CRC: unknown content, must not be touched.
    Corrupt,
}

impl DirectoryState {
    pub(crate) fn classify(raw: &[u8]) -> Self {
        if OnboardProfile::verify_crc(raw) {
            Self::Valid
        } else if is_blank(raw) || raw.iter().all(|&b| b == 0x00) {
            Self::Blank
        } else {
            Self::Corrupt
        }
    }
}

/// True for erased flash (every byte 0xFF).
pub(crate) fn is_blank(raw: &[u8]) -> bool {
    raw.iter().all(|&b| b == 0xFF)
}

/// True for a writable user flash sector (`0x0001..sector_count`, never ROM `0x01xx`).
pub(crate) fn is_user_sector(sector: u16, desc: &OnboardMemoryDesc) -> bool {
    (1..0x0100).contains(&sector) && sector < desc.sector_count as u16
}

/// Directory entries `(sector, enabled)` in slot order, up to the `0xFFFF` terminator.
pub(crate) fn parse_directory(raw: &[u8], profile_count: u8) -> Vec<(u16, bool)> {
    let mut out = Vec::new();
    for i in 0..profile_count as usize {
        let base = i * 4;
        if base + 3 >= raw.len().saturating_sub(2) {
            break;
        }
        let addr = u16::from_be_bytes([raw[base], raw[base + 1]]);
        if addr == 0xFFFF {
            break;
        }
        out.push((addr, raw[base + 2] != 0));
    }
    out
}

/// Pick the flash sector for profile `slot` (1-based): the user sector the directory already
/// assigns to it, else sector `slot` — but never a sector another directory entry points to.
pub(crate) fn choose_profile_sector(
    slot: u8,
    entries: &[(u16, bool)],
    desc: &OnboardMemoryDesc,
) -> Result<u16, HidppError> {
    let i = slot as usize - 1;
    if let Some(&(addr, _)) = entries.get(i)
        && is_user_sector(addr, desc)
    {
        return Ok(addr);
    }
    let candidate = slot as u16;
    if !is_user_sector(candidate, desc) {
        return Err(HidppError::Flash(format!(
            "aucun secteur utilisateur disponible pour le profil #{slot}"
        )));
    }
    if entries
        .iter()
        .enumerate()
        .any(|(j, &(addr, _))| j != i && addr == candidate)
    {
        return Err(HidppError::Flash(format!(
            "le secteur 0x{candidate:04X} appartient déjà à un autre profil — écriture refusée"
        )));
    }
    Ok(candidate)
}

/// Compute the new directory sector, or `None` if it already points `slot` at `target`, enabled.
///
/// - Valid directory: only this slot's entry changes (appended if it is the next free one).
/// - Blank directory: initialised with one entry per slot; besides `slot`, an entry is enabled
///   only if its sector holds a valid profile (`other_valid[p]`) and it was enabled before.
pub(crate) fn build_directory(
    dir_raw: &[u8],
    state: DirectoryState,
    desc: &OnboardMemoryDesc,
    slot: u8,
    target: u16,
    enabled_flags: &[bool],
    other_valid: &[bool],
) -> Result<Option<Vec<u8>>, HidppError> {
    let sz = dir_raw.len();
    let count = desc.profile_count as usize;
    let i = slot as usize - 1;
    let write_entry = |dir: &mut [u8], idx: usize, addr: u16, enabled: bool| {
        let b = addr.to_be_bytes();
        let base = idx * 4;
        dir[base] = b[0];
        dir[base + 1] = b[1];
        dir[base + 2] = u8::from(enabled);
        dir[base + 3] = 0x00;
    };
    let write_terminator = |dir: &mut [u8], idx: usize| {
        let base = idx * 4;
        dir[base] = 0xFF;
        dir[base + 1] = 0xFF;
        dir[base + 2] = 0x00;
        dir[base + 3] = 0x00;
    };
    // entries + terminator + CRC must fit
    if (count + 1) * 4 > sz.saturating_sub(2) {
        return Err(HidppError::Flash(format!(
            "répertoire trop petit ({sz} octets) pour {count} profils"
        )));
    }
    match state {
        DirectoryState::Corrupt => Err(HidppError::Flash("répertoire corrompu".into())),
        DirectoryState::Valid => {
            let entries = parse_directory(dir_raw, desc.profile_count);
            if let Some(&(addr, en)) = entries.get(i) {
                if addr == target && en {
                    return Ok(None);
                }
                let mut dir = dir_raw.to_vec();
                write_entry(&mut dir, i, target, true);
                Ok(Some(dir))
            } else if i == entries.len() {
                let mut dir = dir_raw.to_vec();
                write_entry(&mut dir, i, target, true);
                write_terminator(&mut dir, i + 1);
                Ok(Some(dir))
            } else {
                Err(HidppError::Flash(format!(
                    "le répertoire ne contient que {} profil(s) : impossible d'ajouter le #{slot} sans trou",
                    entries.len()
                )))
            }
        }
        DirectoryState::Blank => {
            let mut dir = vec![0xFFu8; sz];
            for p in 0..count {
                if p == i {
                    write_entry(&mut dir, p, target, true);
                } else {
                    let valid = other_valid.get(p).copied().unwrap_or(false);
                    let en = valid && enabled_flags.get(p).copied().unwrap_or(false);
                    write_entry(&mut dir, p, (p + 1) as u16, en);
                }
            }
            write_terminator(&mut dir, count);
            Ok(Some(dir))
        }
    }
}

/// Write a sector (CRC added in place) and verify it byte-for-byte by reading it back.
pub(crate) fn write_and_verify(
    t: &HidppTransport,
    sector: u16,
    data: &mut [u8],
    sector_size: u16,
) -> Result<(), HidppError> {
    OnboardProfile::write_sector(t, sector, data)?;
    let readback = OnboardProfile::read_sector(t, sector, sector_size)?;
    if readback.as_slice() != &*data {
        return Err(HidppError::Flash(format!(
            "relecture du secteur 0x{sector:04X} différente des données écrites"
        )));
    }
    Ok(())
}

/// One sector backup file written before a flash (see [`save_sector_backup`]).
#[derive(Debug, Clone, Serialize)]
pub struct SectorBackup {
    pub file: String,
    pub sector_hex: String,
    pub tag: String,
    pub created_ms: u64,
    pub size: u64,
}

/// Parse `sec<SSSS>_<tag>_<unix_ms>.bin` into (sector, tag, unix_ms).
pub(crate) fn parse_backup_file_name(name: &str) -> Option<(u16, String, u64)> {
    let sector = parse_backup_sector(name)?;
    let rest = name.get(8..)?.strip_suffix(".bin")?; // after "secSSSS_"
    if name.as_bytes().get(7) != Some(&b'_') {
        return None;
    }
    let (tag, ms) = rest.rsplit_once('_')?;
    let ms: u64 = ms.parse().ok()?;
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((sector, tag.to_string(), ms))
}

/// Every sector backup OpenGear has written, newest first.
pub fn list_sector_backups() -> Vec<SectorBackup> {
    let Ok(rd) = std::fs::read_dir(backup_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<SectorBackup> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let (sector, tag, ms) = parse_backup_file_name(&name)?;
            let size = e.metadata().ok()?.len();
            Some(SectorBackup {
                file: name,
                sector_hex: format!("0x{sector:04X}"),
                tag,
                created_ms: ms,
                size,
            })
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.created_ms));
    out
}

/// Resolve a backup file name (as listed by [`list_sector_backups`]) to its path, refusing
/// anything that is not a plain, well-formed backup name inside the backup folder.
pub fn backup_path_for(file_name: &str) -> Option<std::path::PathBuf> {
    if file_name.contains(['/', '\\', ':']) || file_name.contains("..") {
        return None;
    }
    parse_backup_file_name(file_name)?;
    let path = backup_dir().join(file_name);
    path.is_file().then_some(path)
}

/// Durable backup folder: `%APPDATA%\OpenGear\backups\logitech` (temp dir as a last resort).
pub fn backup_dir() -> std::path::PathBuf {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenGear")
        .join("backups")
        .join("logitech")
}

/// File name of a sector backup: `sec<SSSS>_<tag>_<unix_ms>.bin` (sector in hex).
pub(crate) fn backup_file_name(sector: u16, tag: &str, unix_ms: u128) -> String {
    format!("sec{sector:04X}_{tag}_{unix_ms}.bin")
}

/// Sector number encoded in a backup file name produced by [`backup_file_name`].
pub fn parse_backup_sector(file_name: &str) -> Option<u16> {
    let hex = file_name.strip_prefix("sec")?.get(..4)?;
    u16::from_str_radix(hex, 16).ok()
}

/// Save a sector's current bytes before overwriting it. Never overwrites an older backup.
/// Fails (and so aborts the flash) if the backup cannot be written.
pub(crate) fn save_sector_backup(
    sector: u16,
    bytes: &[u8],
    tag: &str,
) -> Result<std::path::PathBuf, HidppError> {
    let dir = backup_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        HidppError::Flash(format!("sauvegarde impossible ({}) : {e}", dir.display()))
    })?;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(backup_file_name(sector, tag, ms));
    std::fs::write(&path, bytes).map_err(|e| {
        HidppError::Flash(format!("sauvegarde impossible ({}) : {e}", path.display()))
    })?;
    prune_old_backups(&dir, sector, 20);
    Ok(path)
}

fn prune_old_backups(dir: &std::path::Path, sector: u16, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut matching: Vec<(u64, std::path::PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let (s, _, ms) = parse_backup_file_name(&name)?;
            (s == sector).then_some((ms, e.path()))
        })
        .collect();
    if matching.len() <= keep {
        return;
    }
    matching.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, old_path) in matching.into_iter().skip(keep) {
        let _ = std::fs::remove_file(old_path);
    }
}

/// Choose the flash sector for an onboard macro bound to one button.
///
/// - `own_sector`: the macro sector currently bound to that button, reused if no other button
///   references it (returns `(sector, true)` = replacing OpenGear's own macro);
/// - otherwise the first sector in `first..end` that no button references and that is blank.
pub(crate) fn pick_macro_sector(
    first: u16,
    end: u16,
    referenced: &[u16],
    own_sector: Option<u16>,
    mut is_blank_sector: impl FnMut(u16) -> Result<bool, HidppError>,
) -> Result<(u16, bool), HidppError> {
    if let Some(s) = own_sector
        && (first..end).contains(&s)
        && referenced.iter().filter(|&&r| r == s).count() == 1
    {
        return Ok((s, true));
    }
    for s in first..end {
        if referenced.contains(&s) {
            continue;
        }
        if is_blank_sector(s)? {
            return Ok((s, false));
        }
    }
    Err(HidppError::Flash(
        "aucun secteur flash libre pour une nouvelle macro".into(),
    ))
}

fn decode_profile_name(raw: &[u8], slot_index: u8) -> String {
    if raw.iter().all(|&b| b == 0xFF || b == 0x00) {
        return format!("Profil {slot_index}");
    }
    // Check if UTF-16LE (common in G HUB: ASCII char followed by 0x00)
    let u16s: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&v| v != 0x0000 && v != 0xFFFF)
        .collect();
    if !u16s.is_empty() && raw.get(1) == Some(&0x00) {
        let s = String::from_utf16_lossy(&u16s).trim().to_string();
        if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
            return s;
        }
    }
    // Fallback ASCII/UTF-8
    let bytes: Vec<u8> = raw
        .iter()
        .copied()
        .take_while(|&b| b != 0x00 && b != 0xFF)
        .collect();
    let s = String::from_utf8_lossy(&bytes).trim().to_string();
    if s.is_empty() || s.chars().any(|c| c.is_control()) {
        format!("Profil {slot_index}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc() -> OnboardMemoryDesc {
        OnboardMemoryDesc {
            memory_model_id: 1,
            profile_format_id: 4,
            macro_format_id: 1,
            profile_count: 5,
            rom_profile_count: 1,
            button_count: 5,
            sector_count: 16,
            sector_size: 255,
            has_g_shift: false,
            has_dpi_shift: false,
        }
    }

    fn with_crc(mut v: Vec<u8>) -> Vec<u8> {
        let n = v.len();
        let crc = crc16_ccitt(&v[..n - 2]).to_be_bytes();
        v[n - 2] = crc[0];
        v[n - 1] = crc[1];
        v
    }

    /// Valid directory with entries (sector, enabled), terminated.
    fn dir_with(entries: &[(u16, bool)]) -> Vec<u8> {
        let mut d = vec![0xFFu8; 255];
        for (i, &(a, en)) in entries.iter().enumerate() {
            let b = a.to_be_bytes();
            d[i * 4..i * 4 + 4].copy_from_slice(&[b[0], b[1], u8::from(en), 0]);
        }
        let t = entries.len() * 4;
        d[t..t + 4].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
        with_crc(d)
    }

    #[test]
    fn crc16_ccitt_false_check_value() {
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn directory_states() {
        assert_eq!(
            DirectoryState::classify(&dir_with(&[(1, true)])),
            DirectoryState::Valid
        );
        assert_eq!(
            DirectoryState::classify(&vec![0xFF; 255]),
            DirectoryState::Blank
        );
        let mut bad = dir_with(&[(1, true)]);
        bad[0] ^= 0x55;
        assert_eq!(DirectoryState::classify(&bad), DirectoryState::Corrupt);
    }

    #[test]
    fn target_is_the_sector_the_directory_assigns() {
        // G HUB stored profile 2 in sector 0x0007: write there, NOT in sector 2.
        let entries = [(0x0001, true), (0x0007, true)];
        assert_eq!(choose_profile_sector(2, &entries, &desc()).unwrap(), 0x0007);
    }

    #[test]
    fn never_targets_a_sector_owned_by_another_profile() {
        // Slot 2 points to ROM, and sector 2 belongs to profile 1.
        let entries = [(0x0002, true), (0x0101, true)];
        assert!(choose_profile_sector(2, &entries, &desc()).is_err());
        // Blank directory: slot n -> sector n
        assert_eq!(choose_profile_sector(3, &[], &desc()).unwrap(), 3);
    }

    #[test]
    fn rom_and_out_of_range_sectors_are_not_user_sectors() {
        let d = desc();
        assert!(!is_user_sector(0, &d));
        assert!(is_user_sector(1, &d));
        assert!(!is_user_sector(16, &d));
        assert!(!is_user_sector(0x0101, &d));
    }

    #[test]
    fn valid_directory_only_changes_this_slot() {
        let raw = dir_with(&[(1, true), (2, false), (3, true)]);
        let d = desc();
        // Already correct -> no write
        assert!(
            build_directory(&raw, DirectoryState::Valid, &d, 1, 1, &[], &[])
                .unwrap()
                .is_none()
        );
        // Enable slot 2
        let out = build_directory(&raw, DirectoryState::Valid, &d, 2, 2, &[], &[])
            .unwrap()
            .unwrap();
        assert_eq!(
            parse_directory(&out, 5),
            vec![(1, true), (2, true), (3, true)]
        );
        // Append slot 4 right after the last entry
        let out = build_directory(&raw, DirectoryState::Valid, &d, 4, 4, &[], &[])
            .unwrap()
            .unwrap();
        assert_eq!(
            parse_directory(&out, 5),
            vec![(1, true), (2, false), (3, true), (4, true)]
        );
        // Slot 5 would leave a hole -> refused
        assert!(build_directory(&raw, DirectoryState::Valid, &d, 5, 5, &[], &[]).is_err());
    }

    #[test]
    fn blank_directory_never_enables_empty_sectors() {
        let d = desc();
        let blank = vec![0xFFu8; 255];
        let flags = [true, true, true, true, true];
        let valid = [false, true, false, false, false];
        let out = build_directory(&blank, DirectoryState::Blank, &d, 3, 3, &flags, &valid)
            .unwrap()
            .unwrap();
        assert_eq!(
            parse_directory(&out, 5),
            vec![(1, false), (2, true), (3, true), (4, false), (5, false)]
        );
    }

    #[test]
    fn corrupt_directory_is_never_rebuilt() {
        let mut bad = dir_with(&[(1, true)]);
        bad[0] ^= 0x55;
        assert!(build_directory(&bad, DirectoryState::Corrupt, &desc(), 1, 1, &[], &[]).is_err());
    }

    #[test]
    fn patch_keeps_unrelated_bytes_and_high_dpi() {
        let mut raw = vec![0xA5u8; 255];
        raw[3..5].copy_from_slice(&44_000u16.to_le_bytes());
        let p = OnboardProfile::parse_sector(1, 1, false, true, true, &raw, 5);
        assert_eq!(p.dpi_stages[0], 44_000);
        let mut out = raw.clone();
        p.patch_into(&mut out);
        assert_eq!(
            u16::from_le_bytes([out[3], out[4]]),
            44_000,
            "44k DPI must not be clamped"
        );
        // LED / metadata area untouched
        assert_eq!(&out[200..250], &raw[200..250]);
    }

    #[test]
    fn button_actions_round_trip() {
        for &(_, act) in MouseButtonAction::PRESET_ACTIONS {
            assert_eq!(MouseButtonAction::decode(act.encode()), act);
        }
        let m = MouseButtonAction::OnboardMacro {
            sector: 6,
            offset: 0,
        };
        assert_eq!(MouseButtonAction::decode(m.encode()), m);
    }

    #[test]
    fn backup_names_round_trip() {
        let name = backup_file_name(0x0007, "profil", 1_700_000_000_000);
        assert_eq!(parse_backup_sector(&name), Some(0x0007));
        assert_eq!(parse_backup_sector("random.bin"), None);
    }

    #[test]
    fn macro_sector_reuses_own_or_takes_a_blank_unreferenced_one() {
        // Own macro in sector 7, referenced only by this button -> reuse
        assert_eq!(
            pick_macro_sector(6, 16, &[7], Some(7), |_| Ok(false)).unwrap(),
            (7, true)
        );
        // Sector 6 referenced by G HUB, 7 not blank, 8 blank -> 8
        assert_eq!(
            pick_macro_sector(6, 16, &[6], None, |s| Ok(s == 8)).unwrap(),
            (8, false)
        );
        // Own sector shared with another button -> not reused
        assert_eq!(
            pick_macro_sector(6, 16, &[7, 7], Some(7), |s| Ok(s == 9)).unwrap(),
            (9, false)
        );
        // Nothing free
        assert!(pick_macro_sector(6, 16, &[], None, |_| Ok(false)).is_err());
    }
}
