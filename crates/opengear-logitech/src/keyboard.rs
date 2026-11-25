use crate::features::*;
use crate::hidpp::{BatteryInfo, HidppError, HidppTransport, read_battery_info};
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
}

/// Universal Logitech G Keyboard HID++ 2.0 driver (`0x8071` RGB Effects, `0x8040` Brightness, `0x4521`/`0x4522` Game Mode).
pub struct LogitechKeyboard {
    pub(crate) transport: HidppTransport,
    max_brightness: u16,
    pub info: LogitechKeyboardInfo,
    pub(crate) last_frame: Vec<Rgb>,
}

impl LogitechKeyboard {
    /// Discover and connect to ALL Logitech HID++ 2.0 keyboards on `0xFF00`.
    pub fn connect_all() -> Vec<Self> {
        let mut out = Vec::new();
        for transport in HidppTransport::open_all_matching(0xFF00, Some(0x0002)) {
            if transport.proto_major == 0 {
                continue;
            }
            if !Self::is_keyboard(&transport) {
                continue;
            }
            out.push(Self::from_transport(transport));
        }
        out
    }

    pub fn is_keyboard(t: &HidppTransport) -> bool {
        // Explicit HID++ 2.0 Device Type 0 = Keyboard
        if t.read_device_type() == Some(0) {
            return true;
        }
        // Fallback: has RGB_EFFECTS (0x8071) or DISABLE_KEYS (0x4521/0x4522) and NO mouse DPI feature
        let has_dpi = t.get_feature_index(FEAT_ADJUSTABLE_DPI).is_some()
            || t.get_feature_index(FEAT_EXTENDED_ADJUSTABLE_DPI).is_some();
        let has_kb_feat = t.get_feature_index(FEAT_RGB_EFFECTS).is_some()
            || t.get_feature_index(FEAT_DISABLE_KEYS).is_some()
            || t.get_feature_index(FEAT_DISABLE_KEYS_BY_USAGE).is_some();
        !has_dpi && has_kb_feat
    }

    fn from_transport(transport: HidppTransport) -> Self {
        let name = transport
            .read_device_name()
            .unwrap_or_else(|| transport.product_string.clone());
        let firmware = transport.read_firmware_versions();
        let proto_version = format!("{}.{}", transport.proto_major, transport.proto_minor);

        let battery = read_battery_info(&transport);
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
}
