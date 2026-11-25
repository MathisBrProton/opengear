use crate::features::*;
use crate::hidpp::{BatteryInfo, HidppError, HidppTransport, read_battery_info};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqualizerInfo {
    pub band_count: usize,
    pub min_db: i8,
    pub max_db: i8,
    pub frequencies_hz: Vec<u16>,
    pub gains_db: Vec<i8>,
}

#[derive(Debug, Clone, Copy)]
pub struct EqPreset {
    pub name: &'static str,
    pub gains_5: [i8; 5],
}

pub const EQ_PRESETS: &[EqPreset] = &[
    EqPreset { name: "Plat (Défaut)", gains_5: [0, 0, 0, 0, 0] },
    EqPreset { name: "FPS (Pro G HUB)", gains_5: [2, 2, 0, 4, 1] },
    EqPreset { name: "MOBA (Pro G HUB)", gains_5: [0, -2, 3, 3, 3] },
    EqPreset { name: "G2 shox", gains_5: [1, -1, -2, 1, 2] },
    EqPreset { name: "TSM Bjergsen", gains_5: [2, 2, -1, 4, 4] },
    EqPreset { name: "TSM Myth", gains_5: [3, 1, 2, 3, 2] },
    EqPreset { name: "TSM Hamlinz", gains_5: [1, 1, 2, 3, -1] },
    EqPreset { name: "Spitfire Profit", gains_5: [-2, -1, 1, 2, 3] },
    EqPreset { name: "Bass Boost", gains_5: [6, 4, 0, 1, 2] },
    EqPreset { name: "Voix / Discord", gains_5: [-2, 1, 4, 3, -1] },
];

#[derive(Debug, Clone)]
pub struct LogitechHeadsetInfo {
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub proto_version: String,
    pub firmware: Vec<String>,
    pub features: Vec<FeatureEntry>,
    pub battery: Option<BatteryInfo>,
    pub supports_sidetone: bool,
    pub sidetone_pct: u8,
    pub equalizer: Option<EqualizerInfo>,
}

pub struct LogitechHeadset {
    transport: HidppTransport,
    pub info: LogitechHeadsetInfo,
}

impl LogitechHeadset {
    /// Discover and connect to ALL Logitech HID++ headsets on UsagePage `0xFF43`.
    pub fn connect_all() -> Vec<Self> {
        HidppTransport::open_all_matching(0xFF43, Some(0x0202))
            .into_iter()
            .map(Self::from_transport)
            .collect()
    }

    /// Connect to Logitech G PRO / PRO X Gaming Headset (`046D:0AAA`, UsagePage `0xFF43` / `0x0202`).
    pub fn connect() -> Result<Self, HidppError> {
        let transport = HidppTransport::open_matching(None, 0xFF43, Some(0x0202))?;
        Ok(Self::from_transport(transport))
    }

    fn from_transport(transport: HidppTransport) -> Self {
        let name = transport
            .read_device_name()
            .unwrap_or_else(|| transport.product_string.clone());
        let firmware = transport.read_firmware_versions();
        let proto_version = format!("{}.{}", transport.proto_major, transport.proto_minor);

        let supports_sidetone = transport.get_feature_index(FEAT_SIDETONE).is_some()
            || transport.get_feature_index(FEAT_HEADSET_AUDIO_SIDETONE).is_some()
            || transport.pid == 0x0AAA;

        let battery = read_battery_info(&transport);
        let sidetone_pct = Self::read_sidetone_inner(&transport).unwrap_or(0);
        let equalizer = Self::read_equalizer_inner(&transport);

        let info = LogitechHeadsetInfo {
            vid: transport.vid,
            pid: transport.pid,
            name,
            proto_version,
            firmware,
            features: transport.features.clone(),
            battery,
            supports_sidetone,
            sidetone_pct,
            equalizer,
        };

        info!(
            "Casque Logitech connecté : {} ({:04X}:{:04X}, HID++ {}, {} features)",
            info.name,
            info.vid,
            info.pid,
            info.proto_version,
            info.features.len()
        );

        Self { transport, info }
    }

    fn read_sidetone_inner(t: &HidppTransport) -> Option<u8> {
        if let Ok(r) = t.request_feature(FEAT_SIDETONE, 0x00, &[]) {
            return r.first().copied();
        }
        if let Ok(r) = t.request_feature(FEAT_HEADSET_AUDIO_SIDETONE, 0x00, &[]) {
            return r.get(2).copied().or_else(|| r.first().copied());
        }
        None
    }

    /// Set headset microphone sidetone level (0..=100%).
    pub fn set_sidetone(&mut self, percent: u8) -> Result<(), HidppError> {
        let pct = percent.min(100);
        if self.transport.get_feature_index(FEAT_SIDETONE).is_some() {
            self.transport.request_feature(FEAT_SIDETONE, 0x10, &[pct])?;
        } else if self.transport.get_feature_index(FEAT_HEADSET_AUDIO_SIDETONE).is_some() {
            self.transport
                .request_feature(FEAT_HEADSET_AUDIO_SIDETONE, 0x10, &[0x01, pct])?;
        } else if self.info.pid == 0x0AAA {
            // Direct G PRO X fallback verified from HeadsetControl (`logitech_gpro.hpp`: {0x05, 0x1a, level})
            self.transport.write_raw_long(0xFF, &[0x05, 0x1A, pct])?;
        } else {
            return Err(HidppError::UnsupportedFeature(FEAT_SIDETONE));
        }
        self.info.sidetone_pct = pct;
        Ok(())
    }

    fn parse_equalizer_info(info_r: &[u8]) -> Option<(usize, i8, i8)> {
        if info_r.len() < 5 {
            return None;
        }
        let count = info_r[0] as usize;
        if count == 0 || count > 15 {
            return None;
        }
        // Clamp symmetric range below 128 so negating `-db_range` never overflows i8::MIN.
        let db_range = (info_r[1] & 0x7F).min(24) as i8;
        let mut min_db = info_r[3] as i8;
        let mut max_db = info_r[4] as i8;
        if min_db == 0 {
            min_db = -db_range;
        }
        if max_db == 0 {
            max_db = db_range;
        }
        if min_db > max_db {
            std::mem::swap(&mut min_db, &mut max_db);
        }
        Some((count, min_db, max_db))
    }

    fn read_equalizer_inner(t: &HidppTransport) -> Option<EqualizerInfo> {
        if t.get_feature_index(FEAT_EQUALIZER).is_none() {
            return None;
        }
        let info_r = t.request_feature(FEAT_EQUALIZER, 0x00, &[]).ok()?;
        let (count, min_db, max_db) = Self::parse_equalizer_info(&info_r)?;

        let mut frequencies_hz = Vec::with_capacity(count);
        for g in 0..count.div_ceil(7) {
            if let Ok(fr) = t.request_feature(FEAT_EQUALIZER, 0x10, &[(g * 7) as u8]) {
                for b in 0..7 {
                    if g * 7 + b >= count {
                        break;
                    }
                    let off = 1 + b * 2;
                    if off + 1 < fr.len() {
                        frequencies_hz.push(u16::from_be_bytes([fr[off], fr[off + 1]]));
                    }
                }
            }
        }

        let mut gains_db = vec![0i8; count];
        if let Ok(gr) = t.request_feature(FEAT_EQUALIZER, 0x20, &[0x00]) {
            for (i, slot) in gains_db.iter_mut().enumerate() {
                if let Some(&b) = gr.get(i + 1) {
                    *slot = b as i8;
                }
            }
        }

        Some(EqualizerInfo {
            band_count: count,
            min_db,
            max_db,
            frequencies_hz,
            gains_db,
        })
    }

    /// Write hardware equalizer band gains (in dB) to the headset via `0x8310`.
    pub fn set_equalizer_gains(&mut self, gains: &[i8]) -> Result<(), HidppError> {
        let Some(ref eq) = self.info.equalizer.clone() else {
            return Err(HidppError::UnsupportedFeature(FEAT_EQUALIZER));
        };
        // 1 prefix byte + one byte per band must fit a single long HID++ report (16 params).
        if eq.band_count > 15 {
            return Err(HidppError::Protocol(
                0x02,
                "Égaliseur à plus de 15 bandes non supporté (rapport HID++ trop court)",
            ));
        }
        let mut payload = Vec::with_capacity(1 + eq.band_count);
        payload.push(0x02); // Solaar write_prefix_bytes = 0x02 (save/apply)
        for i in 0..eq.band_count {
            let g = gains.get(i).copied().unwrap_or(0).clamp(eq.min_db, eq.max_db);
            payload.push(g as u8);
        }
        self.transport.request_feature(FEAT_EQUALIZER, 0x30, &payload)?;
        if let Some(cur) = self.info.equalizer.as_mut() {
            for i in 0..cur.band_count {
                cur.gains_db[i] = gains.get(i).copied().unwrap_or(0).clamp(cur.min_db, cur.max_db);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_equalizer_info_guards_overflow_and_band_limit() {
        assert_eq!(LogitechHeadset::parse_equalizer_info(&[5, 0x80, 0, 0, 0]), Some((5, 0, 0)));
        assert_eq!(LogitechHeadset::parse_equalizer_info(&[5, 12, 0, 0, 0]), Some((5, -12, 12)));
        assert_eq!(LogitechHeadset::parse_equalizer_info(&[5, 6, 0, 10, 2]), Some((5, 2, 10)));
        assert_eq!(LogitechHeadset::parse_equalizer_info(&[16, 12, 0, 0, 0]), None);
        assert_eq!(LogitechHeadset::parse_equalizer_info(&[0, 12, 0, 0, 0]), None);
    }
}
