use crate::proto::*;
use crate::transport::BragiTransport;
use opengear_core::Rgb;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HwpLightingKind {
    StaticColor,
    RainbowWave,
    SpiralRainbow,
    Rainbow,
    ColorShift,
    ColorPulse,
    ColorWave,
    Visor,
    Rain,
    Unknown(u16),
}

impl HwpLightingKind {
    pub const ALL_SUPPORTED: &'static [Self] = &[
        Self::StaticColor,
        Self::RainbowWave,
        Self::SpiralRainbow,
        Self::ColorPulse,
        Self::ColorWave,
        Self::ColorShift,
        Self::Visor,
        Self::Rain,
    ];

    pub fn from_layout_header(hdr: u16) -> Self {
        match hdr {
            LAYOUT_STATIC_COLOR => Self::StaticColor,
            LAYOUT_RAINBOW_WAVE => Self::RainbowWave,
            LAYOUT_SPIRAL_RAINBOW => Self::SpiralRainbow,
            LAYOUT_RAINBOW => Self::Rainbow,
            LAYOUT_COLOR_SHIFT => Self::ColorShift,
            LAYOUT_COLOR_PULSE => Self::ColorPulse,
            LAYOUT_COLOR_WAVE => Self::ColorWave,
            LAYOUT_VISOR => Self::Visor,
            LAYOUT_RAIN => Self::Rain,
            other => Self::Unknown(other),
        }
    }

    pub fn to_layout_header(self) -> u16 {
        match self {
            Self::StaticColor => LAYOUT_STATIC_COLOR,
            Self::RainbowWave => LAYOUT_RAINBOW_WAVE,
            Self::SpiralRainbow => LAYOUT_SPIRAL_RAINBOW,
            Self::Rainbow => LAYOUT_RAINBOW,
            Self::ColorShift => LAYOUT_COLOR_SHIFT,
            Self::ColorPulse => LAYOUT_COLOR_PULSE,
            Self::ColorWave => LAYOUT_COLOR_WAVE,
            Self::Visor => LAYOUT_VISOR,
            Self::Rain => LAYOUT_RAIN,
            Self::Unknown(h) => h,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::StaticColor => "StaticColor",
            Self::RainbowWave => "RainbowWave",
            Self::SpiralRainbow => "SpiralRainbow",
            Self::Rainbow => "Rainbow",
            Self::ColorShift => "ColorShift",
            Self::ColorPulse => "ColorPulse",
            Self::ColorWave => "ColorWave",
            Self::Visor => "Visor",
            Self::Rain => "Rain",
            Self::Unknown(_) => "Unknown",
        }
    }

    pub fn parse_id(s: &str) -> Self {
        match s {
            "StaticColor" => Self::StaticColor,
            "RainbowWave" => Self::RainbowWave,
            "SpiralRainbow" => Self::SpiralRainbow,
            "Rainbow" => Self::Rainbow,
            "ColorShift" => Self::ColorShift,
            "ColorPulse" => Self::ColorPulse,
            "ColorWave" => Self::ColorWave,
            "Visor" => Self::Visor,
            "Rain" => Self::Rain,
            _ => Self::StaticColor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::StaticColor => "Couleur Statique (Flash)",
            Self::RainbowWave => "Vague Arc-en-ciel (Flash)",
            Self::SpiralRainbow => "Spirale Arc-en-ciel (Flash)",
            Self::Rainbow => "Arc-en-ciel (Flash)",
            Self::ColorShift => "Transition Couleurs (Flash)",
            Self::ColorPulse => "Respiration / Pulse (Flash)",
            Self::ColorWave => "Vague Couleurs (Flash)",
            Self::Visor => "Visor / Scanner (Flash)",
            Self::Rain => "Pluie (Flash)",
            Self::Unknown(_) => "Effet Matériel Inconnu",
        }
    }

    pub fn color_count(self) -> usize {
        match self {
            Self::RainbowWave | Self::SpiralRainbow | Self::Rainbow => 0,
            Self::StaticColor => 1,
            _ => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HwpLightingLayer {
    pub file_id: u16,
    pub kind: HwpLightingKind,
    #[serde(default = "default_layer_flags")]
    pub flags: u8,
    pub speed: u8,
    pub direction: u8,
    pub random_colors: bool,
    pub colors: Vec<Rgb>,
    pub led_indices: Vec<u8>,
}

fn default_layer_flags() -> u8 {
    0x01
}

impl HwpLightingLayer {
    pub fn decode(file_id: u16, raw: &[u8]) -> Option<Self> {
        if raw.len() < 8 {
            return None;
        }
        let hdr = u16::from_le_bytes([raw[0], raw[1]]);
        let kind = HwpLightingKind::from_layout_header(hdr);
        let flags = raw[2];
        let speed = raw[3];
        let direction = raw[4];
        let random_colors = raw[5] != 0;
        let num_colors = raw[6] as usize;

        let mut pos = 7usize;
        let mut colors = Vec::with_capacity(num_colors);
        for _ in 0..num_colors {
            if pos + 4 > raw.len() {
                break;
            }
            let _a = raw[pos];
            let b = raw[pos + 1];
            let g = raw[pos + 2];
            let r = raw[pos + 3];
            colors.push(Rgb::new(r, g, b));
            pos += 4;
        }

        let led_indices = if pos < raw.len() {
            let n_leds = raw[pos] as usize;
            pos += 1;
            let end = (pos + n_leds).min(raw.len());
            raw[pos..end].to_vec()
        } else {
            Vec::new()
        };

        Some(Self {
            file_id,
            kind,
            flags,
            speed,
            direction,
            random_colors,
            colors,
            led_indices,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let hdr = self.kind.to_layout_header().to_le_bytes();
        let wanted_colors = match self.kind {
            HwpLightingKind::Unknown(_) if !self.colors.is_empty() => self.colors.len(),
            _ => self.kind.color_count(),
        };
        let mut out = Vec::with_capacity(12 + wanted_colors * 4 + self.led_indices.len());
        out.push(hdr[0]);
        out.push(hdr[1]);
        out.push(self.flags);
        out.push(self.speed.min(2));
        out.push(self.direction.min(5));
        out.push(u8::from(self.random_colors && wanted_colors > 0));
        out.push(wanted_colors as u8);

        for i in 0..wanted_colors {
            let c = self
                .colors
                .get(i)
                .copied()
                .unwrap_or_else(|| if i == 0 { Rgb::CYAN } else { Rgb::new(155, 60, 255) });
            out.push(0xFF);
            out.push(c.b);
            out.push(c.g);
            out.push(c.r);
        }

        let led_count = self.led_indices.len().min(255);
        out.push(led_count as u8);
        out.extend_from_slice(&self.led_indices[..led_count]);
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HwpProfileInfo {
    pub slot_index: usize,
    pub profile_file_id: u16,
    pub name: String,
    pub assignment_map_file: u16,
    pub property_map_file: u16,
    pub blob_container_file: u16,
    pub lightings_list_file: u16,
    pub key_mask_file: u16,
    pub assignment_count: usize,
    pub property_count: usize,
    pub lighting: Option<HwpLightingLayer>,
    #[serde(skip)]
    pub raw_profile: Vec<u8>,
}

/// Durable Corsair backup folder: `%APPDATA%\OpenGear\backups\corsair`.
pub fn corsair_backup_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenGear")
        .join("backups")
        .join("corsair")
}

#[derive(Debug, Clone, Serialize)]
pub struct CorsairFlashBackup {
    pub file: String,
    pub file_id_hex: String,
    pub tag: String,
    pub created_ms: u64,
    pub size: u64,
}

fn parse_corsair_backup_name(name: &str) -> Option<(u16, String, u64)> {
    let hex = name.strip_prefix("file")?.get(..4)?;
    let fid = u16::from_str_radix(hex, 16).ok()?;
    if name.as_bytes().get(8) != Some(&b'_') {
        return None;
    }
    let rest = name.get(9..)?.strip_suffix(".bin")?;
    let (tag, ms) = rest.rsplit_once('_')?;
    let ms: u64 = ms.parse().ok()?;
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some((fid, tag.to_string(), ms))
}

pub fn list_corsair_backups() -> Vec<CorsairFlashBackup> {
    let Ok(rd) = std::fs::read_dir(corsair_backup_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<CorsairFlashBackup> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let (fid, tag, ms) = parse_corsair_backup_name(&name)?;
            let size = e.metadata().ok()?.len();
            Some(CorsairFlashBackup {
                file: name,
                file_id_hex: format!("0x{fid:04X}"),
                tag,
                created_ms: ms,
                size,
            })
        })
        .collect();
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    out
}

pub fn corsair_backup_path_for(file_name: &str) -> Option<PathBuf> {
    if file_name.contains(['/', '\\', ':']) || file_name.contains("..") {
        return None;
    }
    parse_corsair_backup_name(file_name)?;
    let path = corsair_backup_dir().join(file_name);
    path.is_file().then_some(path)
}

pub(crate) fn save_corsair_backup_to(
    dir: &Path,
    file_id: u16,
    tag: &str,
    bytes: &[u8],
) -> Result<PathBuf, BragiError> {
    std::fs::create_dir_all(dir)
        .map_err(|_| BragiError::Protocol(0x04, "Impossible de créer le dossier de sauvegarde Corsair"))?;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("file{file_id:04X}_{tag}_{ms}.bin"));
    std::fs::write(&path, bytes)
        .map_err(|_| BragiError::Protocol(0x04, "Impossible d'écrire la sauvegarde Flash Corsair"))?;
    Ok(path)
}

pub fn restore_corsair_backup(t: &mut BragiTransport, file_name: &str) -> Result<u16, BragiError> {
    let path = corsair_backup_path_for(file_name)
        .ok_or(BragiError::Protocol(0x01, "Sauvegarde Corsair introuvable ou invalide"))?;
    let (file_id, _, _) = parse_corsair_backup_name(file_name)
        .ok_or(BragiError::Protocol(0x01, "Nom de sauvegarde Corsair invalide"))?;
    let bytes = std::fs::read(&path)
        .map_err(|_| BragiError::Protocol(0x04, "Lecture de la sauvegarde Corsair impossible"))?;
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err(BragiError::Protocol(0x01, "Taille de sauvegarde Corsair invalide"));
    }
    t.write_file(file_id, &bytes)?;
    let verify = t.read_file(file_id)?;
    if verify != bytes {
        return Err(BragiError::Protocol(0x04, "Vérification restauration Corsair échouée"));
    }
    Ok(file_id)
}

/// Truncate a UTF-8 string so its byte length never exceeds `max_bytes`, never splitting a char.
pub(crate) fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> &[u8] {
    if s.len() <= max_bytes {
        return s.as_bytes();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s.as_bytes()[..end]
}

/// Patch the UTF-8 profile name inside a raw `Profile` (`0xF11E`) flash file.
pub(crate) fn patch_profile_name_bytes(raw_profile: &[u8], new_name: &str) -> Result<Vec<u8>, BragiError> {
    if raw_profile.len() < 32 || u16::from_le_bytes([raw_profile[0], raw_profile[1]]) != LAYOUT_PROFILE {
        return Err(BragiError::Protocol(0x01, "En-tête profil Bragi 0xF11E invalide"));
    }
    let old_len = u16::from_le_bytes([raw_profile[30], raw_profile[31]]) as usize;
    let tail_start = (32 + old_len).min(raw_profile.len());
    let capped = truncate_utf8_bytes(new_name.trim(), 60);
    let mut new_prof = Vec::with_capacity(32 + capped.len() + raw_profile.len() - tail_start);
    new_prof.extend_from_slice(&raw_profile[..30]);
    new_prof.extend_from_slice(&(capped.len() as u16).to_le_bytes());
    new_prof.extend_from_slice(capped);
    new_prof.extend_from_slice(&raw_profile[tail_start..]);
    Ok(new_prof)
}

pub fn read_all_hwp_profiles(t: &mut BragiTransport) -> Result<Vec<HwpProfileInfo>, BragiError> {
    let plist = t.read_file(FILE_PROFILES_LIST)?;
    if plist.len() < 4 {
        return Ok(Vec::new());
    }
    let count = u16::from_le_bytes([plist[2], plist[3]]) as usize;
    let mut profile_ids = Vec::with_capacity(count);
    for i in 0..count {
        let off = 4 + i * 2;
        if off + 1 < plist.len() {
            let fid = u16::from_le_bytes([plist[off], plist[off + 1]]);
            if fid != 0 {
                profile_ids.push(fid);
            }
        }
    }

    let mut profiles = Vec::new();
    for (slot_index, pfid) in profile_ids.into_iter().enumerate() {
        let Ok(raw) = t.read_file(pfid) else {
            continue;
        };
        if raw.len() < 32 || u16::from_le_bytes([raw[0], raw[1]]) != LAYOUT_PROFILE {
            continue;
        }
        let assignment_map_file = u16::from_le_bytes([raw[6], raw[7]]);
        let property_map_file = u16::from_le_bytes([raw[8], raw[9]]);
        let blob_container_file = u16::from_le_bytes([raw[10], raw[11]]);
        let lightings_list_file = u16::from_le_bytes([raw[12], raw[13]]);
        let key_mask_file = u16::from_le_bytes([raw[14], raw[15]]);

        let name_len = u16::from_le_bytes([raw[30], raw[31]]) as usize;
        let name = if 32 + name_len <= raw.len() {
            String::from_utf8_lossy(&raw[32..32 + name_len])
                .trim_end_matches('\0')
                .to_string()
        } else {
            format!("Profil HW #{}", slot_index + 1)
        };

        let assignment_count = if assignment_map_file != 0 {
            t.read_file(assignment_map_file)
                .ok()
                .map(|b| b.len().saturating_sub(4) / 8)
                .unwrap_or(0)
        } else {
            0
        };

        let property_count = if property_map_file != 0 {
            t.read_file(property_map_file)
                .ok()
                .and_then(|b| {
                    if b.len() >= 4 {
                        Some(u16::from_le_bytes([b[2], b[3]]) as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or(0)
        } else {
            0
        };

        let mut lighting = None;
        if lightings_list_file != 0 {
            if let Ok(ll) = t.read_file(lightings_list_file) {
                if ll.len() >= 8 && u16::from_le_bytes([ll[0], ll[1]]) == LAYOUT_LIGHTINGS_LIST {
                    let layer_cnt = u16::from_le_bytes([ll[2], ll[3]]) as usize;
                    for li in 0..layer_cnt.min(8) {
                        let off = 6 + li * 2;
                        if off + 1 >= ll.len() {
                            break;
                        }
                        let lfid = u16::from_le_bytes([ll[off], ll[off + 1]]);
                        if lfid != 0 {
                            if let Ok(lraw) = t.read_file(lfid) {
                                if let Some(decoded) = HwpLightingLayer::decode(lfid, &lraw) {
                                    lighting = Some(decoded);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        profiles.push(HwpProfileInfo {
            slot_index,
            profile_file_id: pfid,
            name,
            assignment_map_file,
            property_map_file,
            blob_container_file,
            lightings_list_file,
            key_mask_file,
            assignment_count,
            property_count,
            lighting,
            raw_profile: raw,
        });
    }

    Ok(profiles)
}

/// Safely flash a Hardware Lighting Effect (and optional profile name) with durable backups,
/// byte-for-byte read-back verification, and automatic rollback on verification failure.
pub fn flash_hwp_profile_lighting(
    t: &mut BragiTransport,
    profile: &HwpProfileInfo,
    kind: HwpLightingKind,
    primary: Rgb,
    secondary: Rgb,
    speed: u8,
    direction: u8,
    new_name: Option<&str>,
) -> Result<HwpLightingLayer, BragiError> {
    let existing = profile
        .lighting
        .as_ref()
        .ok_or(BragiError::Protocol(0x01, "Aucun fichier lighting dans ce profil HW"))?;

    // 1. Durable timestamped backup BEFORE writing anything (abort flash if backup fails!)
    let bdir = corsair_backup_dir();
    let orig_l = t.read_file(existing.file_id)?;
    save_corsair_backup_to(&bdir, existing.file_id, "lighting", &orig_l)?;
    let orig_prof = t.read_file(profile.profile_file_id)?;
    save_corsair_backup_to(&bdir, profile.profile_file_id, "profile", &orig_prof)?;

    // 2. Build new lighting layer preserving flags & hardware LED index list
    let new_layer = HwpLightingLayer {
        file_id: existing.file_id,
        kind,
        flags: existing.flags,
        speed: speed.min(2),
        direction: direction.min(5),
        random_colors: false,
        colors: vec![primary, secondary],
        led_indices: existing.led_indices.clone(),
    };
    let encoded = new_layer.encode();

    // 3. Write lighting file reliably & verify exact read-back, rolling back on failure
    if let Err(e) = t.write_file(existing.file_id, &encoded) {
        let _ = t.write_file(existing.file_id, &orig_l);
        return Err(e);
    }
    let verify = t.read_file(existing.file_id)?;
    if verify != encoded {
        let _ = t.write_file(existing.file_id, &orig_l);
        return Err(BragiError::Protocol(
            0x04,
            "Échec vérification lecture après écriture Flash HWP (rollback appliqué)",
        ));
    }

    // 4. Optionally update UTF-8 profile name inside `Profile` (0xF11E) with read-back & rollback
    if let Some(name) = new_name {
        let trimmed = name.trim();
        if !trimmed.is_empty() && trimmed != profile.name {
            let new_prof = patch_profile_name_bytes(&orig_prof, trimmed)?;
            if let Err(e) = t.write_file(profile.profile_file_id, &new_prof) {
                let _ = t.write_file(profile.profile_file_id, &orig_prof);
                return Err(e);
            }
            let verify_prof = t.read_file(profile.profile_file_id)?;
            if verify_prof != new_prof {
                let _ = t.write_file(profile.profile_file_id, &orig_prof);
                return Err(BragiError::Protocol(
                    0x04,
                    "Échec vérification renommage profil Flash HWP (rollback appliqué)",
                ));
            }
        }
    }

    info!(
        "Profil matériel Corsair 0x{:04X} flashé (lighting 0x{:04X} -> {}, {} octets, vérifié OK)",
        profile.profile_file_id,
        existing.file_id,
        kind.label(),
        encoded.len()
    );
    Ok(new_layer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_round_trip_preserves_flags_and_colors() {
        let layer = HwpLightingLayer {
            file_id: 0x6D67,
            kind: HwpLightingKind::ColorWave,
            flags: 0x09,
            speed: 2,
            direction: 3,
            random_colors: true,
            colors: vec![Rgb::new(10, 20, 30), Rgb::new(40, 50, 60)],
            led_indices: vec![1, 4, 20],
        };
        let enc = layer.encode();
        let dec = HwpLightingLayer::decode(0x6D67, &enc).unwrap();
        assert_eq!(dec.flags, 0x09);
        assert_eq!(dec.speed, 2);
        assert_eq!(dec.direction, 3);
        assert!(dec.random_colors);
        assert_eq!(dec.colors, layer.colors);
        assert_eq!(dec.led_indices, layer.led_indices);
    }

    #[test]
    fn utf8_rename_truncates_on_char_boundary_even_for_empty_initial_name() {
        let mut raw = vec![0u8; 32];
        raw[0..2].copy_from_slice(&LAYOUT_PROFILE.to_le_bytes());
        // 30-char French string of 2-byte chars = 60 bytes, plus extra 'é' -> 62 bytes
        let long_fr = "é".repeat(31);
        let patched = patch_profile_name_bytes(&raw, &long_fr).unwrap();
        let name_len = u16::from_le_bytes([patched[30], patched[31]]) as usize;
        assert_eq!(name_len, 60);
        assert!(std::str::from_utf8(&patched[32..32 + name_len]).is_ok());
    }

    #[test]
    fn backup_path_rejects_traversal() {
        assert!(corsair_backup_path_for("../file6D67_lighting_1.bin").is_none());
        assert!(corsair_backup_path_for("file6D67_lighting_1.bin").is_none()); // missing in non-existent dir, valid syntax
        assert!(parse_corsair_backup_name("file6D67_lighting_1700000000.bin").is_some());
        assert!(parse_corsair_backup_name("bad_name.bin").is_none());
    }
}
