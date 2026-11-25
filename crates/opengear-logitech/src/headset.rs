use crate::features::*;
use crate::hidpp::{
    BatteryInfo, HidppError, HidppTransport, TAG_POWER_OFF_TIMEOUT, estimate_rated_battery_hours,
    read_battery_info, read_global_setting_u8, write_global_setting_u8, write_led_zone_raw,
};
use crate::MouseLedMode;
use opengear_core::Rgb;
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
    EqPreset {
        name: "Plat (Défaut)",
        gains_5: [0, 0, 0, 0, 0],
    },
    EqPreset {
        name: "FPS (Pro G HUB)",
        gains_5: [2, 2, 0, 4, 1],
    },
    EqPreset {
        name: "MOBA (Pro G HUB)",
        gains_5: [0, -2, 3, 3, 3],
    },
    EqPreset {
        name: "G2 shox",
        gains_5: [1, -1, -2, 1, 2],
    },
    EqPreset {
        name: "TSM Bjergsen",
        gains_5: [2, 2, -1, 4, 4],
    },
    EqPreset {
        name: "TSM Myth",
        gains_5: [3, 1, 2, 3, 2],
    },
    EqPreset {
        name: "TSM Hamlinz",
        gains_5: [1, 1, 2, 3, -1],
    },
    EqPreset {
        name: "Spitfire Profit",
        gains_5: [-2, -1, 1, 2, 3],
    },
    EqPreset {
        name: "Bass Boost",
        gains_5: [6, 4, 0, 1, 2],
    },
    EqPreset {
        name: "Voix / Discord",
        gains_5: [-2, 1, 4, 3, -1],
    },
];

#[derive(Debug, Clone)]
pub struct LogitechHeadsetInfo {
    pub vid: u16,
    pub pid: u16,
    pub device_index: u8,
    pub is_wireless: bool,
    pub name: String,
    pub proto_version: String,
    pub firmware: Vec<String>,
    pub features: Vec<FeatureEntry>,
    pub battery: Option<BatteryInfo>,
    pub supports_sidetone: bool,
    pub sidetone_pct: u8,
    pub supports_mic_mute: bool,
    pub mic_muted: Option<bool>,
    pub bass_tone: Option<u8>,
    pub eq_preset: Option<String>,
    pub power_off_minutes: Option<u8>,
    pub led_zones_count: u8,
    pub equalizer: Option<EqualizerInfo>,
}

pub struct LogitechHeadset {
    pub(crate) transport: HidppTransport,
    pub info: LogitechHeadsetInfo,
    pub(crate) last_frame: Vec<Rgb>,
}

impl LogitechHeadset {
    /// Determine whether an enumerated HID++ interface on `0xFF43` actually belongs to a headset.
    /// Logitech wired keyboards (e.g. G513) also expose `0xFF43` and must stay out of headsets.
    pub fn is_headset(t: &HidppTransport) -> bool {
        if let Some(dt) = t.read_device_type() {
            return dt == 8;
        }
        if crate::keyboard::LogitechKeyboard::is_keyboard(t) {
            return false;
        }
        let has_dpi = t.get_feature_index(FEAT_ADJUSTABLE_DPI).is_some()
            || t.get_feature_index(FEAT_EXTENDED_ADJUSTABLE_DPI).is_some();
        if has_dpi {
            return false;
        }
        let has_hs_feat = t.get_feature_index(FEAT_SIDETONE).is_some()
            || t.get_feature_index(FEAT_HEADSET_AUDIO_SIDETONE).is_some()
            || t.get_feature_index(FEAT_HEADSET_OUT).is_some()
            || t.get_feature_index(FEAT_HEADSET_MIC_GAIN).is_some()
            || t.get_feature_index(FEAT_EQUALIZER).is_some();
        has_hs_feat
            || (t.proto_major == 0
                && (t.pid == 0x0AAA
                    || t.product_string.to_ascii_lowercase().contains("headset")))
    }

    /// Discover and connect to ALL Logitech HID++ headsets on UsagePage `0xFF43`.
    pub fn connect_all() -> Vec<Self> {
        HidppTransport::open_all_matching(0xFF43, Some(0x0202))
            .into_iter()
            .filter(Self::is_headset)
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
            || transport
                .get_feature_index(FEAT_HEADSET_AUDIO_SIDETONE)
                .is_some()
            || transport.pid == 0x0AAA;

        let mut battery = read_battery_info(&transport);
        let is_wireless = battery.is_some()
            || (transport.device_index != 0xFF && transport.device_index != 0)
            || matches!(
                transport.pid,
                0x0ABA | 0x0AB5 | 0x0AB7 | 0x0AC4 | 0x0A66 | 0x0A5B | 0x0A89
            );
        if let Some(b) = battery.as_mut()
            && let Some(pct) = b.percent
        {
            b.estimated_hours = estimate_rated_battery_hours(transport.pid, &name, pct, None);
        }
        let sidetone_pct = Self::read_sidetone_inner(&transport).unwrap_or(0);
        let equalizer = Self::read_equalizer_inner(&transport);
        let eq_preset = equalizer
            .as_ref()
            .and_then(|eq| Self::match_eq_preset(&eq.gains_db));
        let supports_mic_mute = transport
            .get_feature_index(FEAT_CENTURION_MIC_MUTE)
            .is_some()
            || (transport.get_feature_index(FEAT_SIDETONE).is_some()
                && transport
                    .request_feature(FEAT_SIDETONE, 0x20, &[])
                    .is_ok());
        let mic_muted = Self::read_mic_mute_inner(&transport);
        let bass_tone = Self::read_bass_tone_inner(&transport);
        let power_off_minutes = read_global_setting_u8(&transport, TAG_POWER_OFF_TIMEOUT)
            .or_else(|| Self::read_centurion_sleep_inner(&transport));
        let led_zones_count = Self::read_led_zone_count(&transport);

        let info = LogitechHeadsetInfo {
            vid: transport.vid,
            pid: transport.pid,
            device_index: transport.device_index,
            is_wireless,
            name,
            proto_version,
            firmware,
            features: transport.features.clone(),
            battery,
            supports_sidetone,
            sidetone_pct,
            supports_mic_mute,
            mic_muted,
            bass_tone,
            eq_preset,
            power_off_minutes,
            led_zones_count,
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

        Self {
            transport,
            info,
            last_frame: Vec::new(),
        }
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
            self.transport
                .request_feature(FEAT_SIDETONE, 0x10, &[pct])?;
        } else if self
            .transport
            .get_feature_index(FEAT_HEADSET_AUDIO_SIDETONE)
            .is_some()
        {
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
        t.get_feature_index(FEAT_EQUALIZER)?;
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
            let g = gains
                .get(i)
                .copied()
                .unwrap_or(0)
                .clamp(eq.min_db, eq.max_db);
            payload.push(g as u8);
        }
        self.transport
            .request_feature(FEAT_EQUALIZER, 0x30, &payload)?;
        if let Some(cur) = self.info.equalizer.as_mut() {
            for i in 0..cur.band_count {
                cur.gains_db[i] = gains
                    .get(i)
                    .copied()
                    .unwrap_or(0)
                    .clamp(cur.min_db, cur.max_db);
            }
            self.info.eq_preset = Self::match_eq_preset(&cur.gains_db);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn from_test_transport(transport: HidppTransport) -> Self {
        Self::from_transport(transport)
    }

    pub fn refresh_telemetry(&mut self) {
        self.info.battery = read_battery_info(&self.transport);
        if let Some(b) = self.info.battery.as_mut()
            && let Some(pct) = b.percent
        {
            b.estimated_hours =
                estimate_rated_battery_hours(self.info.pid, &self.info.name, pct, None);
        }
        if let Some(st) = Self::read_sidetone_inner(&self.transport) {
            self.info.sidetone_pct = st;
        }
        if let Some(muted) = Self::read_mic_mute_inner(&self.transport) {
            self.info.mic_muted = Some(muted);
        }
        if let Some(bass) = Self::read_bass_tone_inner(&self.transport) {
            self.info.bass_tone = Some(bass);
        }
        if let Some(mins) = read_global_setting_u8(&self.transport, TAG_POWER_OFF_TIMEOUT)
            .or_else(|| Self::read_centurion_sleep_inner(&self.transport))
        {
            self.info.power_off_minutes = Some(mins);
        }
    }

    fn expand_preset_gains(gains_5: [i8; 5], band_count: usize) -> Vec<i8> {
        if band_count <= 5 {
            return gains_5[..band_count].to_vec();
        }
        let mut out = Vec::with_capacity(band_count);
        for i in 0..band_count {
            let src = (i * 4) / (band_count - 1).max(1);
            out.push(gains_5[src.min(4)]);
        }
        out
    }

    fn match_eq_preset(gains: &[i8]) -> Option<String> {
        let len = gains.len();
        EQ_PRESETS
            .iter()
            .find(|p| Self::expand_preset_gains(p.gains_5, len) == gains)
            .map(|p| p.name.to_string())
    }

    /// Apply a verified G HUB EQ preset by name across 5-band or 10-band hardware EQ.
    pub fn apply_eq_preset(&mut self, preset_name: &str) -> Result<(), HidppError> {
        let preset = EQ_PRESETS
            .iter()
            .find(|p| p.name == preset_name)
            .ok_or(HidppError::Protocol(0x02, "Préréglage EQ inconnu"))?;
        let count = self
            .info
            .equalizer
            .as_ref()
            .map(|e| e.band_count)
            .unwrap_or(5);
        let expanded = Self::expand_preset_gains(preset.gains_5, count);
        self.set_equalizer_gains(&expanded)?;
        self.info.eq_preset = Some(preset.name.to_string());
        Ok(())
    }

    fn read_mic_mute_inner(t: &HidppTransport) -> Option<bool> {
        if let Ok(r) = t.request_feature(FEAT_CENTURION_MIC_MUTE, 0x00, &[]) {
            return r.first().map(|&b| (b & 0x01) != 0);
        }
        if let Ok(r) = t.request_feature(FEAT_SIDETONE, 0x20, &[]) {
            return r.first().map(|&b| b != 0);
        }
        None
    }

    pub fn set_mic_mute(&mut self, muted: bool) -> Result<(), HidppError> {
        if self
            .transport
            .get_feature_index(FEAT_CENTURION_MIC_MUTE)
            .is_some()
        {
            self.transport.request_feature(
                FEAT_CENTURION_MIC_MUTE,
                0x10,
                &[u8::from(muted)],
            )?;
        } else if self.transport.get_feature_index(FEAT_SIDETONE).is_some() {
            self.transport
                .request_feature(FEAT_SIDETONE, 0x30, &[u8::from(muted)])?;
        } else {
            return Err(HidppError::UnsupportedFeature(FEAT_SIDETONE));
        }
        self.info.mic_muted = Some(muted);
        Ok(())
    }

    fn read_bass_tone_inner(t: &HidppTransport) -> Option<u8> {
        let r = t.request_feature(FEAT_BASS_TONE, 0x00, &[]).ok()?;
        r.first().copied()
    }

    pub fn set_bass_tone(&mut self, level: u8) -> Result<(), HidppError> {
        let clamped = level.min(100);
        self.transport
            .request_feature(FEAT_BASS_TONE, 0x10, &[clamped])?;
        self.info.bass_tone = Some(clamped);
        Ok(())
    }

    fn read_centurion_sleep_inner(t: &HidppTransport) -> Option<u8> {
        let r = t
            .request_feature(FEAT_CENTURION_AUTO_SLEEP, 0x00, &[])
            .ok()?;
        r.first().copied()
    }

    pub fn set_power_off_minutes(&mut self, minutes: u8) -> Result<(), HidppError> {
        if self
            .transport
            .get_feature_index(FEAT_GLOBAL_DEVICE_SETTINGS)
            .is_some()
        {
            write_global_setting_u8(&self.transport, TAG_POWER_OFF_TIMEOUT, minutes)?;
        } else if self
            .transport
            .get_feature_index(FEAT_CENTURION_AUTO_SLEEP)
            .is_some()
        {
            self.transport
                .request_feature(FEAT_CENTURION_AUTO_SLEEP, 0x10, &[minutes, 0, 0])?;
        } else {
            return Err(HidppError::UnsupportedFeature(FEAT_GLOBAL_DEVICE_SETTINGS));
        }
        self.info.power_off_minutes = Some(minutes);
        Ok(())
    }

    fn read_led_zone_count(t: &HidppTransport) -> u8 {
        if t.get_feature_index(FEAT_COLOR_LED_EFFECTS).is_some() {
            return t
                .request_feature(FEAT_COLOR_LED_EFFECTS, 0x00, &[])
                .ok()
                .and_then(|r| r.first().copied())
                .unwrap_or(1)
                .clamp(1, 4);
        }
        if t.get_feature_index(FEAT_RGB_EFFECTS).is_some() {
            return t
                .request_feature(FEAT_RGB_EFFECTS, 0x00, &[])
                .ok()
                .and_then(|r| r.first().copied())
                .unwrap_or(1)
                .clamp(1, 4);
        }
        0
    }

    pub fn set_led_zone(
        &mut self,
        zone_idx: u8,
        mode: MouseLedMode,
        color: Rgb,
        speed_ms: u16,
    ) -> Result<(), HidppError> {
        let sp = speed_ms.to_be_bytes();
        let params: [u8; 12] = match mode {
            MouseLedMode::Off => [zone_idx, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            MouseLedMode::Fixed => [zone_idx, 0x01, color.r, color.g, color.b, 0, 0, 0, 0, 0, 0, 0],
            MouseLedMode::Cycle => [zone_idx, 0x03, 0, 0, 0, 0, 0, sp[0], sp[1], 100, 0, 0],
            MouseLedMode::Breathing => {
                [zone_idx, 0x0A, color.r, color.g, color.b, sp[0], sp[1], 0x01, 100, 0, 0, 0]
            }
        };
        write_led_zone_raw(&self.transport, true, &params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_equalizer_info_guards_overflow_and_band_limit() {
        assert_eq!(
            LogitechHeadset::parse_equalizer_info(&[5, 0x80, 0, 0, 0]),
            Some((5, 0, 0))
        );
        assert_eq!(
            LogitechHeadset::parse_equalizer_info(&[5, 12, 0, 0, 0]),
            Some((5, -12, 12))
        );
        assert_eq!(
            LogitechHeadset::parse_equalizer_info(&[5, 6, 0, 10, 2]),
            Some((5, 2, 10))
        );
        assert_eq!(
            LogitechHeadset::parse_equalizer_info(&[16, 12, 0, 0, 0]),
            None
        );
        assert_eq!(
            LogitechHeadset::parse_equalizer_info(&[0, 12, 0, 0, 0]),
            None
        );
    }
}
