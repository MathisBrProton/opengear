use crate::k70_tkl::LightingEndpoint;
use crate::keymap::{K70_TKL_KEYS, K70_TKL_LED_COUNT};
use crate::proto::*;
use crate::transport::{BragiHidCandidate, BragiTransport};
use opengear_core::{KeyboardLayout, OperatingMode, PollingRate, Rgb, RgbEffectConfig};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorsairDeviceKind {
    Keyboard,
    Mouse,
    Headset,
    CoolerHub,
    Dongle,
    Generic,
}

impl CorsairDeviceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Keyboard => "Clavier",
            Self::Mouse => "Souris",
            Self::Headset => "Casque Audio",
            Self::CoolerHub => "Refroidissement / Hub",
            Self::Dongle => "Récepteur SLIPSTREAM",
            Self::Generic => "Périphérique Bragi",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsairMouseCaps {
    pub dpi_stages: [u16; 5],
    pub sniper_dpi: Option<u16>,
    pub active_stage: u8,
    pub current_dpi_x: Option<u16>,
    pub current_dpi_y: Option<u16>,
    pub lift_height: Option<u8>,
    pub motion_sync: Option<bool>,
    pub angle_snapping: Option<bool>,
    pub auto_sleep_enabled: Option<bool>,
    pub auto_sleep_min: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsairHeadsetCaps {
    pub sidetone_muted: Option<bool>,
    pub sidetone_pct: Option<u8>,
    pub mic_muted: Option<bool>,
    pub active_eq_preset: Option<u8>,
    pub anc_mode: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsairCoolerCaps {
    pub product_line_id: Option<u32>,
    pub pump_version_id: Option<u32>,
    pub radiator_size: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CorsairDeviceInfo {
    pub id: String,
    pub vid: u16,
    pub pid: u16,
    pub subdevice_id: u8,
    pub product_name: String,
    pub serial: String,
    pub kind: CorsairDeviceKind,
    pub firmware_version: String,
    pub bootloader_version: String,
    pub layout: KeyboardLayout,
    pub mode: OperatingMode,
    pub pollrate: Option<PollingRate>,
    pub max_pollrate: Option<PollingRate>,
    pub hw_brightness_raw: Option<u32>,
    pub brightness_is_coarse: bool,
    pub out_report_len: usize,
    pub in_report_len: usize,
    pub lighting_endpoint: LightingEndpoint,
    pub battery_pct: Option<u8>,
    pub battery_status: Option<u8>,
    // Keyboard Bragi v2 properties
    pub hw_winlock: Option<bool>,
    pub winlock_shortcuts_mask: Option<u8>,
    pub debounce_enabled: Option<bool>,
    pub debounce_timeout_ms: Option<u8>,
    pub esports_switch_on: Option<bool>,
    pub active_hw_profile: Option<u8>,
    pub storage_total_kb: Option<u32>,
    pub storage_free_kb: Option<u32>,
    pub flashtap_supported: bool,
    /// Current FlashTap (hardware SOCD) state, when the firmware exposes it as a 0/1 switch.
    pub flashtap_enabled: Option<bool>,
    /// Raw answer to the FlashTap property probe (value or error), for diagnostics.
    pub flashtap_probe: String,
    pub rapid_trigger_supported: bool,
    // Onboard Flash HWP profiles decoded from Bragi filesystem (0x000F -> 0xF11E -> 0x6C69)
    pub hwp_profiles: Vec<crate::hwp::HwpProfileInfo>,
    // Mouse / Headset / Cooler capabilities
    pub mouse: Option<CorsairMouseCaps>,
    pub headset: Option<CorsairHeadsetCaps>,
    pub cooler: Option<CorsairCoolerCaps>,
}

/// Universal Corsair Bragi v2 device driver — automatically discovers & adapts to
/// keyboards, mice, headsets, wireless SLIPSTREAM subdevices, and coolers/hubs.
pub struct CorsairDevice {
    transport: BragiTransport,
    pub info: CorsairDeviceInfo,
    lighting_handle_open: bool,
    last_keepalive: Instant,
    restore_hw_on_drop: bool,
}

impl CorsairDevice {
    /// Discover and connect to ALL connected Corsair Bragi v2 devices (wired + SLIPSTREAM wireless subdevices).
    pub fn connect_all() -> Vec<Self> {
        let mut devices = Vec::new();
        let Ok(candidates) = BragiTransport::enumerate_candidates() else {
            return devices;
        };

        for cand in candidates {
            // 1. Open primary host/wired interface (subdevice 0)
            match Self::open_and_probe(&cand, 0) {
                Ok((dev, subdev_bitmap)) => {
                    let is_dongle = dev.info.kind == CorsairDeviceKind::Dongle;
                    if !is_dongle || subdev_bitmap == 0 {
                        devices.push(dev);
                    } else {
                        // Still include dongle if user wants visibility, plus every connected wireless subdevice
                        devices.push(dev);
                    }
                    // 2. Probe any connected SLIPSTREAM wireless subdevices (bits 1..=7 in 0x36)
                    if subdev_bitmap != 0 {
                        for slot in 1..=7u8 {
                            if (subdev_bitmap & (1u32 << slot)) != 0 {
                                if let Ok((sub_dev, _)) = Self::open_and_probe(&cand, slot) {
                                    devices.push(sub_dev);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!(
                        "Corsair candidate {:04X}:{:04X} ignoré : {e}",
                        cand.vid, cand.pid
                    );
                }
            }
        }
        devices
    }

    fn open_and_probe(
        cand: &BragiHidCandidate,
        subdevice_id: u8,
    ) -> Result<(Self, u32), BragiError> {
        let mut transport = BragiTransport::open_candidate(cand, subdevice_id)?;

        // Probe real device VID/PID (essential for wireless subdevices behind a SLIPSTREAM dongle!)
        let vid = transport
            .get_property(PROP_VID)
            .ok()
            .filter(|&v| v > 0 && v <= 0xFFFF)
            .map(|v| v as u16)
            .unwrap_or(transport.vid);
        let pid = transport
            .get_property(PROP_PID)
            .ok()
            .filter(|&v| v > 0 && v <= 0xFFFF)
            .map(|v| v as u16)
            .unwrap_or(transport.pid);

        let initial_mode_raw = transport.get_property(PROP_MODE).unwrap_or(MODE_HARDWARE as u32);
        let mode = if initial_mode_raw == MODE_SOFTWARE as u32 {
            OperatingMode::Software
        } else {
            OperatingMode::Hardware
        };

        let fw_raw = transport.get_property(PROP_APP_VER).unwrap_or(0);
        let bld_raw = transport.get_property(PROP_BLD_VER).unwrap_or(0);
        let layout_raw = transport.get_property(PROP_HWLAYOUT).ok();
        let poll_raw = transport.get_property(PROP_POLLRATE).ok();
        let max_poll_raw = transport.get_property(PROP_MAX_POLLRATE).ok();

        let (hw_brightness_raw, brightness_is_coarse) = match transport.get_property(PROP_BRIGHTNESS) {
            Ok(v) => (Some(v), false),
            Err(_) => match transport.get_property(PROP_BRIGHTNESS_COARSE) {
                Ok(v) => (Some(v), true),
                Err(_) => (None, false),
            },
        };

        let battery_pct = transport
            .get_property(PROP_BATTERY_LEVEL)
            .ok()
            .map(|v| ((v.min(1000)) / 10) as u8);
        let battery_status = transport.get_property(PROP_BATTERY_STATUS).ok().map(|v| v as u8);

        let subdev_bitmap = if subdevice_id == 0 {
            transport.get_property(PROP_CONNECTED_SUBDEVICES).unwrap_or(0)
        } else {
            0
        };

        // Keyboard probes
        let hw_winlock = transport.get_property(PROP_WINLOCK_ENABLED).ok().map(|v| v != 0);
        let winlock_shortcuts_mask = transport
            .get_property(PROP_WINLOCK_DISABLED_SHORTCUTS)
            .ok()
            .map(|v| v as u8);
        let debounce_enabled = transport
            .get_property(PROP_PRESS_DEBOUNCE_ENABLED)
            .ok()
            .map(|v| v != 0);
        let debounce_timeout_ms = transport
            .get_property(PROP_PRESS_DEBOUNCE_TIMEOUT)
            .ok()
            .map(|v| v as u8);
        let esports_switch_on = transport.get_property(PROP_ESPORTS_SWITCH_POS).ok().map(|v| v != 0);
        let active_hw_profile = transport.get_property(PROP_ACTIVE_HW_PROFILE).ok().map(|v| v as u8);
        let storage_total_kb = transport.get_property(PROP_TOTAL_STORAGE_SIZE).ok().map(|v| v / 1024);
        let storage_free_kb = transport.get_property(PROP_FREE_STORAGE_SIZE).ok().map(|v| v / 1024);
        // Only treat 0xF5 as FlashTap if it really behaves like an on/off switch.
        let flashtap_raw = transport.get_property(PROP_FLASHTAP_ENABLED);
        let flashtap_probe = match &flashtap_raw {
            Ok(v) => format!("0x{v:X}"),
            Err(e) => e.to_string(),
        };
        info!(
            "{} : propriété FlashTap 0x{:02X} -> {}",
            transport.product_name, PROP_FLASHTAP_ENABLED, flashtap_probe
        );
        let flashtap_enabled = flashtap_raw.ok().and_then(flashtap_state_from_raw);
        let flashtap_supported = flashtap_enabled.is_some();
        let rapid_trigger_supported = transport.get_property(PROP_RAPID_TRIGGER_ENABLED).is_ok();

        // Mouse probes
        let mouse = if let Ok(s1) = transport.get_property(PROP_DPI_STAGE_1) {
            let s2 = transport.get_property(PROP_DPI_STAGE_2).unwrap_or(800);
            let s3 = transport.get_property(PROP_DPI_STAGE_3).unwrap_or(1600);
            let s4 = transport.get_property(PROP_DPI_STAGE_4).unwrap_or(3200);
            let s5 = transport.get_property(PROP_DPI_STAGE_5).unwrap_or(6400);
            let sniper_dpi = transport.get_property(PROP_DPI_STAGE_SNIPER).ok().map(|v| v as u16);
            let active_stage = transport.get_property(PROP_DPI_CURRENT_STAGE).unwrap_or(0) as u8;
            let current_dpi_x = transport
                .get_property(PROP_DPI_CURRENT_X)
                .or_else(|_| transport.get_property(PROP_DPI_CURRENT))
                .ok()
                .map(|v| v as u16);
            let current_dpi_y = transport.get_property(PROP_DPI_CURRENT_Y).ok().map(|v| v as u16);
            let lift_height = transport.get_property(PROP_LIFT_HEIGHT).ok().map(|v| v as u8);
            let motion_sync = transport.get_property_ext(PROP_EXT_MOTION_SYNC).ok().map(|v| v != 0);
            let angle_snapping = transport
                .get_property_ext(PROP_EXT_ANGLE_SNAPPING)
                .ok()
                .map(|v| v != 0);
            let auto_sleep_enabled = transport.get_property(PROP_AUTO_SLEEP_ENABLED).ok().map(|v| v != 0);
            let auto_sleep_min = transport
                .get_property(PROP_AUTO_SLEEP_TIMEOUT)
                .ok()
                .map(|v| (v / 60000).max(1) as u16);
            Some(CorsairMouseCaps {
                dpi_stages: [s1 as u16, s2 as u16, s3 as u16, s4 as u16, s5 as u16],
                sniper_dpi,
                active_stage,
                current_dpi_x,
                current_dpi_y,
                lift_height,
                motion_sync,
                angle_snapping,
                auto_sleep_enabled,
                auto_sleep_min,
            })
        } else {
            None
        };

        // Headset probes
        let sidetone_lvl = transport.get_property(PROP_SIDETONE_LEVEL).ok();
        let sidetone_mut = transport.get_property(PROP_SIDETONE_MUTED).ok();
        let headset = if sidetone_lvl.is_some() || sidetone_mut.is_some() {
            Some(CorsairHeadsetCaps {
                sidetone_muted: sidetone_mut.map(|v| v != 0),
                sidetone_pct: sidetone_lvl.map(|v| ((v.min(1000)) / 10) as u8),
                mic_muted: transport.get_property(PROP_MIC_MUTED).ok().map(|v| v != 0),
                active_eq_preset: transport.get_property(PROP_ACTIVE_EQ_PRESET).ok().map(|v| v as u8),
                anc_mode: transport.get_property(PROP_ANC_MODE).ok().map(|v| v as u8),
            })
        } else {
            None
        };

        // Cooler / Hub probes
        let cooler_rad = transport.get_property(PROP_COOLER_RADIATOR_SIZE).ok();
        let cooler_pump = transport.get_property(PROP_COOLER_PUMP_VERSION).ok();
        let cooler = if cooler_rad.is_some() || cooler_pump.is_some() {
            Some(CorsairCoolerCaps {
                product_line_id: transport.get_property(PROP_COOLER_PRODUCT_LINE).ok(),
                pump_version_id: cooler_pump,
                radiator_size: cooler_rad,
            })
        } else {
            None
        };

        // Classify device type purely from probed hardware capabilities (universal, zero hardcoded PIDs!)
        let lower_name = transport.product_name.to_ascii_lowercase();
        let kind = if mouse.is_some() || lower_name.contains("mouse") {
            CorsairDeviceKind::Mouse
        } else if headset.is_some()
            || lower_name.contains("headset")
            || lower_name.contains("virtuoso")
            || lower_name.contains("hs80")
        {
            CorsairDeviceKind::Headset
        } else if cooler.is_some()
            || lower_name.contains("commander")
            || lower_name.contains("icue link")
            || lower_name.contains("capellix")
        {
            CorsairDeviceKind::CoolerHub
        } else if subdev_bitmap != 0 && layout_raw.is_none() && hw_winlock.is_none() {
            CorsairDeviceKind::Dongle
        } else if layout_raw.is_some()
            || hw_winlock.is_some()
            || lower_name.contains("keyboard")
            || lower_name.contains("k70")
            || lower_name.contains("k65")
            || lower_name.contains("k100")
        {
            CorsairDeviceKind::Keyboard
        } else {
            CorsairDeviceKind::Generic
        };

        let display_name = if subdevice_id > 0 {
            format!("{} (Sans-fil #{subdevice_id})", transport.product_name)
        } else {
            transport.product_name.clone()
        };

        let id = format!("corsair_{:04X}_{:04X}_sub{}", vid, pid, subdevice_id);

        let info = CorsairDeviceInfo {
            id,
            vid,
            pid,
            subdevice_id,
            product_name: display_name,
            serial: transport.serial.clone(),
            kind,
            firmware_version: format_fw_version(fw_raw),
            bootloader_version: format_fw_version(bld_raw),
            layout: KeyboardLayout::from_bragi_value(layout_raw.unwrap_or(0)),
            mode,
            pollrate: poll_raw.and_then(PollingRate::from_bragi_value),
            max_pollrate: max_poll_raw.and_then(PollingRate::from_bragi_value),
            hw_brightness_raw,
            brightness_is_coarse,
            out_report_len: transport.out_report_len,
            in_report_len: transport.in_report_len,
            lighting_endpoint: LightingEndpoint::Planar0x01,
            battery_pct,
            battery_status,
            hw_winlock,
            winlock_shortcuts_mask,
            debounce_enabled,
            debounce_timeout_ms,
            esports_switch_on,
            active_hw_profile,
            storage_total_kb,
            storage_free_kb,
            flashtap_supported,
            flashtap_enabled,
            flashtap_probe,
            rapid_trigger_supported,
            hwp_profiles: Vec::new(),
            mouse,
            headset,
            cooler,
        };

        info!(
            "Corsair Bragi détecté : {} [{}] ({:04X}:{:04X}, sub={}, FW {})",
            info.product_name,
            info.kind.label(),
            info.vid,
            info.pid,
            info.subdevice_id,
            info.firmware_version
        );

        let mut dev = Self {
            transport,
            info,
            lighting_handle_open: false,
            last_keepalive: Instant::now(),
            restore_hw_on_drop: true,
        };
        if dev.info.kind == CorsairDeviceKind::Keyboard && dev.info.storage_total_kb.is_some() {
            let _ = dev.reload_hwp_profiles();
        }
        Ok((dev, subdev_bitmap))
    }

    /// Read all onboard hardware profiles from Bragi v2 Flash (safe read-only session).
    pub fn reload_hwp_profiles(&mut self) -> Result<(), BragiError> {
        let profs = self.with_flash_session(crate::hwp::read_all_hwp_profiles)?;
        self.info.hwp_profiles = profs;
        Ok(())
    }

    /// Flash a new onboard Hardware Lighting Effect (and optional name) directly into the keyboard's internal flash!
    pub fn flash_hwp_lighting(
        &mut self,
        profile_idx: usize,
        kind: crate::hwp::HwpLightingKind,
        primary: Rgb,
        secondary: Rgb,
        speed: u8,
        direction: u8,
        new_name: Option<&str>,
    ) -> Result<(), BragiError> {
        let prof = self
            .info
            .hwp_profiles
            .get(profile_idx)
            .cloned()
            .ok_or(BragiError::Protocol(0x01, "Index profil matériel invalide"))?;

        self.with_flash_session(|t| {
            crate::hwp::flash_hwp_profile_lighting(
                t,
                &prof,
                kind,
                primary,
                secondary,
                speed,
                direction,
                new_name,
            )
        })?;
        // Reload profiles & re-trigger active HW profile so MCU immediately renders the new flash effect!
        let _ = self.reload_hwp_profiles();
        let active = self.info.active_hw_profile.unwrap_or(0);
        let _ = self.set_active_hw_profile(active);
        Ok(())
    }

    pub fn hw_brightness_pct(&self) -> u8 {
        match self.info.hw_brightness_raw {
            Some(v) if self.info.brightness_is_coarse => match v {
                0 => 0,
                1 => 33,
                2 => 66,
                _ => 100,
            },
            Some(v) => ((v.min(1000)) / 10) as u8,
            None => 100,
        }
    }

    pub fn set_mode(&mut self, mode: OperatingMode) -> Result<(), BragiError> {
        if mode == OperatingMode::Hardware {
            let _ = self.transport.close_handle(HANDLE_LIGHTING);
            self.lighting_handle_open = false;
        }
        if self.info.mode == mode {
            if mode == OperatingMode::Software {
                self.ensure_lighting_handle()?;
            }
            return Ok(());
        }
        let wire = match mode {
            OperatingMode::Hardware => MODE_HARDWARE,
            OperatingMode::Software => MODE_SOFTWARE,
        };
        self.transport.set_property(PROP_MODE, wire)?;
        self.info.mode = mode;

        if mode == OperatingMode::Software {
            let _ = self.transport.get_property(PROP_LIGHTING_CTRL_REG);
            self.ensure_lighting_handle()?;
            if self.info.hw_brightness_raw == Some(0) {
                let _ = self.set_hw_brightness_pct(100);
            }
            self.last_keepalive = Instant::now();
        }
        Ok(())
    }

    fn ensure_lighting_handle(&mut self) -> Result<(), BragiError> {
        if self.lighting_handle_open {
            return Ok(());
        }
        let res = match self.info.lighting_endpoint {
            LightingEndpoint::Triplets0x22 => RES_LIGHTING_TRIPLETS,
            LightingEndpoint::Planar0x01 => RES_LIGHTING_PLANAR,
        };
        let code = self.transport.open_handle(HANDLE_LIGHTING, res)?;
        if code != ERR_OK {
            let alt = match self.info.lighting_endpoint {
                LightingEndpoint::Triplets0x22 => {
                    self.info.lighting_endpoint = LightingEndpoint::Planar0x01;
                    RES_LIGHTING_PLANAR
                }
                LightingEndpoint::Planar0x01 => {
                    self.info.lighting_endpoint = LightingEndpoint::Triplets0x22;
                    RES_LIGHTING_TRIPLETS
                }
            };
            let code2 = self.transport.open_handle(HANDLE_LIGHTING, alt)?;
            if code2 != ERR_OK {
                return Err(BragiError::from_code(code2));
            }
        }
        self.lighting_handle_open = true;
        Ok(())
    }

    pub fn set_pollrate(&mut self, rate: PollingRate) -> Result<(), BragiError> {
        if let Some(max) = self.info.max_pollrate {
            if rate > max {
                return Err(BragiError::PollrateExceedsMax(rate.hz(), max.hz()));
            }
        }
        self.transport
            .set_property(PROP_POLLRATE, rate.to_bragi_value())?;
        self.info.pollrate = Some(rate);
        Ok(())
    }

    pub fn set_hw_brightness_pct(&mut self, pct: u8) -> Result<(), BragiError> {
        let pct = pct.min(100);
        if self.info.brightness_is_coarse {
            let level = match pct {
                0 => 0,
                1..=33 => 1,
                34..=66 => 2,
                _ => 3,
            };
            self.transport.set_property(PROP_BRIGHTNESS_COARSE, level)?;
            self.info.hw_brightness_raw = Some(level as u32);
        } else {
            let val = (pct as u16) * 10;
            self.transport.set_property(PROP_BRIGHTNESS, val)?;
            self.info.hw_brightness_raw = Some(val as u32);
        }
        Ok(())
    }

    pub fn set_hw_winlock(&mut self, enabled: bool) -> Result<(), BragiError> {
        self.transport
            .set_property(PROP_WINLOCK_ENABLED, u16::from(enabled))?;
        self.info.hw_winlock = Some(enabled);
        Ok(())
    }

    /// Enable / disable FlashTap (hardware SOCD: the most recent of the two opposing keys wins,
    /// resolved inside the keyboard, invisible to anti-cheats).
    ///
    /// Defensive: the property must currently hold 0 or 1 (anything else means it is not a plain
    /// switch on this firmware and nothing is written), and the new value is read back.
    pub fn set_flashtap(&mut self, enabled: bool) -> Result<(), BragiError> {
        let current = self.transport.get_property(PROP_FLASHTAP_ENABLED)?;
        if flashtap_state_from_raw(current).is_none() {
            return Err(BragiError::Protocol(
                ERR_INVALID_VALUE,
                "Valeur FlashTap inattendue sur ce firmware : écriture refusée",
            ));
        }
        self.transport
            .set_property(PROP_FLASHTAP_ENABLED, u16::from(enabled))?;
        let readback = self.transport.get_property(PROP_FLASHTAP_ENABLED)?;
        if flashtap_state_from_raw(readback) != Some(enabled) {
            return Err(BragiError::Protocol(
                ERR_INVALID_VALUE,
                "Le clavier n'a pas confirmé le changement FlashTap",
            ));
        }
        self.info.flashtap_enabled = Some(enabled);
        Ok(())
    }

    pub fn set_winlock_shortcuts(&mut self, mask: u8) -> Result<(), BragiError> {
        self.transport
            .set_property(PROP_WINLOCK_DISABLED_SHORTCUTS, mask as u16)?;
        self.info.winlock_shortcuts_mask = Some(mask);
        Ok(())
    }

    pub fn set_hw_debounce(&mut self, enabled: bool, timeout_ms: u8) -> Result<(), BragiError> {
        self.transport
            .set_property(PROP_PRESS_DEBOUNCE_ENABLED, u16::from(enabled))?;
        if enabled {
            let _ = self
                .transport
                .set_property(PROP_PRESS_DEBOUNCE_TIMEOUT, timeout_ms as u16);
        }
        self.info.debounce_enabled = Some(enabled);
        self.info.debounce_timeout_ms = Some(timeout_ms);
        Ok(())
    }

    pub fn set_active_hw_profile(&mut self, idx: u8) -> Result<(), BragiError> {
        self.transport
            .set_property(PROP_ACTIVE_HW_PROFILE, idx as u16)?;
        self.info.active_hw_profile = Some(idx);
        Ok(())
    }

    pub fn set_indicator_color(&mut self, prop_id: u8, color: Rgb) -> Result<(), BragiError> {
        let packed = (color.r as u32) | ((color.g as u32) << 8) | ((color.b as u32) << 16);
        self.transport.set_property_u32(prop_id, packed)?;
        Ok(())
    }

    /// Low-level read of any Bragi onboard flash file by `FileId` (e.g. `0x0011` FileListing, `0x000F` ProfilesList).
    /// Temporarily ensures Software mode (required by Bragi v2 MCU to open flash handles) and restores Hardware mode cleanly.
    pub fn read_flash_file(&mut self, file_id: u16) -> Result<Vec<u8>, BragiError> {
        // File access reuses handle 0 (= the lighting handle) and closes it afterwards:
        // forget it so the next frame re-opens it instead of writing to a closed handle.
        self.lighting_handle_open = false;
        let was_hw = self.info.mode == OperatingMode::Hardware;
        if was_hw {
            let _ = self.transport.set_property(PROP_MODE, MODE_SOFTWARE);
        }
        let res = self.transport.read_file(file_id);
        if was_hw {
            let _ = self.transport.set_property(PROP_MODE, MODE_HARDWARE);
        }
        res
    }

    /// Execute a batch of flash file operations inside a single Software-mode session, restoring Hardware mode on exit.
    pub fn with_flash_session<T, F: FnOnce(&mut BragiTransport) -> Result<T, BragiError>>(
        &mut self,
        f: F,
    ) -> Result<T, BragiError> {
        let was_hw = self.info.mode == OperatingMode::Hardware;
        if self.lighting_handle_open {
            let _ = self.transport.close_handle(HANDLE_LIGHTING);
            self.lighting_handle_open = false;
        }
        if was_hw {
            self.transport.set_property(PROP_MODE, MODE_SOFTWARE)?;
            std::thread::sleep(Duration::from_millis(60));
            self.transport.drain_input();
        }
        let res = f(&mut self.transport);
        if was_hw {
            let _ = self.transport.set_property(PROP_MODE, MODE_HARDWARE);
        }
        res
    }

    pub fn create_flash_file(&mut self, file_id: u16) -> Result<(), BragiError> {
        self.transport.create_file(file_id)
    }

    pub fn remove_flash_file(&mut self, file_id: u16) -> Result<(), BragiError> {
        self.transport.remove_file(file_id)
    }

    pub fn write_flash_file(&mut self, file_id: u16, data: &[u8]) -> Result<(), BragiError> {
        // Same handle-0 reuse as read_flash_file: the lighting handle must be re-opened afterwards.
        self.lighting_handle_open = false;
        self.transport.write_file(file_id, data)
    }

    // ---- Universal Bragi Mouse Controls ----
    pub fn set_mouse_dpi_stage(&mut self, stage_idx: u8, dpi: u16) -> Result<(), BragiError> {
        let prop = match stage_idx {
            0 => PROP_DPI_STAGE_1,
            1 => PROP_DPI_STAGE_2,
            2 => PROP_DPI_STAGE_3,
            3 => PROP_DPI_STAGE_4,
            4 => PROP_DPI_STAGE_5,
            _ => PROP_DPI_STAGE_SNIPER,
        };
        let clamped = dpi.clamp(100, 30000);
        self.transport.set_property(prop, clamped)?;
        if let Some(m) = self.info.mouse.as_mut() {
            if (stage_idx as usize) < 5 {
                m.dpi_stages[stage_idx as usize] = clamped;
            } else {
                m.sniper_dpi = Some(clamped);
            }
        }
        Ok(())
    }

    pub fn set_mouse_active_stage(&mut self, stage_idx: u8) -> Result<(), BragiError> {
        let idx = stage_idx.min(4);
        self.transport
            .set_property(PROP_DPI_CURRENT_STAGE, idx as u16)?;
        if let Some(m) = self.info.mouse.as_mut() {
            m.active_stage = idx;
            m.current_dpi_x = Some(m.dpi_stages[idx as usize]);
        }
        Ok(())
    }

    pub fn set_mouse_lift_height(&mut self, level: u8) -> Result<(), BragiError> {
        self.transport.set_property(PROP_LIFT_HEIGHT, level as u16)?;
        if let Some(m) = self.info.mouse.as_mut() {
            m.lift_height = Some(level);
        }
        Ok(())
    }

    // ---- Universal Bragi Headset Controls ----
    pub fn set_headset_sidetone(&mut self, pct: u8, muted: bool) -> Result<(), BragiError> {
        let clamped = pct.min(100);
        let _ = self
            .transport
            .set_property(PROP_SIDETONE_MUTED, u16::from(muted));
        self.transport
            .set_property(PROP_SIDETONE_LEVEL, (clamped as u16) * 10)?;
        if let Some(h) = self.info.headset.as_mut() {
            h.sidetone_muted = Some(muted);
            h.sidetone_pct = Some(clamped);
        }
        Ok(())
    }

    pub fn tick_keepalive(&mut self) {
        if self.info.mode == OperatingMode::Software
            && self.last_keepalive.elapsed() >= Duration::from_secs(45)
        {
            if let Err(e) = self.transport.send_keepalive() {
                warn!("Keepalive Bragi échoué: {e}");
            }
            self.last_keepalive = Instant::now();
        }
    }

    /// Render an `RgbEffectConfig` to a Corsair Bragi keyboard / RGB device.
    pub fn render_effect(&mut self, effect: &RgbEffectConfig, t_sec: f32) -> Result<(), BragiError> {
        self.render_custom_frame(|led_idx, nx, ny, _id| effect.sample(led_idx, nx, ny, t_sec))
    }

    /// Render a custom per-key RGB frame (used for Ambilight Screen Sync & Reactive Typing Ripples).
    pub fn render_custom_frame<F>(&mut self, sampler: F) -> Result<(), BragiError>
    where
        F: Fn(usize, f32, f32, &str) -> Rgb,
    {
        if self.info.mode != OperatingMode::Software {
            self.set_mode(OperatingMode::Software)?;
        }
        self.ensure_lighting_handle()?;
        self.tick_keepalive();

        let mut frame = [Rgb::BLACK; K70_TKL_LED_COUNT];
        let base_sample = sampler(0, 0.5, 0.5, "");
        for slot in &mut frame {
            *slot = base_sample;
        }
        for key in K70_TKL_KEYS {
            if key.led_index < K70_TKL_LED_COUNT {
                frame[key.led_index] = sampler(key.led_index, key.norm_x(), key.norm_y(), key.id);
            }
        }

        let payload = match self.info.lighting_endpoint {
            LightingEndpoint::Triplets0x22 => {
                let mut buf = Vec::with_capacity(2 + K70_TKL_LED_COUNT * 3);
                buf.push(0x12);
                buf.push(0x00);
                for c in &frame {
                    buf.push(c.r);
                    buf.push(c.g);
                    buf.push(c.b);
                }
                buf
            }
            LightingEndpoint::Planar0x01 => {
                let mut buf = vec![0u8; K70_TKL_LED_COUNT * 3];
                for (i, c) in frame.iter().enumerate() {
                    buf[i] = c.r;
                    buf[K70_TKL_LED_COUNT + i] = c.g;
                    buf[K70_TKL_LED_COUNT * 2 + i] = c.b;
                }
                buf
            }
        };
        self.transport.write_handle_data(HANDLE_LIGHTING, &payload)?;
        Ok(())
    }
}

impl Drop for CorsairDevice {
    fn drop(&mut self) {
        if self.restore_hw_on_drop && self.info.mode == OperatingMode::Software {
            let _ = self.set_mode(OperatingMode::Hardware);
        }
    }
}

/// FlashTap state from the raw `0xF5` property: only 0 / 1 are accepted as a valid switch.
pub(crate) fn flashtap_state_from_raw(raw: u32) -> Option<bool> {
    match raw {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod flashtap_tests {
    use super::flashtap_state_from_raw;

    #[test]
    fn only_boolean_values_are_a_flashtap_switch() {
        assert_eq!(flashtap_state_from_raw(0), Some(false));
        assert_eq!(flashtap_state_from_raw(1), Some(true));
        assert_eq!(flashtap_state_from_raw(2), None);
        assert_eq!(flashtap_state_from_raw(0x0101), None);
    }
}
