use crate::features::*;
use crate::hidpp::{
    BatteryInfo, HidppError, HidppTransport, TAG_POWER_OFF_TIMEOUT, estimate_rated_battery_hours,
    read_battery_info, read_global_setting_u8, write_global_setting_u8,
};
use opengear_core::Rgb;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyboardRgbMode {
    Off,
    Fixed,
    Breathing,
    Cycle,
    Wave,
}

#[derive(Debug, Clone)]
pub struct LogitechKeyboardInfo {
    pub id: String,
    pub vid: u16,
    pub pid: u16,
    pub device_index: u8,
    pub name: String,
    pub proto_version: String,
    pub firmware: Vec<String>,
    pub features: Vec<FeatureEntry>,
    pub battery: Option<BatteryInfo>,
    pub supports_rgb_effects: bool,
    pub rgb_cluster_count: u8,
    pub supports_brightness: bool,
    pub brightness_pct: Option<u8>,
    pub supports_game_mode: bool,
    pub game_mode_enabled: Option<bool>,
    pub supports_fn_inversion: bool,
    pub fn_inversion: Option<bool>,
    pub supports_per_key_rgb: bool,
    pub power_off_minutes: Option<u8>,
}

/// Universal Logitech G Keyboard HID++ 2.0 driver (`0x8071` RGB Effects, `0x8040` Brightness, `0x4521`/`0x4522` Game Mode).
pub struct LogitechKeyboard {
    pub(crate) transport: HidppTransport,
    max_brightness: u16,
    pub info: LogitechKeyboardInfo,
    pub(crate) last_frame: Vec<Rgb>,
}

impl LogitechKeyboard {
    /// Discover and connect to ALL Logitech HID++ 2.0 keyboards on `0xFF00` or `0xFF43`.
    pub fn connect_all() -> Vec<Self> {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for (up, usage) in [(0xFF00u16, Some(0x0002u16)), (0xFF43u16, Some(0x0202u16))] {
            for transport in HidppTransport::open_all_matching(up, usage) {
                if transport.proto_major == 0 || !Self::is_keyboard(&transport) {
                    continue;
                }
                let key = (transport.vid, transport.pid, transport.device_index);
                if seen.insert(key) {
                    out.push(Self::from_transport(transport));
                }
            }
        }
        out
    }

    pub fn is_keyboard(t: &HidppTransport) -> bool {
        // Explicit HID++ 2.0 DeviceType: 0 = Keyboard, 3 = Mouse, 8 = Headset.
        if let Some(dt) = t.read_device_type() {
            return dt == 0;
        }
        // Fallback: has RGB_EFFECTS (0x8071), COLOR_LED_EFFECTS (0x8070), or DISABLE_KEYS
        // and NO mouse DPI or headset audio feature.
        let has_dpi = t.get_feature_index(FEAT_ADJUSTABLE_DPI).is_some()
            || t.get_feature_index(FEAT_EXTENDED_ADJUSTABLE_DPI).is_some();
        let has_hs = t.get_feature_index(FEAT_SIDETONE).is_some()
            || t.get_feature_index(FEAT_HEADSET_AUDIO_SIDETONE).is_some()
            || t.get_feature_index(FEAT_HEADSET_OUT).is_some()
            || t.get_feature_index(FEAT_EQUALIZER).is_some();
        let has_kb_feat = t.get_feature_index(FEAT_RGB_EFFECTS).is_some()
            || t.get_feature_index(FEAT_COLOR_LED_EFFECTS).is_some()
            || t.get_feature_index(FEAT_DISABLE_KEYS).is_some()
            || t.get_feature_index(FEAT_DISABLE_KEYS_BY_USAGE).is_some();
        !has_dpi && !has_hs && has_kb_feat
    }

    fn from_transport(transport: HidppTransport) -> Self {
        let name = transport
            .read_device_name()
            .unwrap_or_else(|| transport.product_string.clone());
        let firmware = transport.read_firmware_versions();
        let proto_version = format!("{}.{}", transport.proto_major, transport.proto_minor);

        let mut battery = read_battery_info(&transport);
        if let Some(b) = battery.as_mut()
            && let Some(pct) = b.percent
        {
            b.estimated_hours = estimate_rated_battery_hours(transport.pid, &name, pct, None);
        }
        let supports_per_key_rgb = transport.get_feature_index(FEAT_PER_KEY_LIGHTING).is_some()
            || transport
                .get_feature_index(FEAT_PER_KEY_LIGHTING_V1)
                .is_some();
        let supports_rgb_effects = transport.get_feature_index(FEAT_RGB_EFFECTS).is_some()
            || transport
                .get_feature_index(FEAT_COLOR_LED_EFFECTS)
                .is_some();
        let rgb_cluster_count = if transport.get_feature_index(FEAT_RGB_EFFECTS).is_some() {
            transport
                .request_feature(FEAT_RGB_EFFECTS, 0x00, &[])
                .ok()
                .and_then(|r| r.first().copied())
                .unwrap_or(1)
        } else {
            1
        };

        let supports_brightness = transport
            .get_feature_index(FEAT_BRIGHTNESS_CONTROL)
            .is_some();
        let max_brightness = if supports_brightness {
            transport
                .request_feature(FEAT_BRIGHTNESS_CONTROL, 0x00, &[])
                .ok()
                .and_then(|r| {
                    if r.len() >= 2 {
                        let m = u16::from_be_bytes([r[0], r[1]]);
                        Some(m.max(1))
                    } else {
                        None
                    }
                })
                .unwrap_or(1000)
        } else {
            1000
        };
        let brightness_pct = if supports_brightness {
            transport
                .request_feature(FEAT_BRIGHTNESS_CONTROL, 0x10, &[])
                .ok()
                .and_then(|r| {
                    if r.len() >= 2 {
                        let raw = u16::from_be_bytes([r[0], r[1]]).min(max_brightness);
                        let pct = ((raw as u32 * 100 + (max_brightness as u32 / 2))
                            / max_brightness as u32)
                            .min(100) as u8;
                        Some(pct)
                    } else {
                        None
                    }
                })
        } else {
            None
        };

        let supports_game_mode = transport.get_feature_index(FEAT_DISABLE_KEYS).is_some()
            || transport
                .get_feature_index(FEAT_DISABLE_KEYS_BY_USAGE)
                .is_some();
        let game_mode_enabled = if transport.get_feature_index(FEAT_DISABLE_KEYS).is_some() {
            transport
                .request_feature(FEAT_DISABLE_KEYS, 0x10, &[0x00])
                .ok()
                .and_then(|r| r.first().map(|&m| (m & 0x07) != 0))
        } else {
            None
        };
        let supports_fn_inversion = transport.get_feature_index(FEAT_FN_INVERSION).is_some()
            || transport
                .get_feature_index(FEAT_MULTI_HOST_FN_INVERSION)
                .is_some();
        let fn_inversion = Self::read_fn_inversion_inner(&transport);
        let power_off_minutes = read_global_setting_u8(&transport, TAG_POWER_OFF_TIMEOUT);

        let id = format!(
            "logi_kb_{:04X}_{:04X}_s{:02X}",
            transport.vid, transport.pid, transport.device_index
        );
        let info = LogitechKeyboardInfo {
            id,
            vid: transport.vid,
            pid: transport.pid,
            device_index: transport.device_index,
            name,
            proto_version,
            firmware,
            features: transport.features.clone(),
            battery,
            supports_rgb_effects,
            rgb_cluster_count,
            supports_brightness,
            brightness_pct,
            supports_game_mode,
            game_mode_enabled,
            supports_fn_inversion,
            fn_inversion,
            supports_per_key_rgb,
            power_off_minutes,
        };
        info!(
            "Clavier Logitech HID++ connecté : {} (slot 0x{:02X}, {} features)",
            info.name,
            info.device_index,
            info.features.len()
        );
        Self {
            transport,
            max_brightness,
            info,
            last_frame: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_transport(
        transport: HidppTransport,
        rgb_cluster_count: u8,
        supports_rgb_effects: bool,
    ) -> Self {
        let id = format!(
            "logi_kb_{:04X}_{:04X}_s{:02X}",
            transport.vid, transport.pid, transport.device_index
        );
        let info = LogitechKeyboardInfo {
            id,
            vid: transport.vid,
            pid: transport.pid,
            device_index: transport.device_index,
            name: "Mock Logitech Keyboard".into(),
            proto_version: "4.5".into(),
            firmware: Vec::new(),
            features: transport.features.clone(),
            battery: None,
            supports_rgb_effects,
            rgb_cluster_count,
            supports_brightness: false,
            brightness_pct: None,
            supports_game_mode: false,
            game_mode_enabled: None,
            supports_fn_inversion: false,
            fn_inversion: None,
            supports_per_key_rgb: false,
            power_off_minutes: None,
        };
        Self {
            transport,
            max_brightness: 1000,
            info,
            last_frame: Vec::new(),
        }
    }

    /// Apply RGB lighting effect to one cluster (for `0x8071`) or zone 0 (for `0x8070`).
    pub fn set_cluster_rgb(
        &mut self,
        cluster: u8,
        mode: KeyboardRgbMode,
        color: Rgb,
        speed_ms: u16,
    ) -> Result<(), HidppError> {
        let sp = speed_ms.to_be_bytes();
        if self.transport.get_feature_index(FEAT_RGB_EFFECTS).is_some() {
            let params: Vec<u8> = match mode {
                KeyboardRgbMode::Off => vec![cluster, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                KeyboardRgbMode::Fixed => {
                    vec![
                        cluster, 0x01, color.r, color.g, color.b, 0, 0, 0, 0, 0, 0, 0,
                    ]
                }
                KeyboardRgbMode::Cycle => {
                    vec![cluster, 0x03, 0, 0, 0, 0, 0, sp[0], sp[1], 100, 0, 0]
                }
                KeyboardRgbMode::Wave => {
                    vec![cluster, 0x04, 0, 0, 0, 0, 0, sp[0], sp[1], 1, 100, 0]
                }
                KeyboardRgbMode::Breathing => vec![
                    cluster, 0x0A, color.r, color.g, color.b, sp[0], sp[1], 0x01, 100, 0, 0, 0,
                ],
            };
            self.transport
                .request_feature(FEAT_RGB_EFFECTS, 0x10, &params)?;
            return Ok(());
        }
        if self
            .transport
            .get_feature_index(FEAT_COLOR_LED_EFFECTS)
            .is_some()
        {
            let params: Vec<u8> = match mode {
                KeyboardRgbMode::Off => vec![0, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                KeyboardRgbMode::Fixed => {
                    vec![0, 0x01, color.r, color.g, color.b, 0, 0, 0, 0, 0, 0, 0]
                }
                KeyboardRgbMode::Cycle | KeyboardRgbMode::Wave => {
                    vec![0, 0x03, 0, 0, 0, 0, 0, sp[0], sp[1], 100, 0, 0]
                }
                KeyboardRgbMode::Breathing => {
                    vec![
                        0, 0x0A, color.r, color.g, color.b, sp[0], sp[1], 0x01, 100, 0, 0, 0,
                    ]
                }
            };
            self.transport
                .request_feature(FEAT_COLOR_LED_EFFECTS, 0x30, &params)?;
            return Ok(());
        }
        Err(HidppError::UnsupportedFeature(FEAT_RGB_EFFECTS))
    }

    /// Apply RGB lighting effect via `0x8071` (`RGB_EFFECTS`) or fallback `0x8070` (`COLOR_LED_EFFECTS`).
    pub fn set_rgb_effect(
        &mut self,
        mode: KeyboardRgbMode,
        color: Rgb,
        speed_ms: u16,
    ) -> Result<(), HidppError> {
        if self.transport.get_feature_index(FEAT_RGB_EFFECTS).is_some() {
            // 0x8071 fn 0x10: setClusterEffect(clusterIdx, effectId, R, G, B, ...)
            let clusters = self.info.rgb_cluster_count.clamp(1, 4);
            for cluster in 0..clusters {
                self.set_cluster_rgb(cluster, mode, color, speed_ms)?;
            }
            return Ok(());
        }
        self.set_cluster_rgb(0, mode, color, speed_ms)
    }

    /// Set keyboard backlight brightness (0..=100%) via `0x8040`.
    pub fn set_brightness_pct(&mut self, pct: u8) -> Result<(), HidppError> {
        let clamped = pct.min(100);
        let raw_val = (((clamped as u32) * (self.max_brightness as u32) + 50) / 100)
            .min(self.max_brightness as u32) as u16;
        let raw = raw_val.to_be_bytes();
        self.transport
            .request_feature(FEAT_BRIGHTNESS_CONTROL, 0x20, &raw)?;
        self.info.brightness_pct = Some(clamped);
        Ok(())
    }

    pub fn refresh_telemetry(&mut self) {
        self.info.battery = read_battery_info(&self.transport);
        if let Some(b) = self.info.battery.as_mut()
            && let Some(pct) = b.percent
        {
            b.estimated_hours =
                estimate_rated_battery_hours(self.info.pid, &self.info.name, pct, None);
        }
        if let Some(fn_inv) = Self::read_fn_inversion_inner(&self.transport) {
            self.info.fn_inversion = Some(fn_inv);
        }
        if let Some(mins) = read_global_setting_u8(&self.transport, TAG_POWER_OFF_TIMEOUT) {
            self.info.power_off_minutes = Some(mins);
        }
    }

    fn read_fn_inversion_inner(t: &HidppTransport) -> Option<bool> {
        if let Ok(r) = t.request_feature(FEAT_FN_INVERSION, 0x00, &[]) {
            return Some(r.first().copied().unwrap_or(0) != 0);
        }
        if let Ok(r) = t.request_feature(FEAT_MULTI_HOST_FN_INVERSION, 0x00, &[0x00]) {
            return Some(r.get(1).copied().unwrap_or(0) != 0);
        }
        None
    }

    /// Toggle Fn key inversion via `0x40A2` or `0x40A3` (host 0).
    pub fn set_fn_inversion(&mut self, inverted: bool) -> Result<(), HidppError> {
        if self
            .transport
            .get_feature_index(FEAT_FN_INVERSION)
            .is_some()
        {
            self.transport
                .request_feature(FEAT_FN_INVERSION, 0x10, &[u8::from(inverted)])?;
        } else if self
            .transport
            .get_feature_index(FEAT_MULTI_HOST_FN_INVERSION)
            .is_some()
        {
            self.transport.request_feature(
                FEAT_MULTI_HOST_FN_INVERSION,
                0x10,
                &[0x00, u8::from(inverted)],
            )?;
        } else {
            return Err(HidppError::UnsupportedFeature(FEAT_FN_INVERSION));
        }
        self.info.fn_inversion = Some(inverted);
        Ok(())
    }

    /// Toggle Game Mode / Windows-key lock via `0x4521` (1-byte mask) or `0x4522` (HID usages `0xE3`/`0xE7`).
    pub fn set_game_mode(&mut self, enabled: bool) -> Result<(), HidppError> {
        if self
            .transport
            .get_feature_index(FEAT_DISABLE_KEYS)
            .is_some()
        {
            // Verified in Feature4521DisableKeys::fn2: 1-byte bitmask (bits 0..2 = WinL, WinR, Menu).
            let mask = if enabled { 0x07 } else { 0x00 };
            self.transport
                .request_feature(FEAT_DISABLE_KEYS, 0x20, &[mask])?;
        } else if self
            .transport
            .get_feature_index(FEAT_DISABLE_KEYS_BY_USAGE)
            .is_some()
        {
            if enabled {
                self.transport.request_feature(
                    FEAT_DISABLE_KEYS_BY_USAGE,
                    0x10,
                    &[0xE3, 0xE7, 0x65],
                )?;
            } else {
                self.transport
                    .request_feature(FEAT_DISABLE_KEYS_BY_USAGE, 0x30, &[])?;
            }
        } else {
            return Err(HidppError::UnsupportedFeature(FEAT_DISABLE_KEYS));
        }
        self.info.game_mode_enabled = Some(enabled);
        Ok(())
    }

    /// Set wireless keyboard sleep / power-off timeout in minutes (`0 = Never`) via `0x8103`.
    pub fn set_power_off_minutes(&mut self, minutes: u8) -> Result<(), HidppError> {
        write_global_setting_u8(&self.transport, TAG_POWER_OFF_TIMEOUT, minutes)?;
        self.info.power_off_minutes = Some(minutes);
        Ok(())
    }

    /// Fill all keys via `0x8080` PerKeyLighting v0 (`fn 4` SetAllKeysToColor + `fn 5` Flush).
    pub fn set_all_per_key_color(&mut self, color: Rgb) -> Result<(), HidppError> {
        self.transport.request_feature(
            FEAT_PER_KEY_LIGHTING,
            0x40,
            &[0x00, 0x00, color.r, color.g, color.b],
        )?;
        self.transport
            .request_feature(FEAT_PER_KEY_LIGHTING, 0x50, &[0x00])?;
        Ok(())
    }

    /// Set per-key colors via `0x8081` PerKeyLighting v1 (`fn 1` batches of 4 tuples + `fn 7` FrameEnd).
    pub fn set_per_key_batch(&mut self, keys: &[(u8, Rgb)]) -> Result<(), HidppError> {
        if self
            .transport
            .get_feature_index(FEAT_PER_KEY_LIGHTING_V1)
            .is_none()
        {
            return Err(HidppError::UnsupportedFeature(FEAT_PER_KEY_LIGHTING_V1));
        }
        for chunk in keys.chunks(4) {
            let mut params = [0xFFu8; 16];
            for (i, &(key_id, col)) in chunk.iter().enumerate() {
                let off = i * 4;
                params[off] = key_id;
                params[off + 1] = col.r;
                params[off + 2] = col.g;
                params[off + 3] = col.b;
            }
            self.transport
                .request_feature(FEAT_PER_KEY_LIGHTING_V1, 0x10, &params)?;
        }
        self.transport.request_feature(
            FEAT_PER_KEY_LIGHTING_V1,
            0x70,
            &[0x01, 0x00, 0x00, 0x00, 0x00],
        )?;
        Ok(())
    }
}
