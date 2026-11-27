//! Corsair Bragi devices in the shared device model (`opengear_core::driver`).

use crate::device::{CorsairDevice, CorsairDeviceKind};
use crate::hwp::HwpLightingKind;
use crate::keymap::{K70_TKL_KEYS, keys_for_layout, layout_for_device};
use crate::proto::{
    CORSAIR_VID, PROP_BRIGHTNESS_INDICATOR_COLOR, PROP_ESPORTS_BACKLIGHT_COLOR,
    PROP_PROFILE_INDICATOR_COLOR, PROP_WINLOCK_OFF_COLOR, PROP_WINLOCK_ON_COLOR,
};
use anyhow::{Context, Result, bail};
use opengear_core::driver::{
    ChoiceOption, Connection, Device, DeviceInfo, DeviceKeyGeom, DeviceKeyboardLayout, DeviceKind,
    DeviceSpec, DeviceStatus, Driver, Section, Setting, Value, polling_rate_options, setting_ids,
};
use opengear_core::lighting::{
    EffectParams, EffectRequest, HardwareEffect, Led, Lighting, Zone, ZoneKind,
};
use opengear_core::{
    OnboardBackupEntry, OnboardMemory, OnboardMemoryInfo, OnboardProfileSummary, OperatingMode,
    PointerDevice, PointerDpiInfo, PollingRate, Rgb,
};

/// Read-only Bragi flash file dump for CLI diagnostics (`opengear corsair hwp-dump`).
pub fn dump_first_hwp_flash() -> Result<String> {
    use std::fmt::Write as _;
    let mut devices = CorsairDevice::connect_all();
    let dev = devices
        .first_mut()
        .context("Aucun périphérique Corsair Bragi détecté")?;
    let mut out = format!(
        "=== Dump Flash Bragi v2 (READ-ONLY) : {} ===\n",
        dev.info.product_name
    );
    dev.with_flash_session(|t| {
        let mut to_inspect: Vec<u16> = Vec::new();
        for &(fid, label) in &[
            (crate::proto::FILE_LISTING, "FileListing (0x0011)"),
            (crate::proto::FILE_PROFILES_LIST, "ProfilesList (0x000F)"),
        ] {
            match t.read_file(fid) {
                Ok(bytes) => {
                    let _ = write!(out, "\n[{label}] {} octets : ", bytes.len());
                    for b in &bytes {
                        let _ = write!(out, "{b:02X} ");
                    }
                    out.push('\n');
                    for chunk in bytes.as_chunks::<2>().0 {
                        let id = u16::from_le_bytes([chunk[0], chunk[1]]);
                        if id >= crate::proto::FILE_FREE_MIN && !to_inspect.contains(&id) {
                            to_inspect.push(id);
                        }
                    }
                }
                Err(e) => {
                    let _ = writeln!(out, "[{label}] -> {e}");
                }
            }
        }
        let _ = writeln!(
            out,
            "\n=== Fichiers utilisateurs détectés ({}) ===",
            to_inspect.len()
        );
        for fid in to_inspect {
            match t.read_file(fid) {
                Ok(bytes) => {
                    let hdr = if bytes.len() >= 2 {
                        u16::from_le_bytes([bytes[0], bytes[1]])
                    } else {
                        0
                    };
                    let _ = writeln!(
                        out,
                        "\n  File 0x{fid:04X} ({} B, magic=0x{hdr:04X}):",
                        bytes.len()
                    );
                    for (row, chunk) in bytes.chunks(16).enumerate() {
                        let _ = write!(out, "    {:04X}: ", row * 16);
                        for b in chunk {
                            let _ = write!(out, "{b:02X} ");
                        }
                        out.push_str("  ");
                        for &b in chunk {
                            let ch = if (0x20..=0x7E).contains(&b) {
                                b as char
                            } else {
                                '.'
                            };
                            out.push(ch);
                        }
                        out.push('\n');
                    }
                }
                Err(e) => {
                    let _ = writeln!(out, "  File 0x{fid:04X} -> {e}");
                }
            }
        }
        Ok(())
    })?;
    Ok(out)
}

/// Driver discovering all connected Corsair Bragi v2 devices.
pub struct CorsairDriver;

impl Driver for CorsairDriver {
    fn name(&self) -> &'static str {
        "corsair"
    }

    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[CORSAIR_VID]
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        let mut out: Vec<Box<dyn Device>> = CorsairDevice::connect_all()
            .into_iter()
            .map(|d| Box::new(d) as Box<dyn Device>)
            .collect();
        for cpro in crate::commander::CorsairCommanderProDevice::discover_all() {
            out.push(Box::new(cpro));
        }
        out
    }
}

const HW_PROFILE: &str = "corsair.hw_profile";
const DEBOUNCE: &str = "corsair.debounce";
const DEBOUNCE_MS: &str = "corsair.debounce_ms";
const LIFT_HEIGHT: &str = "corsair.lift_height";
/// Shortcuts blocked by the hardware Windows lock: `(setting id, label, bit)`.
const WINLOCK_SHORTCUTS: [(&str, &str, u8); 4] = [
    ("corsair.winlock.win", "Verrou : touche Windows", 1),
    ("corsair.winlock.alt_tab", "Verrou : Alt + Tab", 2),
    ("corsair.winlock.alt_f4", "Verrou : Alt + F4", 4),
    ("corsair.winlock.shift_tab", "Verrou : Maj + Tab", 8),
];
/// Programmable keyboard indicator LEDs: `(setting id, label, Bragi property id)`.
const INDICATOR_COLORS: [(&str, &str, u8); 5] = [
    (
        "corsair.indicator.esports",
        "Voyant mode tournoi",
        PROP_ESPORTS_BACKLIGHT_COLOR,
    ),
    (
        "corsair.indicator.winlock_on",
        "Voyant verrou Windows actif",
        PROP_WINLOCK_ON_COLOR,
    ),
    (
        "corsair.indicator.winlock_off",
        "Voyant verrou Windows inactif",
        PROP_WINLOCK_OFF_COLOR,
    ),
    (
        "corsair.indicator.profile",
        "Voyant bouton profil",
        PROP_PROFILE_INDICATOR_COLOR,
    ),
    (
        "corsair.indicator.brightness",
        "Voyant luminosité",
        PROP_BRIGHTNESS_INDICATOR_COLOR,
    ),
];
const DEBOUNCE_STEPS_MS: [u8; 10] = [0, 1, 2, 3, 5, 10, 15, 20, 30, 60];
/// Mask used by the firmware when it does not report one.
const DEFAULT_WINLOCK_MASK: u8 = 3;

fn ms_options() -> Vec<ChoiceOption> {
    DEBOUNCE_STEPS_MS
        .iter()
        .map(|ms| ChoiceOption::new(ms.to_string(), format!("{ms} ms")))
        .collect()
}

fn number(value: &Value) -> Result<f64> {
    value.as_f64().context("nombre attendu")
}

fn flag(value: &Value) -> Result<bool> {
    value.as_bool().context("booléen attendu")
}

fn parsed<T: std::str::FromStr>(value: &Value) -> Result<T> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .context("choix invalide")
}

impl Device for CorsairDevice {
    fn info(&self) -> DeviceInfo {
        let i = &self.info;
        DeviceInfo {
            id: format!("corsair:{}", i.id),
            vendor: "Corsair".into(),
            model: i.product_name.clone(),
            kind: match i.kind {
                CorsairDeviceKind::Keyboard => DeviceKind::Keyboard,
                CorsairDeviceKind::Mouse => DeviceKind::Mouse,
                CorsairDeviceKind::Headset => DeviceKind::Headset,
                CorsairDeviceKind::CoolerHub => DeviceKind::Cooler,
                CorsairDeviceKind::Dongle | CorsairDeviceKind::Generic => DeviceKind::Other,
            },
            connection: if i.subdevice_id == 0 {
                Connection::Usb
            } else {
                Connection::Wireless
            },
            firmware: (!i.firmware_version.is_empty()).then(|| i.firmware_version.clone()),
        }
    }

    fn status(&self) -> DeviceStatus {
        DeviceStatus {
            battery_pct: self.info.battery_pct,
            ..Default::default()
        }
    }

    fn specs(&self) -> Vec<DeviceSpec> {
        let i = &self.info;
        let mut out = Vec::new();
        if i.kind == CorsairDeviceKind::Keyboard {
            out.push(DeviceSpec::simple("Disposition", i.layout.label()));
        }
        if let Some(total) = i.storage_total_kb
            && total > 0
        {
            let free = i
                .storage_free_kb
                .map_or_else(|| "?".to_string(), |f| f.to_string());
            out.push(DeviceSpec::simple(
                "Mémoire interne",
                format!("{free} Ko libres sur {total} Ko"),
            ));
        }
        if let Some(ref c) = i.cooler {
            if let Some(v) = c.product_line_id {
                out.push(DeviceSpec::simple("Gamme", format!("0x{v:02X}")));
            }
            if let Some(v) = c.pump_version_id {
                out.push(DeviceSpec::simple("Version de pompe", format!("0x{v:02X}")));
            }
            if let Some(v) = c.radiator_size {
                out.push(DeviceSpec::simple("Taille du radiateur", format!("{v} mm")));
            }
        }
        if i.esports_switch_on == Some(true) {
            out.push(DeviceSpec::simple("Interrupteur tournoi", "Activé"));
        }
        let usb = if i.subdevice_id > 0 {
            format!(
                "{:04X}:{:04X} (sous-appareil {})",
                i.vid, i.pid, i.subdevice_id
            )
        } else {
            format!("{:04X}:{:04X}", i.vid, i.pid)
        };
        out.push(DeviceSpec::expert("Identifiant USB", usb, true));
        if !i.bootloader_version.is_empty() {
            out.push(DeviceSpec::expert(
                "Bootloader",
                &i.bootloader_version,
                true,
            ));
        }
        if !i.serial.is_empty() {
            out.push(DeviceSpec::expert("Numéro de série", &i.serial, true));
        }
        if i.out_report_len > 0 {
            out.push(DeviceSpec::expert(
                "Taille de rapport",
                format!("{} octets", i.out_report_len),
                true,
            ));
        }
        out
    }

    fn settings(&self) -> Vec<Setting> {
        let i = &self.info;
        let mut out = Vec::new();
        if let Some(rate) = i.pollrate {
            let max = i.max_pollrate.map_or(8000, |m| m.hz());
            let rates = [125, 250, 500, 1000, 2000, 4000, 8000]
                .into_iter()
                .filter(|&hz| hz <= max);
            out.push(
                Setting::choice(
                    setting_ids::POLLING_RATE,
                    "Fréquence d'envoi",
                    Section::Performance,
                    polling_rate_options(rates),
                    &rate.hz().to_string(),
                )
                .help("Plus haut = moins de latence, un peu plus de charge processeur.")
                .stored_in_device(),
            );
        }
        if i.kind == CorsairDeviceKind::Keyboard {
            out.push(
                Setting::slider(
                    setting_ids::HW_BRIGHTNESS,
                    "Luminosité matérielle",
                    Section::Lighting,
                    (0.0, 100.0, 1.0),
                    "%",
                    f64::from(self.hw_brightness_pct()),
                )
                .stored_in_device(),
            );
        }
        if let Some(profile) = i.active_hw_profile {
            out.push(
                Setting::choice(
                    HW_PROFILE,
                    "Profil interne",
                    Section::General,
                    (0..3)
                        .map(|n| ChoiceOption::new(n.to_string(), format!("Profil {}", n + 1)))
                        .collect(),
                    &profile.to_string(),
                )
                .stored_in_device(),
            );
        }
        if let Some(on) = i.hw_winlock {
            out.push(
                Setting::toggle(
                    setting_ids::WIN_LOCK,
                    "Bloquer la touche Windows (dans le clavier)",
                    Section::Input,
                    on,
                )
                .stored_in_device(),
            );
            let mask = i.winlock_shortcuts_mask.unwrap_or(DEFAULT_WINLOCK_MASK);
            for (id, label, bit) in WINLOCK_SHORTCUTS {
                out.push(
                    Setting::toggle(id, label, Section::Input, mask & bit != 0)
                        .expert()
                        .stored_in_device(),
                );
            }
        }
        if i.flashtap_supported {
            out.push(
                Setting::toggle(
                    setting_ids::HW_SOCD,
                    "FlashTap (priorité des touches opposées dans le clavier)",
                    Section::Input,
                    i.flashtap_enabled.unwrap_or(false),
                )
                .help("Le clavier gère lui-même deux touches opposées enfoncées : seule la dernière compte.")
                .stored_in_device(),
            );
        }
        if let Some(on) = i.debounce_enabled {
            let ms = i.debounce_timeout_ms.unwrap_or(30);
            out.push(
                Setting::toggle(DEBOUNCE, "Anti-rebond matériel", Section::Input, on)
                    .expert()
                    .stored_in_device(),
            );
            out.push(
                Setting::choice(
                    DEBOUNCE_MS,
                    "Délai anti-rebond",
                    Section::Input,
                    ms_options(),
                    &ms.to_string(),
                )
                .help("Plus bas = plus réactif, mais risque de doubles frappes.")
                .expert()
                .stored_in_device(),
            );
        }
        if let Some(level) = i.mouse.as_ref().and_then(|m| m.lift_height) {
            out.push(
                Setting::choice(
                    LIFT_HEIGHT,
                    "Distance de décrochage",
                    Section::Performance,
                    vec![
                        ChoiceOption::new("0", "Basse"),
                        ChoiceOption::new("1", "Moyenne"),
                        ChoiceOption::new("2", "Haute"),
                    ],
                    &level.to_string(),
                )
                .stored_in_device(),
            );
        }
        if let Some(pct) = i.headset.as_ref().and_then(|h| h.sidetone_pct) {
            out.push(
                Setting::slider(
                    setting_ids::SIDETONE,
                    "Retour micro (sidetone)",
                    Section::Audio,
                    (0.0, 100.0, 1.0),
                    "%",
                    f64::from(pct),
                )
                .help("T'entendre dans le casque quand tu parles."),
            );
        }
        if i.kind == CorsairDeviceKind::Keyboard {
            for (sid, label, _) in INDICATOR_COLORS {
                out.push(
                    Setting::color(sid, label, Section::Lighting, "#FF1848")
                        .expert()
                        .stored_in_device(),
                );
            }
        }
        out
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        (self.info.kind == CorsairDeviceKind::Keyboard).then_some(self as &mut dyn Lighting)
    }

    fn pointer(&mut self) -> Option<&mut dyn PointerDevice> {
        self.info
            .mouse
            .is_some()
            .then_some(self as &mut dyn PointerDevice)
    }

    fn onboard(&mut self) -> Option<&mut dyn OnboardMemory> {
        (!self.info.hwp_profiles.is_empty() || self.info.kind == CorsairDeviceKind::Keyboard)
            .then_some(self as &mut dyn OnboardMemory)
    }

    fn diagnostic_lines(&self) -> Vec<String> {
        vec![format!(
            "Corsair Bragi : report {} o | FlashTap 0xF5 = {}",
            self.info.out_report_len, self.info.flashtap_probe
        )]
    }

    fn keyboard_layout(&self) -> Option<DeviceKeyboardLayout> {
        if self.info.kind != CorsairDeviceKind::Keyboard {
            return None;
        }
        let desc = layout_for_device(self.info.vid, self.info.pid)?;
        let keys = keys_for_layout(&desc, self.info.layout)
            .into_iter()
            .map(|k| DeviceKeyGeom {
                code: k.code(),
                fallback_label: k.label_qwerty,
                x: k.x,
                y: k.y,
                width: k.width,
                height: k.height,
                led_index: k.led_index,
            })
            .collect();
        Some(DeviceKeyboardLayout {
            product_name: self.info.product_name.clone(),
            standard: self.info.layout,
            keys,
        })
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        match id {
            setting_ids::POLLING_RATE => {
                let rate = PollingRate::from_hz(parsed(value)?).context("fréquence invalide")?;
                self.set_pollrate(rate)?;
            }
            setting_ids::HW_BRIGHTNESS => self.set_hw_brightness_pct(number(value)? as u8)?,
            HW_PROFILE => self.set_active_hw_profile(parsed(value)?)?,
            setting_ids::WIN_LOCK => self.set_hw_winlock(flag(value)?)?,
            setting_ids::HW_SOCD => self.set_flashtap(flag(value)?)?,
            DEBOUNCE => {
                let ms = self.info.debounce_timeout_ms.unwrap_or(30);
                self.set_hw_debounce(flag(value)?, ms)?;
            }
            DEBOUNCE_MS => {
                let on = self.info.debounce_enabled.unwrap_or(true);
                self.set_hw_debounce(on, parsed(value)?)?;
            }
            LIFT_HEIGHT => self.set_mouse_lift_height(parsed(value)?)?,
            setting_ids::SIDETONE => {
                let pct = number(value)? as u8;
                self.set_headset_sidetone(pct, pct == 0)?;
            }
            other => {
                if let Some(&(_, _, prop_id)) =
                    INDICATOR_COLORS.iter().find(|(sid, _, _)| *sid == other)
                {
                    let hex = value.as_str().context("couleur hex attendue")?;
                    let col = Rgb::from_hex(hex).context("couleur hex invalide")?;
                    self.set_indicator_color(prop_id, col)?;
                    return Ok(());
                }
                let Some(&(_, _, bit)) = WINLOCK_SHORTCUTS.iter().find(|(sid, _, _)| *sid == other)
                else {
                    bail!("Réglage inconnu : {other}");
                };
                let mask = self
                    .info
                    .winlock_shortcuts_mask
                    .unwrap_or(DEFAULT_WINLOCK_MASK);
                let mask = if flag(value)? {
                    mask | bit
                } else {
                    mask & !bit
                };
                self.set_winlock_shortcuts(mask)?;
            }
        }
        Ok(())
    }
}

impl Lighting for CorsairDevice {
    fn zones(&self) -> Vec<Zone> {
        if self.info.kind != CorsairDeviceKind::Keyboard {
            return Vec::new();
        }
        let (_, keys) = self.active_keymap();
        let raw_keys = layout_for_device(self.info.vid, self.info.pid)
            .map(|d| d.keys)
            .unwrap_or(K70_TKL_KEYS);
        let leds = keys
            .iter()
            .map(|k| {
                let raw = raw_keys
                    .iter()
                    .find(|r| r.led_index == k.led_index)
                    .unwrap_or(k);
                let mut led = Led::at(k.label_qwerty, raw.norm_x(), raw.norm_y());
                if let Some(code) = k.code() {
                    led = led.key(code);
                }
                led
            })
            .collect();
        vec![Zone {
            id: "keys".into(),
            name: "Touches & voyants".into(),
            kind: ZoneKind::Matrix,
            leds,
            resizable: None,
        }]
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        if self.info.kind != CorsairDeviceKind::Keyboard || self.info.hwp_profiles.is_empty() {
            return Vec::new();
        }
        HwpLightingKind::ALL_SUPPORTED
            .iter()
            .map(|&kind| {
                let colors = match kind.color_count() {
                    0 => (0, 0),
                    1 => (1, 1),
                    _ => (1, 2),
                };
                let speed = if kind == HwpLightingKind::StaticColor {
                    None
                } else {
                    Some((0, 2))
                };
                let direction = matches!(
                    kind,
                    HwpLightingKind::RainbowWave
                        | HwpLightingKind::SpiralRainbow
                        | HwpLightingKind::ColorWave
                        | HwpLightingKind::Visor
                );
                HardwareEffect {
                    id: kind.id().into(),
                    label: kind.label().into(),
                    params: EffectParams {
                        colors,
                        speed,
                        brightness: None,
                        direction,
                        savable: true,
                    },
                    zones: Vec::new(),
                }
            })
            .collect()
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        if self.info.kind != CorsairDeviceKind::Keyboard {
            return Ok(());
        }
        let (led_count, keys) = self.active_keymap();
        let mut frame = vec![Rgb::BLACK; led_count];
        for (key, &col) in keys.iter().zip(colors) {
            if key.led_index < led_count {
                frame[key.led_index] = col;
            }
        }
        self.write_raw_frame(&frame)?;
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        if !req.save {
            bail!(
                "Les effets matériels Corsair s'enregistrent dans la mémoire flash du clavier (confirmation requise)"
            );
        }
        let prof_idx = self.info.active_hw_profile.unwrap_or(0) as usize;
        let kind = HwpLightingKind::parse_id(&req.effect);
        let primary = req.colors.first().copied().unwrap_or(Rgb::CYAN);
        let secondary = req.colors.get(1).copied().unwrap_or(primary);
        let speed = req.speed.unwrap_or(1);
        let direction = u8::from(req.reverse);
        self.flash_hwp_lighting(prof_idx, kind, primary, secondary, speed, direction, None)?;
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        self.set_mode(OperatingMode::Hardware)?;
        Ok(())
    }
}

impl PointerDevice for CorsairDevice {
    fn pointer_info(&self) -> Option<PointerDpiInfo> {
        let m = self.info.mouse.as_ref()?;
        let active = (m.active_stage as usize).min(m.dpi_stages.len().saturating_sub(1));
        let dpi_x = m.current_dpi_x.unwrap_or(m.dpi_stages[active]);
        let dpi_y = m.current_dpi_y.unwrap_or(dpi_x);
        let max_hz = self.info.max_pollrate.map_or(8000, |p| p.hz());
        let supported_rates_hz = [125, 250, 500, 1000, 2000, 4000, 8000]
            .into_iter()
            .filter(|&hz| hz <= max_hz)
            .collect();
        Some(PointerDpiInfo {
            dpi_x,
            dpi_y,
            min_dpi: 100,
            max_dpi: 30000,
            step_dpi: 50,
            independent_xy: false,
            supports_lod: false,
            lod: None,
            stages: m.dpi_stages.to_vec(),
            active_stage: Some(active),
            report_rate_hz: self.info.pollrate.map(|p| p.hz()),
            supported_rates_hz,
        })
    }

    fn set_dpi(&mut self, x: u16, _y: Option<u16>) -> Result<()> {
        let stage = self
            .info
            .mouse
            .as_ref()
            .map(|m| m.active_stage)
            .unwrap_or(0);
        self.set_mouse_dpi_stage(stage, x)
            .map_err(anyhow::Error::from)
    }

    fn set_dpi_stage(&mut self, stage: usize, dpi: u16) -> Result<()> {
        self.set_mouse_dpi_stage(stage as u8, dpi)
            .map_err(anyhow::Error::from)
    }

    fn set_active_dpi_stage(&mut self, stage: usize) -> Result<()> {
        self.set_mouse_active_stage(stage as u8)
            .map_err(anyhow::Error::from)
    }
}

impl OnboardMemory for CorsairDevice {
    fn onboard_info(&self) -> Option<OnboardMemoryInfo> {
        let active = self.info.active_hw_profile.unwrap_or(0) as u16;
        Some(OnboardMemoryInfo {
            mode: Some(if self.info.mode == OperatingMode::Software {
                "host".into()
            } else {
                "onboard".into()
            }),
            mode_label: Some(match self.info.mode {
                OperatingMode::Software => "Piloté par OpenGear".into(),
                OperatingMode::Hardware => "Profils flash Corsair".into(),
            }),
            profile_count: self.info.hwp_profiles.len() as u8,
            button_count: 0,
            active_profile: active,
            active_dpi_index: 0,
            writable: false,
            macros_supported: false,
        })
    }

    fn onboard_profiles(&self) -> Vec<OnboardProfileSummary> {
        let active = self.info.active_hw_profile.unwrap_or(0) as u16;
        self.info
            .hwp_profiles
            .iter()
            .map(|hp| OnboardProfileSummary {
                slot: hp.slot_index as u16,
                name: hp.name.clone(),
                active: hp.slot_index as u16 == active,
                dpi_stages: Vec::new(),
                default_dpi_idx: None,
                report_rate_hz: None,
                editable: false,
                sector_addr: Some(format!("0x{:04X}", hp.profile_file_id)),
                from_rom: false,
                crc_valid: None,
                buttons: Vec::new(),
            })
            .collect()
    }

    fn set_active_onboard_profile(&mut self, slot: u16) -> Result<()> {
        self.set_active_hw_profile(slot as u8)
            .map_err(anyhow::Error::from)
    }

    fn reload_onboard_profiles(&mut self) -> Result<()> {
        self.reload_hwp_profiles().map_err(anyhow::Error::from)
    }

    fn onboard_backups(&self) -> Vec<OnboardBackupEntry> {
        crate::list_corsair_backups()
            .into_iter()
            .map(|b| OnboardBackupEntry {
                file: b.file,
                tag: b.tag,
                sector_hex: b.file_id_hex,
                size: b.size,
                created_ms: b.created_ms,
            })
            .collect()
    }

    fn restore_onboard_backup(&mut self, file: &str) -> Result<String> {
        let file_id = self
            .with_flash_session(|t| crate::restore_corsair_backup(t, file))
            .map_err(anyhow::Error::from)?;
        let _ = self.reload_hwp_profiles();
        Ok(format!(
            "Sauvegarde Corsair {file} restaurée (fichier HWP 0x{file_id:04X})"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LightingEndpoint;
    use crate::device::CorsairDeviceInfo;
    use crate::keymap::K70_TKL_LED_COUNT;
    use crate::proto::*;
    use crate::transport::{BragiTransport, HidIo};
    use opengear_core::lighting::{Placement, render_frame};
    use opengear_core::{EffectKind, KeyboardLayout, RgbEffectConfig};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct RecordingHid {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl RecordingHid {
        fn new() -> Self {
            Self {
                writes: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl HidIo for RecordingHid {
        fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(data.len())
        }

        fn read_timeout(
            &self,
            buf: &mut [u8],
            _timeout_ms: i32,
        ) -> Result<usize, hidapi::HidError> {
            let writes = self.writes.lock().unwrap();
            let Some(last) = writes.last() else {
                return Ok(0);
            };
            let magic = last.get(1).copied().unwrap_or(0x08);
            let cmd = last.get(2).copied().unwrap_or(0);
            let reply = [0x00, magic, cmd, ERR_OK, 0, 0, 0];
            let n = reply.len().min(buf.len());
            buf[..n].copy_from_slice(&reply[..n]);
            Ok(n)
        }
    }

    fn fake_kb(io: RecordingHid, endpoint: LightingEndpoint) -> CorsairDevice {
        let t = BragiTransport::from_io(Box::new(io), 0, 1025, 1025);
        CorsairDevice::from_test_transport(
            t,
            CorsairDeviceInfo {
                id: "corsair_1B1C_1B73_sub0_TEST".into(),
                vid: CORSAIR_VID,
                pid: PID_K70_RGB_TKL,
                subdevice_id: 0,
                product_name: "K70 RGB TKL".into(),
                serial: "TEST".into(),
                kind: CorsairDeviceKind::Keyboard,
                firmware_version: "1.0.0".into(),
                bootloader_version: "1.0.0".into(),
                layout: KeyboardLayout::Iso,
                mode: OperatingMode::Hardware,
                pollrate: Some(PollingRate::Hz1000),
                max_pollrate: Some(PollingRate::Hz8000),
                hw_brightness_raw: Some(1000),
                brightness_is_coarse: false,
                out_report_len: 1025,
                in_report_len: 1025,
                lighting_endpoint: endpoint,
                battery_pct: None,
                battery_status: None,
                hw_winlock: None,
                winlock_shortcuts_mask: None,
                debounce_enabled: None,
                debounce_timeout_ms: None,
                esports_switch_on: None,
                active_hw_profile: Some(0),
                storage_total_kb: None,
                storage_free_kb: None,
                flashtap_supported: false,
                flashtap_enabled: None,
                flashtap_probe: String::new(),
                rapid_trigger_supported: false,
                hwp_profiles: Vec::new(),
                mouse: None,
                headset: None,
                cooler: None,
            },
        )
    }

    #[test]
    fn lighting_set_frame_matches_legacy_key_bytes_for_planar_and_triplets() {
        for endpoint in [LightingEndpoint::Planar0x01, LightingEndpoint::Triplets0x22] {
            let io_old = RecordingHid::new();
            let io_new = RecordingHid::new();
            let mut old_dev = fake_kb(io_old.clone(), endpoint);
            let mut new_dev = fake_kb(io_new.clone(), endpoint);

            let eff = RgbEffectConfig {
                kind: EffectKind::RainbowWave,
                brightness: 1.0,
                speed: 1.0,
                ..Default::default()
            };
            old_dev.render_effect(&eff, 0.25).unwrap();

            let zones = new_dev.zones();
            assert!(!zones.is_empty());
            assert!(
                zones[0]
                    .leds
                    .iter()
                    .any(|l| l.key_code.as_deref() == Some("Escape")),
                "keyboard LEDs must carry W3C key_code"
            );
            let frame = render_frame(&zones, &Placement::default(), &eff, 0.25);
            new_dev.set_frame(&frame).unwrap();

            {
                let w_old = io_old.writes.lock().unwrap();
                let w_new = io_new.writes.lock().unwrap();
                assert_eq!(w_old.len(), w_new.len(), "packet count must match");
                // Setup packets (PROP_MODE, PROP_LIGHTING_CTRL_REG, CMD_OPEN_HANDLE) are byte-identical.
                for idx in 0..w_old.len() - 1 {
                    assert_eq!(w_old[idx], w_new[idx], "setup packet {idx} mismatch");
                }
                // Every physical key in ISO keymap must have identical RGB bytes in the final frame packet.
                let p_old = w_old.last().unwrap();
                let p_new = w_new.last().unwrap();
                assert_eq!(&p_old[..8], &p_new[..8]);
                let (_, keys) = new_dev.active_keymap();
                for k in &keys {
                    match endpoint {
                        LightingEndpoint::Planar0x01 => {
                            let base = 8; // 1 (report id 0x00) + 7 (Bragi CMD_WRITE_DATA header)
                            assert_eq!(p_old[base + k.led_index], p_new[base + k.led_index]);
                            assert_eq!(
                                p_old[base + K70_TKL_LED_COUNT + k.led_index],
                                p_new[base + K70_TKL_LED_COUNT + k.led_index]
                            );
                            assert_eq!(
                                p_old[base + K70_TKL_LED_COUNT * 2 + k.led_index],
                                p_new[base + K70_TKL_LED_COUNT * 2 + k.led_index]
                            );
                        }
                        LightingEndpoint::Triplets0x22 => {
                            let base = 8 + 2; // + [0x12, 0x00] header
                            let off = base + k.led_index * 3;
                            assert_eq!(&p_old[off..off + 3], &p_new[off..off + 3]);
                        }
                    }
                }
            }

            new_dev.release().unwrap();
            let release_pkt = io_new.writes.lock().unwrap().last().unwrap().clone();
            assert_eq!(release_pkt[2], CMD_SET);
            assert_eq!(release_pkt[3], PROP_MODE);
            assert_eq!(release_pkt[5], MODE_HARDWARE as u8);
        }
    }

    #[test]
    fn hardware_effect_refuses_to_run_without_explicit_save() {
        let io = RecordingHid::new();
        let mut dev = fake_kb(io.clone(), LightingEndpoint::Planar0x01);
        let req = EffectRequest {
            effect: "StaticColor".into(),
            zone: None,
            colors: vec![Rgb::RED],
            speed: None,
            brightness: None,
            reverse: false,
            save: false,
        };
        assert!(dev.apply_hardware_effect(&req).is_err());
        assert!(io.writes.lock().unwrap().is_empty());
    }
}
