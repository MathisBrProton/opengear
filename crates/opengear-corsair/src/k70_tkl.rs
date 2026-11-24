use crate::keymap::{K70_TKL_KEYS, K70_TKL_LED_COUNT};
use crate::proto::*;
use crate::transport::BragiTransport;
use opengear_core::{KeyboardLayout, OperatingMode, PollingRate, Rgb, RgbEffectConfig};
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingEndpoint {
    /// Resource 0x0022: 2-byte header `[0x12, 0x00]` + interleaved RGB triplets.
    Triplets0x22,
    /// Resource 0x0001: planar `RRR...GGG...BBB...` channels.
    Planar0x01,
}

#[derive(Debug, Clone)]
pub struct CorsairKeyboardInfo {
    pub vid: u16,
    pub pid: u16,
    pub product_name: String,
    pub serial: String,
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
    // Extended Bragi v2 hardware properties reversed via REA/Ghidra from iCUE5 BragiProtocols.dll
    pub hw_winlock: Option<bool>,
    pub winlock_shortcuts_mask: Option<u8>,
    pub debounce_enabled: Option<bool>,
    pub debounce_timeout_ms: Option<u8>,
    pub esports_switch_on: Option<bool>,
    pub active_hw_profile: Option<u8>,
    pub storage_total_kb: Option<u32>,
    pub storage_free_kb: Option<u32>,
    pub flashtap_supported: bool,
}

pub struct CorsairKeyboard {
    transport: BragiTransport,
    pub info: CorsairKeyboardInfo,
    lighting_handle_open: bool,
    last_keepalive: Instant,
    restore_hw_on_drop: bool,
}

impl CorsairKeyboard {
    /// Connect to a Corsair Bragi keyboard (defaults to any Corsair Bragi keyboard, e.g. K70 RGB TKL 1B1C:1B73).
    /// Queries properties cleanly and leaves the device in its original operating mode until software features are activated.
    pub fn connect(target_pid: Option<u16>) -> Result<Self, BragiError> {
        let mut transport = BragiTransport::open_first(target_pid)?;

        // Check current mode
        let initial_mode_raw = transport.get_property(PROP_MODE).unwrap_or(MODE_HARDWARE as u32);
        let initial_mode = if initial_mode_raw == MODE_SOFTWARE as u32 {
            OperatingMode::Software
        } else {
            OperatingMode::Hardware
        };

        // Read metadata & capabilities (some firmwares require software mode temporarily to read all properties,
        // so we try directly first and only switch briefly if needed).
        let fw_raw = transport.get_property(PROP_APP_VER).unwrap_or(0);
        let bld_raw = transport.get_property(PROP_BLD_VER).unwrap_or(0);
        let layout_raw = transport.get_property(PROP_HWLAYOUT).unwrap_or(0);
        let poll_raw = transport.get_property(PROP_POLLRATE).ok();
        let max_poll_raw = transport.get_property(PROP_MAX_POLLRATE).ok();

        let (hw_brightness_raw, brightness_is_coarse) = match transport.get_property(PROP_BRIGHTNESS) {
            Ok(v) => (Some(v), false),
            Err(_) => match transport.get_property(PROP_BRIGHTNESS_COARSE) {
                Ok(v) => (Some(v), true),
                Err(_) => (None, false),
            },
        };

        // Default to standard Bragi Planar resource 0x0001 (used by K70 RGB TKL);
        // ensure_lighting_handle() automatically falls back to 0x0022 (Triplets) if needed in Software mode.
        let lighting_endpoint = LightingEndpoint::Planar0x01;

        let hw_winlock = transport.get_property(PROP_WINLOCK_ENABLED).ok().map(|v| v != 0);
        let winlock_shortcuts_mask = transport.get_property(PROP_WINLOCK_DISABLED_SHORTCUTS).ok().map(|v| v as u8);
        let debounce_enabled = transport.get_property(PROP_PRESS_DEBOUNCE_ENABLED).ok().map(|v| v != 0);
        let debounce_timeout_ms = transport.get_property(PROP_PRESS_DEBOUNCE_TIMEOUT).ok().map(|v| v as u8);
        let esports_switch_on = transport.get_property(PROP_ESPORTS_SWITCH_POS).ok().map(|v| v != 0);
        let active_hw_profile = transport.get_property(PROP_ACTIVE_HW_PROFILE).ok().map(|v| v as u8);
        let storage_total_kb = transport.get_property(PROP_TOTAL_STORAGE_SIZE).ok().map(|v| v / 1024);
        let storage_free_kb = transport.get_property(PROP_FREE_STORAGE_SIZE).ok().map(|v| v / 1024);
        let flashtap_supported = transport.get_property(PROP_FLASHTAP_ENABLED).is_ok();

        let info = CorsairKeyboardInfo {
            vid: transport.vid,
            pid: transport.pid,
            product_name: transport.product_name.clone(),
            serial: transport.serial.clone(),
            firmware_version: format_fw_version(fw_raw),
            bootloader_version: format_fw_version(bld_raw),
            layout: KeyboardLayout::from_bragi_value(layout_raw),
            mode: initial_mode,
            pollrate: poll_raw.and_then(PollingRate::from_bragi_value),
            max_pollrate: max_poll_raw.and_then(PollingRate::from_bragi_value),
            hw_brightness_raw,
            brightness_is_coarse,
            out_report_len: transport.out_report_len,
            in_report_len: transport.in_report_len,
            lighting_endpoint,
            hw_winlock,
            winlock_shortcuts_mask,
            debounce_enabled,
            debounce_timeout_ms,
            esports_switch_on,
            active_hw_profile,
            storage_total_kb,
            storage_free_kb,
            flashtap_supported,
        };

        info!(
            "Connecté à {} ({:04X}:{:04X}), FW {}, Layout {}, Pollrate {:?} (max {:?}), Report {}B, RGB {:?}",
            info.product_name,
            info.vid,
            info.pid,
            info.firmware_version,
            info.layout.label(),
            info.pollrate,
            info.max_pollrate,
            info.out_report_len,
            info.lighting_endpoint,
        );

        Ok(Self {
            transport,
            info,
            lighting_handle_open: false,
            last_keepalive: Instant::now(),
            restore_hw_on_drop: true,
        })
    }

    /// Prevent restoring hardware mode on Drop (useful for one-shot CLI commands like setting pollrate or brightness).
    pub fn set_restore_hw_on_drop(&mut self, restore: bool) {
        self.restore_hw_on_drop = restore;
    }

    pub fn set_mode(&mut self, mode: OperatingMode) -> Result<(), BragiError> {
        if mode == OperatingMode::Hardware {
            // Always close lighting handle before reverting to HW mode (even if opened by a prior run)
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
            // Init lighting control register (matches OpenRGB CorsairPeripheralV2Controller)
            let _ = self.transport.get_property(PROP_LIGHTING_CTRL_REG);
            self.ensure_lighting_handle()?;
            // If hardware backlight was dimmed to 0%, ensure LEDs are visible in software mode
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
            // Try the alternative endpoint if the probed one refused after mode switch
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

    pub fn get_pollrate(&mut self) -> Result<PollingRate, BragiError> {
        let raw = self.transport.get_property(PROP_POLLRATE)?;
        let rate = PollingRate::from_bragi_value(raw)
            .ok_or_else(|| BragiError::Protocol(raw as u8, "Unknown pollrate value"))?;
        self.info.pollrate = Some(rate);
        Ok(rate)
    }

    /// Change USB polling rate (125 Hz .. 8000 Hz).
    /// Note: Changing polling rate on Corsair AXON keyboards may briefly re-enumerate USB on some firmwares.
    pub fn set_pollrate(&mut self, rate: PollingRate) -> Result<(), BragiError> {
        if let Some(max) = self.info.max_pollrate {
            if rate > max {
                return Err(BragiError::PollrateExceedsMax(rate.hz(), max.hz()));
            }
        }
        self.transport.set_property(PROP_POLLRATE, rate.to_bragi_value())?;
        self.info.pollrate = Some(rate);
        info!("Fréquence de polling définie sur {}", rate);
        Ok(())
    }

    /// Set hardware backlight brightness (0..=100%).
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
            let val = (pct as u16) * 10; // 0..=1000
            self.transport.set_property(PROP_BRIGHTNESS, val)?;
            self.info.hw_brightness_raw = Some(val as u32);
        }
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

    /// Send keepalive if needed (>45s elapsed while in Software mode).
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

    /// Push a full 193-LED RGB frame directly to the keyboard.
    pub fn send_rgb_frame(&mut self, leds: &[Rgb; K70_TKL_LED_COUNT]) -> Result<(), BragiError> {
        if self.info.mode != OperatingMode::Software {
            self.set_mode(OperatingMode::Software)?;
        }
        self.ensure_lighting_handle()?;
        self.tick_keepalive();

        let payload = match self.info.lighting_endpoint {
            LightingEndpoint::Triplets0x22 => {
                // Header [0x12, 0x00] + RGBRGBRGB...
                let mut buf = Vec::with_capacity(2 + K70_TKL_LED_COUNT * 3);
                buf.push(0x12);
                buf.push(0x00);
                for c in leds {
                    buf.push(c.r);
                    buf.push(c.g);
                    buf.push(c.b);
                }
                buf
            }
            LightingEndpoint::Planar0x01 => {
                // Planar RRR...GGG...BBB...
                let mut buf = vec![0u8; K70_TKL_LED_COUNT * 3];
                for (i, c) in leds.iter().enumerate() {
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

    /// Render an `RgbEffectConfig` over all K70 TKL keys at elapsed time `t_sec` and send to hardware.
    pub fn render_effect(&mut self, effect: &RgbEffectConfig, t_sec: f32) -> Result<[Rgb; K70_TKL_LED_COUNT], BragiError> {
        let mut frame = [Rgb::BLACK; K70_TKL_LED_COUNT];
        // Fill base fallback for any auxiliary/unmapped LED indices
        let base_sample = effect.sample(0, 0.5, 0.5, t_sec);
        for slot in &mut frame {
            *slot = base_sample;
        }
        // Populate exact mapped keys with their spatial coordinates & per-key overrides
        for key in K70_TKL_KEYS {
            if key.led_index < K70_TKL_LED_COUNT {
                frame[key.led_index] = effect.sample(key.led_index, key.norm_x(), key.norm_y(), t_sec);
            }
        }
        self.send_rgb_frame(&frame)?;
        Ok(frame)
    }

    /// Set a single solid color across the entire keyboard.
    pub fn set_solid_color(&mut self, color: Rgb) -> Result<(), BragiError> {
        let frame = [color; K70_TKL_LED_COUNT];
        self.send_rgb_frame(&frame)
    }

    /// Toggle hardware WinLock directly inside the keyboard MCU (`0x45`).
    pub fn set_hw_winlock(&mut self, enabled: bool) -> Result<(), BragiError> {
        self.transport.set_property(PROP_WINLOCK_ENABLED, u16::from(enabled))?;
        self.info.hw_winlock = Some(enabled);
        Ok(())
    }

    /// Configure which shortcuts WinLock disables in hardware (`0x4A`: bit0=Win, bit1=Alt+Tab, bit2=Alt+F4, bit3=Shift+Tab).
    pub fn set_winlock_shortcuts(&mut self, mask: u8) -> Result<(), BragiError> {
        self.transport.set_property(PROP_WINLOCK_DISABLED_SHORTCUTS, mask as u16)?;
        self.info.winlock_shortcuts_mask = Some(mask);
        Ok(())
    }

    /// Configure hardware keyswitch press debounce (`0x09` enabled + `0x0A` timeout in ms, e.g. 0..=270).
    pub fn set_hw_debounce(&mut self, enabled: bool, timeout_ms: u8) -> Result<(), BragiError> {
        self.transport.set_property(PROP_PRESS_DEBOUNCE_ENABLED, u16::from(enabled))?;
        if enabled {
            let _ = self.transport.set_property(PROP_PRESS_DEBOUNCE_TIMEOUT, timeout_ms as u16);
        }
        self.info.debounce_enabled = Some(enabled);
        self.info.debounce_timeout_ms = Some(timeout_ms);
        Ok(())
    }

    /// Switch active onboard hardware profile index (`0x6E`).
    pub fn set_active_hw_profile(&mut self, idx: u8) -> Result<(), BragiError> {
        self.transport.set_property(PROP_ACTIVE_HW_PROFILE, idx as u16)?;
        self.info.active_hw_profile = Some(idx);
        Ok(())
    }

    /// Customize hardware indicator LED color (WinLock On `0x5D`, WinLock Off `0x5E`, Profile `0x49`, Brightness `0x5C`, Esports `0x77`).
    pub fn set_indicator_color(&mut self, prop_id: u8, color: Rgb) -> Result<(), BragiError> {
        let packed = (color.r as u32) | ((color.g as u32) << 8) | ((color.b as u32) << 16);
        self.transport.set_property_u32(prop_id, packed)?;
        Ok(())
    }
}

impl Drop for CorsairKeyboard {
    fn drop(&mut self) {
        if self.restore_hw_on_drop && self.info.mode == OperatingMode::Software {
            let _ = self.set_mode(OperatingMode::Hardware);
        }
    }
}
