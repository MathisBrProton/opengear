use crate::features::*;
use crate::hidpp::{HidppError, HidppTransport};
use crate::onboard::{MouseButtonAction, OnboardMemoryDesc, OnboardProfile};
use opengear_core::Rgb;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatteryState {
    Discharging,
    Charging,
    AlmostFull,
    Full,
    SlowCharging,
    Error,
    Unknown(u8),
}

impl BatteryState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discharging => "Sur batterie",
            Self::Charging => "En charge",
            Self::AlmostFull => "Presque pleine",
            Self::Full => "Chargée (100%)",
            Self::SlowCharging => "Charge lente",
            Self::Error => "Erreur charge",
            Self::Unknown(_) => "Inconnu",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryPowerModel {
    pub capacity_mwh: f32,
    pub sensor_mw: f32,
    pub mcu_mw: f32,
    pub radio_mw: f32,
    pub total_mw: f32,
    pub estimated_hours: f32,
}

impl BatteryPowerModel {
    /// Compute remaining runtime from dynamic `battery.xml` power parameters.
    pub fn from_params(
        battery_pct: u8,
        capacity_mwh: f32,
        sensor_mw: f32,
        mcu_mw: f32,
        radio_mw: f32,
    ) -> Self {
        let total_mw = (sensor_mw + mcu_mw + radio_mw).max(0.5);
        let remaining_mwh = capacity_mwh * (battery_pct.min(100) as f32 / 100.0);
        let estimated_hours = remaining_mwh / total_mw;
        Self {
            capacity_mwh,
            sensor_mw,
            mcu_mw,
            radio_mw,
            total_mw,
            estimated_hours,
        }
    }

    /// Fallback exact G HUB power consumption model from `pro_x_wireless_mouse/battery/battery.xml` (`046D:4093`).
    pub fn for_pro_x_wireless(battery_pct: u8, report_rate_hz: u16) -> Self {
        let radio_mw = match report_rate_hz {
            0..=180 => 2.2036,
            181..=350 => 3.0738,
            351..=750 => 4.8142,
            _ => 8.2951,
        };
        Self::from_params(battery_pct, 874.0, 1.5456, 2.3064, radio_mw)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub percent: Option<u8>,
    pub voltage_mv: Option<u16>,
    pub state: BatteryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LodLevel {
    Low = 0,
    Medium = 1,
    High = 2,
}

impl LodLevel {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Low,
            1 => Self::Medium,
            _ => Self::High,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Bas (Low)",
            Self::Medium => "Moyen (Medium)",
            Self::High => "Élevé (High)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpiSensorInfo {
    pub extended: bool,
    pub dpi_x: u16,
    pub dpi_y: u16,
    pub default_dpi: u16,
    pub min_dpi: u16,
    pub max_dpi: u16,
    pub step_dpi: u16,
    pub supports_independent_xy: bool,
    pub supports_lod: bool,
    pub lod: Option<LodLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRateInfo {
    pub extended: bool,
    pub current_hz: u16,
    pub supported_hz: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnboardMode {
    Onboard,
    Host,
    Unknown(u8),
}

impl OnboardMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Onboard => "Mémoire embarquée (Onboard)",
            Self::Host => "Mode Logiciel (Host / OpenGear)",
            Self::Unknown(_) => "Inconnu",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardProfilesSummary {
    pub mode: OnboardMode,
    pub profile_count: u8,
    pub button_count: u8,
    pub active_profile: u16,
    pub active_dpi_index: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseLedMode {
    Off,
    Fixed,
    Breathing,
    Cycle,
}

impl MouseLedMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Éteint",
            Self::Fixed => "Statique",
            Self::Breathing => "Respiration",
            Self::Cycle => "Cycle couleur",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedZoneInfo {
    pub index: u8,
    pub location: String,
    pub num_effects: u8,
}

#[derive(Debug, Clone)]
pub struct LogitechMouseInfo {
    pub vid: u16,
    pub pid: u16,
    pub device_index: u8,
    pub name: String,
    pub proto_version: String,
    pub firmware: Vec<String>,
    pub features: Vec<FeatureEntry>,
    pub battery: Option<BatteryInfo>,
    pub dpi: Option<DpiSensorInfo>,
    pub report_rate: Option<ReportRateInfo>,
    pub onboard: Option<OnboardProfilesSummary>,
    pub onboard_desc: Option<OnboardMemoryDesc>,
    pub onboard_profiles: Vec<OnboardProfile>,
    pub angle_snapping: Option<bool>,
    pub led_zones: Vec<LedZoneInfo>,
}

pub struct LogitechMouse {
    transport: HidppTransport,
    pub info: LogitechMouseInfo,
}

impl LogitechMouse {
    /// Connect to ALL connected Logitech HID++ 2.0 mice (LIGHTSPEED, Unifying, Bolt, wired) simultaneously!
    pub fn connect_all() -> Vec<Self> {
        let mut out = Vec::new();
        for transport in HidppTransport::open_all_matching(0xFF00, Some(0x0002)) {
            if transport.proto_major == 0 {
                continue;
            }
            // Skip keyboards (classified separately by LogitechKeyboard)
            if crate::keyboard::LogitechKeyboard::is_keyboard(&transport) {
                continue;
            }
            out.push(Self::from_transport(transport));
        }
        out
    }

    /// Connect to the first Logitech LIGHTSPEED / Unifying / Wired mouse on Usage Page `0xFF00`.
    pub fn connect() -> Result<Self, HidppError> {
        let transport = HidppTransport::open_matching(None, 0xFF00, Some(0x0002))?;
        if transport.proto_major == 0 {
            return Err(HidppError::DeviceNotFound);
        }
        Ok(Self::from_transport(transport))
    }

    fn from_transport(transport: HidppTransport) -> Self {
        let name = transport
            .read_device_name()
            .unwrap_or_else(|| transport.product_string.clone());
        let firmware = transport.read_firmware_versions();
        let proto_version = format!("{}.{}", transport.proto_major, transport.proto_minor);

        let battery = Self::read_battery_inner(&transport);
        let dpi = Self::read_dpi_inner(&transport);
        let report_rate = Self::read_report_rate_inner(&transport);
        let onboard = Self::read_onboard_inner(&transport);
        let (onboard_desc, onboard_profiles) = if onboard.is_some() {
            match OnboardProfile::read_all(&transport) {
                Ok((desc, profs)) => (Some(desc), profs),
                Err(_) => (None, Vec::new()),
            }
        } else {
            (None, Vec::new())
        };
        let angle_snapping = Self::read_angle_snapping_inner(&transport);
        let led_zones = Self::read_led_zones_inner(&transport);

        let info = LogitechMouseInfo {
            vid: transport.vid,
            pid: transport.pid,
            device_index: transport.device_index,
            name,
            proto_version,
            firmware,
            features: transport.features.clone(),
            battery,
            dpi,
            report_rate,
            onboard,
            onboard_desc,
            onboard_profiles,
            angle_snapping,
            led_zones,
        };

        info!(
            "Logitech HID++ connecté : {} (slot 0x{:02X}, HID++ {}, {} features)",
            info.name,
            info.device_index,
            info.proto_version,
            info.features.len()
        );

        Self { transport, info }
    }

    /// Flash an onboard hardware macro into a free flash sector and bind it to `button_idx` in `profile_idx`!
    pub fn flash_onboard_macro(
        &mut self,
        profile_idx: usize,
        button_idx: usize,
        steps: &[crate::onboard::OnboardMacroStep],
    ) -> Result<(), HidppError> {
        let desc = self
            .info
            .onboard_desc
            .clone()
            .ok_or(HidppError::UnsupportedFeature(FEAT_ONBOARD_PROFILES))?;
        // Validate the target BEFORE writing anything to flash.
        let prof = self
            .info
            .onboard_profiles
            .get(profile_idx)
            .ok_or(HidppError::Protocol(0x02, "Index profil hors limites"))?;
        if button_idx >= prof.buttons.len() {
            return Err(HidppError::Protocol(0x02, "Index bouton hors limites"));
        }
        let own_sector = match prof.buttons[button_idx] {
            MouseButtonAction::OnboardMacro { sector, .. } => Some(sector as u16),
            _ => None,
        };
        // Every macro sector referenced by any loaded profile (incl. G HUB's) is off-limits.
        let referenced: Vec<u16> = self
            .info
            .onboard_profiles
            .iter()
            .flat_map(|p| p.buttons.iter().chain(p.gshift_buttons.iter()))
            .filter_map(|b| match b {
                MouseButtonAction::OnboardMacro { sector, .. } => Some(*sector as u16),
                _ => None,
            })
            .collect();
        let first = desc.profile_count as u16 + 1;
        let end = (desc.sector_count as u16).min(0x0100);
        let transport = &self.transport;
        let (macro_sector, replace_existing) =
            crate::onboard::pick_macro_sector(first, end, &referenced, own_sector, |s| {
                OnboardProfile::read_sector(transport, s, desc.sector_size)
                    .map(|raw| crate::onboard::is_blank(&raw))
            })?;
        let action = OnboardProfile::write_macro_sector(
            &self.transport,
            &desc,
            macro_sector,
            steps,
            replace_existing,
        )?;
        self.info.onboard_profiles[profile_idx].buttons[button_idx] = action;
        self.flash_onboard_profile(profile_idx)?;
        Ok(())
    }

    /// Restore a sector backup written by OpenGear before a flash
    /// (`%APPDATA%\OpenGear\backups\logitech\sec<SSSS>_*.bin`), then reload the profiles.
    pub fn restore_onboard_backup(&mut self, path: &std::path::Path) -> Result<(), HidppError> {
        let desc = self
            .info
            .onboard_desc
            .clone()
            .ok_or(HidppError::UnsupportedFeature(FEAT_ONBOARD_PROFILES))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let sector = crate::onboard::parse_backup_sector(name).ok_or_else(|| {
            HidppError::Flash(format!("nom de sauvegarde non reconnu : {name}"))
        })?;
        let bytes = std::fs::read(path)
            .map_err(|e| HidppError::Flash(format!("lecture de {} impossible : {e}", path.display())))?;
        OnboardProfile::restore_sector_backup(&self.transport, &desc, sector, &bytes)?;
        self.reload_onboard_profiles()
    }

    pub fn refresh_telemetry(&mut self) {
        self.info.battery = Self::read_battery_inner(&self.transport);
        if let Some(dpi) = Self::read_dpi_inner(&self.transport) {
            self.info.dpi = Some(dpi);
        }
        if let Some(rr) = Self::read_report_rate_inner(&self.transport) {
            self.info.report_rate = Some(rr);
        }
        if let Some(ob) = Self::read_onboard_inner(&self.transport) {
            self.info.onboard = Some(ob);
        }
    }

    fn read_battery_inner(t: &HidppTransport) -> Option<BatteryInfo> {
        // 1. Prefer 0x1004 Unified Battery
        if let Ok(r) = t.request_feature(FEAT_UNIFIED_BATTERY, 0x10, &[]) {
            if r.len() >= 3 {
                let pct = r[0];
                let status = match r[2] {
                    0 => BatteryState::Discharging,
                    1 => BatteryState::Charging,
                    2 => BatteryState::SlowCharging,
                    3 => BatteryState::Full,
                    4 => BatteryState::AlmostFull,
                    other => BatteryState::Unknown(other),
                };
                return Some(BatteryInfo {
                    percent: if pct <= 100 { Some(pct) } else { None },
                    voltage_mv: None,
                    state: status,
                });
            }
        }
        // 2. Fallback 0x1000 Battery Status
        if let Ok(r) = t.request_feature(FEAT_BATTERY_STATUS, 0x00, &[]) {
            if r.len() >= 3 {
                let pct = r[0];
                let status = match r[2] {
                    0 => BatteryState::Discharging,
                    1 => BatteryState::Charging,
                    2 => BatteryState::AlmostFull,
                    3 => BatteryState::Full,
                    4 => BatteryState::SlowCharging,
                    5..=7 => BatteryState::Error,
                    other => BatteryState::Unknown(other),
                };
                return Some(BatteryInfo {
                    percent: Some(pct.min(100)),
                    voltage_mv: None,
                    state: status,
                });
            }
        }
        // 3. Fallback 0x1001 Battery Voltage
        if let Ok(r) = t.request_feature(FEAT_BATTERY_VOLTAGE, 0x00, &[]) {
            if r.len() >= 3 {
                let mv = u16::from_be_bytes([r[0], r[1]]);
                let flags = r[2];
                let state = if (flags & 0x80) != 0 {
                    BatteryState::Charging
                } else {
                    BatteryState::Discharging
                };
                // Rough Li-Po estimation 3500..4180 mV
                let pct = ((mv.saturating_sub(3500) as f32 / 680.0) * 100.0).clamp(0.0, 100.0) as u8;
                return Some(BatteryInfo {
                    percent: Some(pct),
                    voltage_mv: Some(mv),
                    state,
                });
            }
        }
        None
    }

    fn parse_dpi_list(bytes: &[u8]) -> (u16, u16, u16) {
        let mut min_dpi = u16::MAX;
        let mut max_dpi = 0u16;
        let mut step_dpi = 50u16;
        let mut i = 0;
        while i + 1 < bytes.len() {
            let v = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
            if v == 0 {
                break;
            }
            if v > 0xE000 {
                step_dpi = (v - 0xE000).max(1);
            } else {
                min_dpi = min_dpi.min(v);
                max_dpi = max_dpi.max(v);
            }
            i += 2;
        }
        if min_dpi == u16::MAX {
            min_dpi = 100;
        }
        if max_dpi == 0 {
            max_dpi = 25600;
        }
        (min_dpi, max_dpi, step_dpi)
    }

    fn read_dpi_inner(t: &HidppTransport) -> Option<DpiSensorInfo> {
        // 1. Try 0x2202 Extended Adjustable DPI (HERO 2: X/Y + LOD!)
        if t.get_feature_index(FEAT_EXTENDED_ADJUSTABLE_DPI).is_some() {
            let caps = t.request_feature(FEAT_EXTENDED_ADJUSTABLE_DPI, 0x10, &[0x00]).ok()?;
            let has_y = caps.get(2).map_or(false, |&b| (b & 0x01) != 0);
            let has_lod = caps.get(2).map_or(false, |&b| (b & 0x02) != 0);

            let list_resp = t
                .request_feature(FEAT_EXTENDED_ADJUSTABLE_DPI, 0x20, &[0x00, 0x00])
                .unwrap_or_default();
            let (min_dpi, max_dpi, step_dpi) = if list_resp.len() >= 5 {
                Self::parse_dpi_list(&list_resp[3..])
            } else {
                (100, 32000, 50)
            };

            let cur = t.request_feature(FEAT_EXTENDED_ADJUSTABLE_DPI, 0x50, &[0x00]).ok()?;
            debug!("0x2202 getDpi raw: {:02X?}", cur);
            if cur.len() >= 3 {
                // Byte 0 = sensor index, bytes 1..3 = X DPI, bytes 3..5 = Y DPI (if supported), byte 5 = LOD
                let dpi_x = u16::from_be_bytes([cur[1], cur[2]]);
                let dpi_y = if has_y && cur.len() >= 5 {
                    let y = u16::from_be_bytes([cur[3], cur[4]]);
                    if y > 0 { y } else { dpi_x }
                } else {
                    dpi_x
                };
                let lod = if has_lod && cur.len() >= 6 {
                    Some(LodLevel::from_u8(cur[5]))
                } else {
                    None
                };
                return Some(DpiSensorInfo {
                    extended: true,
                    dpi_x,
                    dpi_y,
                    default_dpi: 800,
                    min_dpi,
                    max_dpi,
                    step_dpi,
                    supports_independent_xy: has_y,
                    supports_lod: has_lod,
                    lod,
                });
            }
        }

        // 2. Standard 0x2201 Adjustable DPI
        if t.get_feature_index(FEAT_ADJUSTABLE_DPI).is_some() {
            let list_resp = t
                .request_feature(FEAT_ADJUSTABLE_DPI, 0x10, &[0x00])
                .unwrap_or_default();
            let (min_dpi, max_dpi, step_dpi) = if list_resp.len() >= 3 {
                Self::parse_dpi_list(&list_resp[1..])
            } else {
                (100, 25600, 50)
            };

            let cur = t.request_feature(FEAT_ADJUSTABLE_DPI, 0x20, &[0x00]).ok()?;
            if cur.len() >= 5 {
                let dpi = u16::from_be_bytes([cur[1], cur[2]]);
                let def = u16::from_be_bytes([cur[3], cur[4]]);
                return Some(DpiSensorInfo {
                    extended: false,
                    dpi_x: dpi,
                    dpi_y: dpi,
                    default_dpi: def,
                    min_dpi,
                    max_dpi,
                    step_dpi,
                    supports_independent_xy: false,
                    supports_lod: false,
                    lod: None,
                });
            }
        }
        None
    }

    pub fn set_dpi(&mut self, dpi_x: u16, dpi_y: Option<u16>, lod: Option<LodLevel>) -> Result<(), HidppError> {
        let Some(ref info) = self.info.dpi.clone() else {
            return Err(HidppError::UnsupportedFeature(FEAT_ADJUSTABLE_DPI));
        };

        let clamped_x = dpi_x.clamp(info.min_dpi, info.max_dpi);
        let clamped_y = dpi_y.unwrap_or(clamped_x).clamp(info.min_dpi, info.max_dpi);

        if info.extended {
            let x_b = clamped_x.to_be_bytes();
            let mut params = vec![0x00, x_b[0], x_b[1]];
            if info.supports_independent_xy {
                let y_b = clamped_y.to_be_bytes();
                params.push(y_b[0]);
                params.push(y_b[1]);
            }
            if info.supports_lod {
                let l = lod.or(info.lod).unwrap_or(LodLevel::Medium) as u8;
                params.push(l);
            }
            self.transport
                .request_feature(FEAT_EXTENDED_ADJUSTABLE_DPI, 0x60, &params)?;
        } else {
            let x_b = clamped_x.to_be_bytes();
            self.transport
                .request_feature(FEAT_ADJUSTABLE_DPI, 0x30, &[0x00, x_b[0], x_b[1]])?;
        }

        if let Some(d) = self.info.dpi.as_mut() {
            d.dpi_x = clamped_x;
            d.dpi_y = if d.supports_independent_xy { clamped_y } else { clamped_x };
            if d.supports_lod {
                if let Some(l) = lod {
                    d.lod = Some(l);
                }
            }
        }
        Ok(())
    }

    fn ext_rate_idx_to_hz(idx: u8) -> Option<u16> {
        match idx {
            0 => Some(125),
            1 => Some(250),
            2 => Some(500),
            3 => Some(1000),
            4 => Some(2000),
            5 => Some(4000),
            6 => Some(8000),
            _ => None,
        }
    }

    fn hz_to_ext_rate_idx(hz: u16) -> Option<u8> {
        match hz {
            125 => Some(0),
            250 => Some(1),
            500 => Some(2),
            1000 => Some(3),
            2000 => Some(4),
            4000 => Some(5),
            8000 => Some(6),
            _ => None,
        }
    }

    fn read_report_rate_inner(t: &HidppTransport) -> Option<ReportRateInfo> {
        // 1. Try 0x8061 Extended Adjustable Report Rate (up to 4K/8K Hz)
        if t.get_feature_index(FEAT_EXTENDED_REPORT_RATE).is_some() {
            let list_r = t.request_feature(FEAT_EXTENDED_REPORT_RATE, 0x10, &[]).ok()?;
            let flags = u16::from_be_bytes([
                list_r.first().copied().unwrap_or(0),
                list_r.get(1).copied().unwrap_or(0),
            ]);
            let mut supported = Vec::new();
            for bit in 0..7u8 {
                if (flags & (1 << bit)) != 0 {
                    if let Some(hz) = Self::ext_rate_idx_to_hz(bit) {
                        supported.push(hz);
                    }
                }
            }
            let cur_r = t.request_feature(FEAT_EXTENDED_REPORT_RATE, 0x20, &[]).ok()?;
            let cur_hz = cur_r
                .first()
                .copied()
                .and_then(Self::ext_rate_idx_to_hz)
                .unwrap_or(1000);
            return Some(ReportRateInfo {
                extended: true,
                current_hz: cur_hz,
                supported_hz: supported,
            });
        }

        // 2. Fallback 0x8060 Adjustable Report Rate (125..1000 Hz)
        if t.get_feature_index(FEAT_REPORT_RATE).is_some() {
            let list_r = t.request_feature(FEAT_REPORT_RATE, 0x00, &[]).ok()?;
            let flags = list_r.first().copied().unwrap_or(0);
            let mut supported = Vec::new();
            for ms in [8u8, 4, 2, 1] {
                if (flags & (1 << (ms - 1))) != 0 {
                    supported.push(1000 / ms as u16);
                }
            }
            let cur_r = t.request_feature(FEAT_REPORT_RATE, 0x10, &[]).ok()?;
            let cur_ms = cur_r.first().copied().unwrap_or(1).max(1);
            return Some(ReportRateInfo {
                extended: false,
                current_hz: 1000 / cur_ms as u16,
                supported_hz: supported,
            });
        }
        None
    }

    pub fn set_report_rate_hz(&mut self, hz: u16) -> Result<(), HidppError> {
        let Some(ref rr) = self.info.report_rate.clone() else {
            return Err(HidppError::UnsupportedFeature(FEAT_REPORT_RATE));
        };
        if rr.extended {
            let idx = Self::hz_to_ext_rate_idx(hz).ok_or(HidppError::Protocol(0x02, "Hz invalide"))?;
            self.transport
                .request_feature(FEAT_EXTENDED_REPORT_RATE, 0x30, &[idx])?;
        } else {
            let ms = match hz {
                125 => 8,
                250 => 4,
                500 => 2,
                1000 => 1,
                _ => return Err(HidppError::Protocol(0x02, "Hz non supporté par 0x8060")),
            };
            self.transport.request_feature(FEAT_REPORT_RATE, 0x20, &[ms])?;
        }
        if let Some(r) = self.info.report_rate.as_mut() {
            r.current_hz = hz;
        }
        Ok(())
    }

    fn read_onboard_inner(t: &HidppTransport) -> Option<OnboardProfilesSummary> {
        if t.get_feature_index(FEAT_ONBOARD_PROFILES).is_none() {
            return None;
        }
        let desc = t.request_feature(FEAT_ONBOARD_PROFILES, 0x00, &[]).ok()?;
        let profile_count = desc.get(3).copied().unwrap_or(0);
        let button_count = desc.get(5).copied().unwrap_or(0);

        let mode_r = t.request_feature(FEAT_ONBOARD_PROFILES, 0x20, &[]).ok()?;
        let mode = match mode_r.first().copied().unwrap_or(0) {
            1 => OnboardMode::Onboard,
            2 => OnboardMode::Host,
            other => OnboardMode::Unknown(other),
        };

        let prof_r = t
            .request_feature(FEAT_ONBOARD_PROFILES, 0x40, &[])
            .unwrap_or_default();
        let active_profile = if prof_r.len() >= 2 {
            u16::from_be_bytes([prof_r[0], prof_r[1]])
        } else {
            1
        };

        let dpi_idx = t
            .request_feature(FEAT_ONBOARD_PROFILES, 0xB0, &[])
            .ok()
            .and_then(|r| r.first().copied())
            .unwrap_or(0);

        Some(OnboardProfilesSummary {
            mode,
            profile_count,
            button_count,
            active_profile,
            active_dpi_index: dpi_idx,
        })
    }

    pub fn set_onboard_mode(&mut self, mode: OnboardMode) -> Result<(), HidppError> {
        let byte = match mode {
            OnboardMode::Onboard => 0x01,
            OnboardMode::Host => 0x02,
            OnboardMode::Unknown(b) => b,
        };
        self.transport
            .request_feature(FEAT_ONBOARD_PROFILES, 0x10, &[byte])?;
        if let Some(ob) = self.info.onboard.as_mut() {
            ob.mode = mode;
        }
        Ok(())
    }

    pub fn set_active_profile(&mut self, profile_1based: u16) -> Result<(), HidppError> {
        let b = profile_1based.to_be_bytes();
        self.transport
            .request_feature(FEAT_ONBOARD_PROFILES, 0x30, &[b[0], b[1]])?;
        if let Some(ob) = self.info.onboard.as_mut() {
            ob.active_profile = profile_1based;
        }
        Ok(())
    }

    pub fn set_active_dpi_stage(&mut self, stage_0based: u8) -> Result<(), HidppError> {
        self.transport
            .request_feature(FEAT_ONBOARD_PROFILES, 0xC0, &[stage_0based])?;
        if let Some(ob) = self.info.onboard.as_mut() {
            ob.active_dpi_index = stage_0based;
        }
        Ok(())
    }

    /// Reload all onboard profiles directly from mouse flash / ROM (`0x8100`).
    pub fn reload_onboard_profiles(&mut self) -> Result<(), HidppError> {
        let (desc, profs) = OnboardProfile::read_all(&self.transport)?;
        self.info.onboard_desc = Some(desc);
        self.info.onboard_profiles = profs;
        Ok(())
    }

    /// Flash an edited onboard profile into the mouse flash memory with CRC-16 CCITT + read-back verify.
    pub fn flash_onboard_profile(&mut self, profile_idx_0based: usize) -> Result<(), HidppError> {
        let desc = self
            .info
            .onboard_desc
            .clone()
            .ok_or(HidppError::UnsupportedFeature(FEAT_ONBOARD_PROFILES))?;
        if profile_idx_0based >= self.info.onboard_profiles.len() {
            return Err(HidppError::Protocol(0x02, "Index profil hors limites"));
        }
        let enabled_flags: Vec<bool> = self.info.onboard_profiles.iter().map(|p| p.enabled).collect();
        self.info.onboard_profiles[profile_idx_0based].flash_to_mouse(
            &self.transport,
            &desc,
            &enabled_flags,
        )?;
        // Re-select active profile so firmware reloads the newly flashed parameters immediately
        let slot = self.info.onboard_profiles[profile_idx_0based].slot_index as u16;
        let _ = self.set_active_profile(slot);
        self.refresh_telemetry();
        Ok(())
    }

    fn read_angle_snapping_inner(t: &HidppTransport) -> Option<bool> {
        let r = t.request_feature(FEAT_ANGLE_SNAPPING, 0x00, &[]).ok()?;
        Some(r.first().copied().unwrap_or(0) != 0)
    }

    pub fn set_angle_snapping(&mut self, enabled: bool) -> Result<(), HidppError> {
        self.transport
            .request_feature(FEAT_ANGLE_SNAPPING, 0x10, &[u8::from(enabled)])?;
        self.info.angle_snapping = Some(enabled);
        Ok(())
    }

    fn read_led_zones_inner(t: &HidppTransport) -> Vec<LedZoneInfo> {
        let mut zones = Vec::new();
        if t.get_feature_index(FEAT_COLOR_LED_EFFECTS).is_some() {
            if let Ok(info_r) = t.request_feature(FEAT_COLOR_LED_EFFECTS, 0x00, &[]) {
                let count = info_r.first().copied().unwrap_or(0).min(8);
                for z in 0..count {
                    if let Ok(zr) = t.request_feature(FEAT_COLOR_LED_EFFECTS, 0x10, &[z]) {
                        let loc_id = u16::from_be_bytes([
                            zr.get(1).copied().unwrap_or(0),
                            zr.get(2).copied().unwrap_or(0),
                        ]);
                        let loc = match loc_id {
                            1 => "Primaire",
                            2 => "Logo G",
                            3 => "Côté gauche",
                            4 => "Côté droit",
                            5 => "Combiné",
                            _ => "Zone LED",
                        };
                        zones.push(LedZoneInfo {
                            index: z,
                            location: loc.to_string(),
                            num_effects: zr.get(3).copied().unwrap_or(0),
                        });
                    }
                }
            }
        }
        zones
    }

    /// Set Color LED effect on a zone via `0x8070` (`fn 0x30` setZoneEffect).
    pub fn set_led_zone(&mut self, zone_idx: u8, mode: MouseLedMode, color: Rgb, speed_ms: u16) -> Result<(), HidppError> {
        if self.transport.get_feature_index(FEAT_COLOR_LED_EFFECTS).is_none() {
            return Err(HidppError::UnsupportedFeature(FEAT_COLOR_LED_EFFECTS));
        }
        let sp = speed_ms.to_be_bytes();
        let params: Vec<u8> = match mode {
            MouseLedMode::Off => vec![zone_idx, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            MouseLedMode::Fixed => vec![zone_idx, 0x01, color.r, color.g, color.b, 0, 0, 0, 0, 0, 0, 0],
            MouseLedMode::Cycle => vec![zone_idx, 0x03, 0, 0, 0, 0, 0, sp[0], sp[1], 100, 0, 0],
            MouseLedMode::Breathing => vec![zone_idx, 0x0A, color.r, color.g, color.b, sp[0], sp[1], 0x01, 100, 0, 0, 0],
        };
        self.transport
            .request_feature(FEAT_COLOR_LED_EFFECTS, 0x30, &params)?;
        Ok(())
    }
}
