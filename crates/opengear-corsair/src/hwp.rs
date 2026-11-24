use crate::proto::*;
use crate::transport::BragiTransport;
use opengear_core::Rgb;
use serde::{Deserialize, Serialize};
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
    pub speed: u8,     // 0=Lent, 1=Moyen, 2=Rapide
    pub direction: u8, // 0=Gauche, 1=Droite, 2=Haut, 3=Bas, 4=Horaire, 5=Anti-horaire
    pub random_colors: bool,
    pub colors: Vec<Rgb>,
    pub led_indices: Vec<u8>,
}

impl HwpLightingLayer {
    /// Decode a Bragi v2 HWP lighting file (verified byte-for-byte against K70 RGB TKL `0x6D67` + `BragiProtocols.dll` `FUN_180260410`).
    pub fn decode(file_id: u16, raw: &[u8]) -> Option<Self> {
        if raw.len() < 8 {
            return None;
        }
        let hdr = u16::from_le_bytes([raw[0], raw[1]]);
        let kind = HwpLightingKind::from_layout_header(hdr);
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
            // Verified byte order from K70 0x6D67 + 0x6D60 Cereal XML: [Alpha, Blue, Green, Red]!
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
            speed,
            direction,
            random_colors,
            colors,
            led_indices,
        })
    }

    /// Encode into exact Bragi v2 HWP binary layout (`FUN_180260410`).
    pub fn encode(&self) -> Vec<u8> {
        let hdr = self.kind.to_layout_header().to_le_bytes();
        let wanted_colors = self.kind.color_count();
        let mut out = Vec::with_capacity(12 + wanted_colors * 4 + self.led_indices.len());
        out.push(hdr[0]);
        out.push(hdr[1]);
        out.push(0x01); // flags: startWithProfile = true, key-index list (bit 3 = 0)
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
            // Exact verified [Alpha, Blue, Green, Red] encoding!
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

/// Read all onboard hardware profiles from Bragi v2 Flash (`ProfilesList 0x000F` -> `Profile 0xF11E` -> `LightingsList 0x6C69` -> Lighting Effect).
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
                    let layer_cnt = u16::from_le_bytes([ll[2], ll[3]]);
                    if layer_cnt >= 1 {
                        let first_lfid = u16::from_le_bytes([ll[6], ll[7]]);
                        if first_lfid != 0 {
                            if let Ok(lraw) = t.read_file(first_lfid) {
                                lighting = HwpLightingLayer::decode(first_lfid, &lraw);
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

/// Safely flash a Hardware Lighting Effect (and optional profile name) into the K70 onboard flash!
/// Automatically backs up the original flash files to `%TEMP%` before writing, and reads back to verify byte-for-byte!
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

    // 1. Automatic local backup of original lighting file + profile file
    let backup_dir = std::env::temp_dir();
    if let Ok(orig_l) = t.read_file(existing.file_id) {
        let _ = std::fs::write(
            backup_dir.join(format!("opengear_corsair_backup_0x{:04X}.bin", existing.file_id)),
            &orig_l,
        );
    }
    let _ = std::fs::write(
        backup_dir.join(format!(
            "opengear_corsair_backup_prof_0x{:04X}.bin",
            profile.profile_file_id
        )),
        &profile.raw_profile,
    );

    // 2. Build new lighting layer preserving the exact hardware LED index list
    let new_layer = HwpLightingLayer {
        file_id: existing.file_id,
        kind,
        speed: speed.min(2),
        direction: direction.min(5),
        random_colors: false,
        colors: vec![primary, secondary],
        led_indices: existing.led_indices.clone(),
    };
    let encoded = new_layer.encode();

    // 3. Write lighting file to flash & verify exact read-back!
    t.write_file(existing.file_id, &encoded)?;
    let verify = t.read_file(existing.file_id)?;
    if verify != encoded {
        return Err(BragiError::Protocol(
            0x04,
            "Échec vérification lecture après écriture Flash HWP",
        ));
    }

    // 4. Optionally update UTF-8 profile name inside `Profile` (0xF11E) preserving all subfile pointers
    if let Some(name) = new_name {
        let trimmed = name.trim();
        if !trimmed.is_empty() && trimmed != profile.name && profile.raw_profile.len() >= 33 {
            let old_len =
                u16::from_le_bytes([profile.raw_profile[30], profile.raw_profile[31]]) as usize;
            let tail_start = (32 + old_len).min(profile.raw_profile.len());
            let name_bytes = trimmed.as_bytes();
            let capped = &name_bytes[..name_bytes.len().min(60)];
            let mut new_prof = Vec::with_capacity(32 + capped.len() + profile.raw_profile.len() - tail_start);
            new_prof.extend_from_slice(&profile.raw_profile[..30]);
            new_prof.extend_from_slice(&(capped.len() as u16).to_le_bytes());
            new_prof.extend_from_slice(capped);
            new_prof.extend_from_slice(&profile.raw_profile[tail_start..]);
            t.write_file(profile.profile_file_id, &new_prof)?;
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
