//! Logitech HID++ devices in the shared device model (`opengear_core::driver`).

use crate::features::{FEAT_RGB_EFFECTS, LOGITECH_VID};
use crate::hidpp::{BatteryInfo, BatteryState};
use crate::{
    KeyboardRgbMode, LogitechHeadset, LogitechKeyboard, LogitechMouse, MouseLedMode, OnboardMode,
};
use anyhow::{Context, Result, bail};
use opengear_core::Rgb;
use opengear_core::driver::{
    ChoiceOption, Connection, Device, DeviceInfo, DeviceKind, DeviceStatus, Driver, Section,
    Setting, Value, polling_rate_options, setting_ids,
};
use opengear_core::lighting::{EffectParams, EffectRequest, HardwareEffect, Lighting, Zone};
use opengear_core::{
    EqInfo, EqPresetInfo, Equalizer, OnboardMemory, OnboardProfileSummary, PointerDevice,
    PointerDpiInfo,
};

/// Driver discovering all Logitech HID++ 2.0 mice, keyboards and headsets.
pub struct LogitechDriver;

impl Driver for LogitechDriver {
    fn name(&self) -> &'static str {
        "logitech"
    }

    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[LOGITECH_VID]
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        let mut out: Vec<Box<dyn Device>> = Vec::new();
        out.extend(
            LogitechMouse::connect_all()
                .into_iter()
                .map(|d| Box::new(d) as Box<dyn Device>),
        );
        out.extend(
            LogitechKeyboard::connect_all()
                .into_iter()
                .map(|d| Box::new(d) as Box<dyn Device>),
        );
        out.extend(
            LogitechHeadset::connect_all()
                .into_iter()
                .map(|d| Box::new(d) as Box<dyn Device>),
        );
        out
    }
}

fn hidpp_speed_ms(speed: Option<u8>) -> u16 {
    let s = speed.unwrap_or(5).clamp(1, 10) as u16;
    if s == 5 { 4000 } else { (11 - s) * 800 }
}

const ONBOARD_MODE: &str = "logitech.onboard_mode";
const POWER_OFF_MINUTES: &str = "logitech.power_off_minutes";
const SMARTSHIFT_MODE: &str = "logitech.smartshift_mode";
const SMARTSHIFT_TORQUE: &str = "logitech.smartshift_torque";
const FN_INVERSION: &str = "logitech.fn_inversion";
const HEADSET_MIC_MUTE: &str = "logitech.mic_mute";
const HEADSET_BASS_TONE: &str = "logitech.bass_tone";
const HEADSET_EQ_PRESET: &str = "logitech.eq_preset";
/// HID++ device index of a device plugged in by cable (receivers use 1..=6).
const WIRED_INDEX: u8 = 0xFF;

fn sleep_timer_setting(minutes: u8) -> Setting {
    let opts = [0u8, 1, 2, 5, 10, 15, 30, 60]
        .into_iter()
        .map(|m| {
            let label = if m == 0 {
                "Jamais".to_string()
            } else if m == 1 {
                "1 minute".to_string()
            } else {
                format!("{m} minutes")
            };
            ChoiceOption::new(m.to_string(), label)
        })
        .collect();
    let val = [0u8, 1, 2, 5, 10, 15, 30, 60]
        .into_iter()
        .min_by_key(|&c| c.abs_diff(minutes))
        .unwrap_or(minutes);
    Setting::choice(
        POWER_OFF_MINUTES,
        "Mise en veille sans fil",
        Section::Power,
        opts,
        &val.to_string(),
    )
    .help("Délai d'inactivité avant mise en veille de l'appareil sans fil.")
}

fn status_of(battery: Option<&BatteryInfo>) -> DeviceStatus {
    DeviceStatus {
        battery_pct: battery.and_then(|b| b.percent),
        charging: battery.map(|b| b.state.is_charging() || b.state == BatteryState::Full),
        battery_hours: battery.and_then(|b| {
            if b.state.is_charging() || b.state == BatteryState::Full {
                None
            } else {
                b.estimated_hours
            }
        }),
    }
}

fn info(kind: DeviceKind, pid: u16, index: u8, name: &str, firmware: &[String]) -> DeviceInfo {
    DeviceInfo {
        id: format!("logitech:046D:{pid:04X}:{index}"),
        vendor: "Logitech G".into(),
        model: name.to_string(),
        kind,
        connection: if index == WIRED_INDEX || index == 0 {
            Connection::Usb
        } else {
            Connection::Wireless
        },
        firmware: firmware.first().cloned(),
    }
}

fn unknown(id: &str) -> Result<()> {
    bail!("Réglage inconnu : {id}")
}

impl Device for LogitechMouse {
    fn info(&self) -> DeviceInfo {
        let i = &self.info;
        info(
            DeviceKind::Mouse,
            i.pid,
            i.device_index,
            &i.name,
            &i.firmware,
        )
    }

    fn status(&self) -> DeviceStatus {
        status_of(self.info.battery.as_ref())
    }

    fn settings(&self) -> Vec<Setting> {
        let i = &self.info;
        let mut out = Vec::new();
        if let Some(rr) = i
            .report_rate
            .as_ref()
            .filter(|r| !r.supported_hz.is_empty())
        {
            out.push(Setting::choice(
                setting_ids::POLLING_RATE,
                "Fréquence d'envoi",
                Section::Performance,
                polling_rate_options(rr.supported_hz.iter().copied()),
                &rr.current_hz.to_string(),
            ));
        }
        if let Some(on) = i.angle_snapping {
            out.push(
                Setting::toggle(
                    setting_ids::ANGLE_SNAPPING,
                    "Correction d'angle",
                    Section::Performance,
                    on,
                )
                .help("Redresse les mouvements presque horizontaux ou verticaux.")
                .expert(),
            );
        }
        // An unknown mode is not offered: we could not show which option is current.
        let mode = i.onboard.as_ref().and_then(|ob| match ob.mode {
            OnboardMode::Onboard => Some("onboard"),
            OnboardMode::Host => Some("host"),
            OnboardMode::Unknown(_) => None,
        });
        if let Some(mode) = mode {
            out.push(
                Setting::choice(
                    ONBOARD_MODE,
                    "Mémoire de la souris",
                    Section::General,
                    vec![
                        ChoiceOption::new("onboard", "Profils enregistrés dans la souris"),
                        ChoiceOption::new("host", "Piloté par OpenGear"),
                    ],
                    mode,
                )
                .expert(),
            );
        }
        if let Some(ss) = i.smart_shift.as_ref() {
            let mode_val = if ss.wheel_mode == 1 {
                "freespin"
            } else {
                "ratchet"
            };
            out.push(
                Setting::choice(
                    SMARTSHIFT_MODE,
                    "Molette SmartShift",
                    Section::Input,
                    vec![
                        ChoiceOption::new("ratchet", "Crantée (avec débrayage)"),
                        ChoiceOption::new("freespin", "Défilement libre (FreeSpin)"),
                    ],
                    mode_val,
                )
                .help("Bascule le mécanisme magnétique de la molette SmartShift.")
                .expert(),
            );
            if ss.tunable_torque {
                out.push(
                    Setting::slider(
                        SMARTSHIFT_TORQUE,
                        "Force de crantage SmartShift",
                        Section::Input,
                        (1.0, f64::from(ss.max_torque.max(1)), 1.0),
                        "%",
                        f64::from(ss.torque.clamp(1, ss.max_torque.max(1))),
                    )
                    .expert(),
                );
            }
        }
        if let Some(mins) = i.power_off_minutes {
            out.push(sleep_timer_setting(mins));
        }
        out
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        (!self.info.led_zones.is_empty()).then_some(self as &mut dyn Lighting)
    }

    fn pointer(&mut self) -> Option<&mut dyn PointerDevice> {
        (self.info.dpi.is_some() || self.info.report_rate.is_some())
            .then_some(self as &mut dyn PointerDevice)
    }

    fn onboard(&mut self) -> Option<&mut dyn OnboardMemory> {
        (!self.info.onboard_profiles.is_empty()).then_some(self as &mut dyn OnboardMemory)
    }

    fn refresh_status(&mut self) {
        self.refresh_telemetry();
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        match id {
            setting_ids::POLLING_RATE => {
                let hz = value
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .context("fréquence invalide")?;
                self.set_report_rate_hz(hz)?;
            }
            setting_ids::ANGLE_SNAPPING => {
                self.set_angle_snapping(value.as_bool().context("booléen attendu")?)?
            }
            ONBOARD_MODE => {
                let mode = if value.as_str() == Some("host") {
                    OnboardMode::Host
                } else {
                    OnboardMode::Onboard
                };
                self.set_onboard_mode(mode)?;
                self.refresh_telemetry();
            }
            SMARTSHIFT_MODE => {
                let ss = self
                    .info
                    .smart_shift
                    .as_ref()
                    .context("SmartShift non supporté")?;
                let wheel = if value.as_str() == Some("freespin") {
                    1
                } else {
                    2
                };
                let (disengage, torque) = (ss.auto_disengage, ss.torque);
                self.set_smart_shift(wheel, disengage, Some(torque))?;
            }
            SMARTSHIFT_TORQUE => {
                let ss = self
                    .info
                    .smart_shift
                    .as_ref()
                    .context("SmartShift non supporté")?;
                let tq = (value.as_f64().context("nombre attendu")? as u8).clamp(1, ss.max_torque);
                let (wheel, disengage) = (ss.wheel_mode, ss.auto_disengage);
                self.set_smart_shift(wheel, disengage, Some(tq))?;
            }
            POWER_OFF_MINUTES => {
                let mins = value
                    .as_str()
                    .and_then(|s| s.parse::<u8>().ok())
                    .context("durée invalide")?;
                self.set_power_off_minutes(mins)?;
            }
            other => return unknown(other),
        }
        Ok(())
    }
}

impl Device for LogitechKeyboard {
    fn info(&self) -> DeviceInfo {
        let i = &self.info;
        info(
            DeviceKind::Keyboard,
            i.pid,
            i.device_index,
            &i.name,
            &i.firmware,
        )
    }

    fn status(&self) -> DeviceStatus {
        status_of(self.info.battery.as_ref())
    }

    fn settings(&self) -> Vec<Setting> {
        let i = &self.info;
        let mut out = Vec::new();
        if i.supports_brightness {
            out.push(Setting::slider(
                setting_ids::HW_BRIGHTNESS,
                "Luminosité matérielle",
                Section::Lighting,
                (0.0, 100.0, 1.0),
                "%",
                f64::from(i.brightness_pct.unwrap_or(100)),
            ));
        }
        if i.supports_game_mode {
            out.push(
                Setting::toggle(
                    setting_ids::WIN_LOCK,
                    "Mode Jeu (verrou touche Windows)",
                    Section::Input,
                    i.game_mode_enabled.unwrap_or(false),
                )
                .help("Désactive les touches Windows et Menu via 0x4521/0x4522 pour éviter les retours bureau en jeu."),
            );
        }
        if i.supports_fn_inversion {
            out.push(
                Setting::toggle(
                    FN_INVERSION,
                    "Inversion touche Fn (F1–F12 directs)",
                    Section::Input,
                    i.fn_inversion.unwrap_or(false),
                )
                .expert(),
            );
        }
        if let Some(mins) = i.power_off_minutes {
            out.push(sleep_timer_setting(mins));
        }
        out
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        self.info
            .supports_rgb_effects
            .then_some(self as &mut dyn Lighting)
    }

    fn refresh_status(&mut self) {
        self.refresh_telemetry();
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        match id {
            setting_ids::HW_BRIGHTNESS => {
                self.set_brightness_pct(value.as_f64().context("nombre attendu")? as u8)?
            }
            setting_ids::WIN_LOCK => {
                self.set_game_mode(value.as_bool().context("booléen attendu")?)?
            }
            FN_INVERSION => self.set_fn_inversion(value.as_bool().context("booléen attendu")?)?,
            POWER_OFF_MINUTES => {
                let mins = value
                    .as_str()
                    .and_then(|s| s.parse::<u8>().ok())
                    .context("durée invalide")?;
                self.set_power_off_minutes(mins)?;
            }
            other => return unknown(other),
        }
        Ok(())
    }
}

impl Device for LogitechHeadset {
    fn info(&self) -> DeviceInfo {
        let i = &self.info;
        let idx = if i.is_wireless && (i.device_index == WIRED_INDEX || i.device_index == 0) {
            1
        } else {
            i.device_index
        };
        info(DeviceKind::Headset, i.pid, idx, &i.name, &i.firmware)
    }

    fn status(&self) -> DeviceStatus {
        status_of(self.info.battery.as_ref())
    }

    fn settings(&self) -> Vec<Setting> {
        let i = &self.info;
        let mut out = Vec::new();
        if i.supports_sidetone {
            out.push(
                Setting::slider(
                    setting_ids::SIDETONE,
                    "Retour micro (sidetone)",
                    Section::Audio,
                    (0.0, 100.0, 1.0),
                    "%",
                    f64::from(i.sidetone_pct),
                )
                .help("T'entendre dans le casque quand tu parles."),
            );
        }
        if i.supports_mic_mute {
            out.push(Setting::toggle(
                HEADSET_MIC_MUTE,
                "Couper le micro (matériel)",
                Section::Audio,
                i.mic_muted.unwrap_or(false),
            ));
        }
        if let Some(bass) = i.bass_tone {
            out.push(Setting::slider(
                HEADSET_BASS_TONE,
                "Renforcement des basses",
                Section::Audio,
                (0.0, 100.0, 1.0),
                "%",
                f64::from(bass),
            ));
        }
        if i.equalizer.is_some() {
            let opts: Vec<ChoiceOption> = crate::EQ_PRESETS
                .iter()
                .map(|p| ChoiceOption::new(p.name, p.name))
                .collect();
            let cur = i.eq_preset.as_deref().unwrap_or(crate::EQ_PRESETS[0].name);
            if opts.iter().any(|o| o.id == cur) {
                out.push(Setting::choice(
                    HEADSET_EQ_PRESET,
                    "Profil égaliseur matériel",
                    Section::Audio,
                    opts,
                    cur,
                ));
            }
        }
        if let Some(mins) = i.power_off_minutes {
            out.push(sleep_timer_setting(mins));
        }
        out
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        (self.info.led_zones_count > 0).then_some(self as &mut dyn Lighting)
    }

    fn equalizer(&mut self) -> Option<&mut dyn Equalizer> {
        self.info
            .equalizer
            .is_some()
            .then_some(self as &mut dyn Equalizer)
    }

    fn refresh_status(&mut self) {
        self.refresh_telemetry();
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        match id {
            setting_ids::SIDETONE => {
                self.set_sidetone(value.as_f64().context("nombre attendu")? as u8)?
            }
            HEADSET_MIC_MUTE => self.set_mic_mute(value.as_bool().context("booléen attendu")?)?,
            HEADSET_BASS_TONE => {
                self.set_bass_tone(value.as_f64().context("nombre attendu")? as u8)?
            }
            HEADSET_EQ_PRESET => {
                let name = value.as_str().context("nom de profil attendu")?;
                self.apply_eq_preset(name)?;
            }
            POWER_OFF_MINUTES => {
                let mins = value
                    .as_str()
                    .and_then(|s| s.parse::<u8>().ok())
                    .context("durée invalide")?;
                self.set_power_off_minutes(mins)?;
            }
            other => return unknown(other),
        }
        Ok(())
    }
}

fn mouse_hw_effect(
    id: &str,
    label: &str,
    colors: (u8, u8),
    speed: Option<(u8, u8)>,
    zones: Vec<String>,
) -> HardwareEffect {
    HardwareEffect {
        id: id.into(),
        label: label.into(),
        params: EffectParams {
            colors,
            speed,
            brightness: None,
            direction: false,
            savable: false,
        },
        zones,
    }
}

impl Lighting for LogitechMouse {
    fn zones(&self) -> Vec<Zone> {
        let n = self.info.led_zones.len();
        self.info
            .led_zones
            .iter()
            .enumerate()
            .map(|(i, z)| {
                let y = if n <= 1 {
                    0.5
                } else {
                    0.35 + 0.3 * (i as f32 / (n - 1) as f32)
                };
                Zone::single(&format!("zone_{}", z.index), &z.location, 0.5, y)
            })
            .collect()
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        let zids: Vec<String> = self
            .info
            .led_zones
            .iter()
            .map(|z| format!("zone_{}", z.index))
            .collect();
        vec![
            mouse_hw_effect("Fixed", "Statique", (1, 1), None, zids.clone()),
            mouse_hw_effect(
                "Breathing",
                "Respiration",
                (1, 1),
                Some((1, 10)),
                zids.clone(),
            ),
            mouse_hw_effect(
                "Cycle",
                "Cycle couleur",
                (0, 0),
                Some((1, 10)),
                zids.clone(),
            ),
            mouse_hw_effect("Off", "Éteint", (0, 0), None, zids),
        ]
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        let zone_indices: Vec<u8> = self.info.led_zones.iter().map(|z| z.index).collect();
        for (i, zid) in zone_indices.into_iter().enumerate() {
            let Some(&col) = colors.get(i) else {
                break;
            };
            if self.last_frame.get(i) == Some(&col) {
                continue;
            }
            self.set_led_zone(zid, MouseLedMode::Fixed, col, 4000)?;
        }
        self.last_frame = colors
            .iter()
            .take(self.info.led_zones.len())
            .copied()
            .collect();
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        let mode = match req.effect.as_str() {
            "Off" => MouseLedMode::Off,
            "Breathing" => MouseLedMode::Breathing,
            "Cycle" => MouseLedMode::Cycle,
            _ => MouseLedMode::Fixed,
        };
        let color = req.colors.first().copied().unwrap_or(Rgb::CYAN);
        let speed_ms = hidpp_speed_ms(req.speed);
        let zone_indices: Vec<u8> = match req.zone.as_deref() {
            Some(z) => {
                let idx = z
                    .strip_prefix("zone_")
                    .and_then(|s| s.parse::<u8>().ok())
                    .context("Zone Logitech invalide")?;
                vec![idx]
            }
            None => self.info.led_zones.iter().map(|z| z.index).collect(),
        };
        for zid in zone_indices {
            self.set_led_zone(zid, mode, color, speed_ms)?;
        }
        self.last_frame.clear();
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        self.last_frame.clear();
        Ok(())
    }

    fn max_fps(&self) -> u8 {
        5
    }
}

impl Lighting for LogitechKeyboard {
    fn zones(&self) -> Vec<Zone> {
        let has_clusters = self.transport.get_feature_index(FEAT_RGB_EFFECTS).is_some();
        let count = if has_clusters {
            self.info.rgb_cluster_count.clamp(1, 4) as usize
        } else {
            1
        };
        (0..count)
            .map(|i| {
                let x = (i as f32 + 0.5) / count as f32;
                let name = if count == 1 {
                    "Rétroéclairage".to_string()
                } else {
                    format!("Zone {}", i + 1)
                };
                Zone::single(&format!("cluster_{i}"), &name, x, 0.5)
            })
            .collect()
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        let zids: Vec<String> = self.zones().into_iter().map(|z| z.id).collect();
        vec![
            mouse_hw_effect("Fixed", "Statique", (1, 1), None, zids.clone()),
            mouse_hw_effect(
                "Breathing",
                "Respiration",
                (1, 1),
                Some((1, 10)),
                zids.clone(),
            ),
            mouse_hw_effect(
                "Cycle",
                "Cycle de couleurs",
                (0, 0),
                Some((1, 10)),
                zids.clone(),
            ),
            mouse_hw_effect(
                "Wave",
                "Vague arc-en-ciel",
                (0, 0),
                Some((1, 10)),
                zids.clone(),
            ),
            mouse_hw_effect("Off", "Éteint", (0, 0), None, zids),
        ]
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        let count = self.zones().len();
        for i in 0..count {
            let Some(&col) = colors.get(i) else {
                break;
            };
            if self.last_frame.get(i) == Some(&col) {
                continue;
            }
            self.set_cluster_rgb(i as u8, KeyboardRgbMode::Fixed, col, 4000)?;
        }
        self.last_frame = colors.iter().take(count).copied().collect();
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        let mode = match req.effect.as_str() {
            "Off" => KeyboardRgbMode::Off,
            "Breathing" => KeyboardRgbMode::Breathing,
            "Cycle" => KeyboardRgbMode::Cycle,
            "Wave" => KeyboardRgbMode::Wave,
            _ => KeyboardRgbMode::Fixed,
        };
        let color = req.colors.first().copied().unwrap_or(Rgb::CYAN);
        let speed_ms = hidpp_speed_ms(req.speed);
        match req.zone.as_deref() {
            Some(z) => {
                let cluster = z
                    .strip_prefix("cluster_")
                    .and_then(|s| s.parse::<u8>().ok())
                    .context("Zone clavier Logitech invalide")?;
                self.set_cluster_rgb(cluster, mode, color, speed_ms)?;
            }
            None => self.set_rgb_effect(mode, color, speed_ms)?,
        }
        self.last_frame.clear();
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        self.last_frame.clear();
        Ok(())
    }

    fn max_fps(&self) -> u8 {
        5
    }
}

impl Lighting for LogitechHeadset {
    fn zones(&self) -> Vec<Zone> {
        let count = self.info.led_zones_count.clamp(1, 4) as usize;
        (0..count)
            .map(|i| {
                let name = if count == 1 {
                    "Éclairage casque".to_string()
                } else {
                    format!("Oreillette {}", i + 1)
                };
                Zone::single(&format!("zone_{i}"), &name, 0.5, 0.35 + 0.3 * (i as f32))
            })
            .collect()
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        let zids: Vec<String> = self.zones().into_iter().map(|z| z.id).collect();
        vec![
            mouse_hw_effect("Fixed", "Statique", (1, 1), None, zids.clone()),
            mouse_hw_effect(
                "Breathing",
                "Respiration",
                (1, 1),
                Some((1, 10)),
                zids.clone(),
            ),
            mouse_hw_effect(
                "Cycle",
                "Cycle couleur",
                (0, 0),
                Some((1, 10)),
                zids.clone(),
            ),
            mouse_hw_effect("Off", "Éteint", (0, 0), None, zids),
        ]
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        let count = self.info.led_zones_count.clamp(1, 4) as usize;
        for i in 0..count {
            let Some(&col) = colors.get(i) else {
                break;
            };
            if self.last_frame.get(i) == Some(&col) {
                continue;
            }
            self.set_led_zone(i as u8, MouseLedMode::Fixed, col, 4000)?;
        }
        self.last_frame = colors.iter().take(count).copied().collect();
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        let mode = match req.effect.as_str() {
            "Off" => MouseLedMode::Off,
            "Breathing" => MouseLedMode::Breathing,
            "Cycle" => MouseLedMode::Cycle,
            _ => MouseLedMode::Fixed,
        };
        let color = req.colors.first().copied().unwrap_or(Rgb::CYAN);
        let speed_ms = hidpp_speed_ms(req.speed);
        let count = self.info.led_zones_count.clamp(1, 4);
        match req.zone.as_deref() {
            Some(z) => {
                let zid = z
                    .strip_prefix("zone_")
                    .and_then(|s| s.parse::<u8>().ok())
                    .context("Zone casque Logitech invalide")?;
                self.set_led_zone(zid, mode, color, speed_ms)?;
            }
            None => {
                for zid in 0..count {
                    self.set_led_zone(zid, mode, color, speed_ms)?;
                }
            }
        }
        self.last_frame.clear();
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        self.last_frame.clear();
        Ok(())
    }

    fn max_fps(&self) -> u8 {
        5
    }
}

impl PointerDevice for LogitechMouse {
    fn pointer_info(&self) -> Option<PointerDpiInfo> {
        let dpi = self.info.dpi.as_ref();
        let rr = self.info.report_rate.as_ref();
        if dpi.is_none() && rr.is_none() {
            return None;
        }
        let active_stages = self
            .info
            .onboard
            .as_ref()
            .and_then(|ob| {
                self.info
                    .onboard_profiles
                    .iter()
                    .find(|p| u16::from(p.slot_index) == ob.active_profile)
                    .map(|p| p.dpi_stages.to_vec())
            })
            .unwrap_or_default();
        Some(PointerDpiInfo {
            dpi_x: dpi.map(|d| d.dpi_x).unwrap_or(0),
            dpi_y: dpi.map(|d| d.dpi_y).unwrap_or(0),
            min_dpi: dpi.map(|d| d.min_dpi).unwrap_or(100),
            max_dpi: dpi.map(|d| d.max_dpi).unwrap_or(32000),
            step_dpi: dpi.map(|d| d.step_dpi).unwrap_or(50),
            independent_xy: dpi.is_some_and(|d| d.supports_independent_xy),
            stages: active_stages,
            active_stage: self
                .info
                .onboard
                .as_ref()
                .map(|o| o.active_dpi_index as usize),
            report_rate_hz: rr.map(|r| r.current_hz),
            supported_rates_hz: rr.map(|r| r.supported_hz.clone()).unwrap_or_default(),
        })
    }

    fn set_dpi(&mut self, x: u16, y: Option<u16>) -> Result<()> {
        let lod = self.info.dpi.as_ref().and_then(|d| d.lod);
        LogitechMouse::set_dpi(self, x, y, lod).map_err(anyhow::Error::from)
    }

    fn set_active_dpi_stage(&mut self, stage: usize) -> Result<()> {
        LogitechMouse::set_active_dpi_stage(self, stage as u8).map_err(anyhow::Error::from)?;
        self.refresh_telemetry();
        Ok(())
    }
}

impl OnboardMemory for LogitechMouse {
    fn onboard_profiles(&self) -> Vec<OnboardProfileSummary> {
        let active_slot = self
            .info
            .onboard
            .as_ref()
            .map(|o| o.active_profile)
            .unwrap_or(1);
        self.info
            .onboard_profiles
            .iter()
            .map(|p| OnboardProfileSummary {
                slot: u16::from(p.slot_index),
                name: if p.name.is_empty() {
                    format!("Profil {}", p.slot_index)
                } else {
                    p.name.clone()
                },
                active: u16::from(p.slot_index) == active_slot,
                dpi_stages: p.dpi_stages.to_vec(),
                report_rate_hz: Some(p.report_rate_hz),
                editable: !p.from_rom,
            })
            .collect()
    }

    fn set_active_onboard_profile(&mut self, slot: u16) -> Result<()> {
        LogitechMouse::set_active_profile(self, slot).map_err(anyhow::Error::from)?;
        self.refresh_telemetry();
        Ok(())
    }

    fn reload_onboard_profiles(&mut self) -> Result<()> {
        LogitechMouse::reload_onboard_profiles(self).map_err(anyhow::Error::from)?;
        self.refresh_telemetry();
        Ok(())
    }
}

impl Equalizer for LogitechHeadset {
    fn eq_info(&self) -> Option<EqInfo> {
        let eq = self.info.equalizer.as_ref()?;
        Some(EqInfo {
            band_count: eq.band_count,
            min_db: eq.min_db,
            max_db: eq.max_db,
            frequencies_hz: eq.frequencies_hz.clone(),
            gains_db: eq.gains_db.clone(),
            presets: crate::EQ_PRESETS
                .iter()
                .enumerate()
                .map(|(i, p)| EqPresetInfo {
                    id: format!("preset_{i}"),
                    name: p.name.to_string(),
                    gains_db: p.gains_5.to_vec(),
                })
                .collect(),
        })
    }

    fn set_eq_gains(&mut self, gains: &[i8]) -> Result<()> {
        LogitechHeadset::set_equalizer_gains(self, gains).map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LedZoneInfo;
    use crate::features::{FEAT_COLOR_LED_EFFECTS, FEAT_RGB_EFFECTS, FeatureEntry};
    use crate::hidpp::{HidIo, HidppTransport, REPORT_ID_LONG};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct RecordingLogiIo {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl RecordingLogiIo {
        fn new() -> Self {
            Self {
                writes: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl HidIo for RecordingLogiIo {
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
            let Some(tx) = writes.last() else {
                return Ok(0);
            };
            let reply = [
                REPORT_ID_LONG,
                tx.get(1).copied().unwrap_or(0xFF),
                tx.get(2).copied().unwrap_or(0),
                tx.get(3).copied().unwrap_or(0),
                0,
                0,
                0,
            ];
            let n = reply.len().min(buf.len());
            buf[..n].copy_from_slice(&reply[..n]);
            Ok(n)
        }
    }

    fn feat(index: u8, id: u16) -> FeatureEntry {
        FeatureEntry {
            index,
            id,
            flags: 0,
            version: 0,
            name: String::new(),
        }
    }

    #[test]
    fn mouse_lighting_writes_exact_legacy_hidpp_bytes() {
        let io_old = RecordingLogiIo::new();
        let io_new = RecordingLogiIo::new();
        let zones = vec![
            LedZoneInfo {
                index: 0,
                location: "Primaire".into(),
                num_effects: 4,
            },
            LedZoneInfo {
                index: 1,
                location: "Logo G".into(),
                num_effects: 4,
            },
        ];

        let mut t_old = HidppTransport::from_io(Box::new(io_old.clone()), 0xFF, true);
        t_old.features = vec![feat(3, FEAT_COLOR_LED_EFFECTS)];
        let mut old_mouse = LogitechMouse::from_test_transport(t_old, zones.clone());

        let mut t_new = HidppTransport::from_io(Box::new(io_new.clone()), 0xFF, true);
        t_new.features = vec![feat(3, FEAT_COLOR_LED_EFFECTS)];
        let mut new_mouse = LogitechMouse::from_test_transport(t_new, zones);

        let col = Rgb::new(0x12, 0x34, 0x56);
        old_mouse
            .set_led_zone(0, MouseLedMode::Fixed, col, 4000)
            .unwrap();
        old_mouse
            .set_led_zone(1, MouseLedMode::Fixed, col, 4000)
            .unwrap();
        old_mouse
            .set_led_zone(0, MouseLedMode::Breathing, col, 4000)
            .unwrap();
        old_mouse
            .set_led_zone(1, MouseLedMode::Breathing, col, 4000)
            .unwrap();

        new_mouse.set_frame(&[col, col]).unwrap();
        // Repeat identical frame: must not re-send over wireless HID++!
        new_mouse.set_frame(&[col, col]).unwrap();
        new_mouse
            .apply_hardware_effect(&EffectRequest {
                effect: "Breathing".into(),
                zone: None,
                colors: vec![col],
                speed: Some(5),
                brightness: None,
                reverse: false,
                save: false,
            })
            .unwrap();

        assert_eq!(
            *io_old.writes.lock().unwrap(),
            *io_new.writes.lock().unwrap()
        );
    }

    #[test]
    fn keyboard_lighting_writes_exact_legacy_hidpp_bytes() {
        let io_old = RecordingLogiIo::new();
        let io_new = RecordingLogiIo::new();

        let mut t_old = HidppTransport::from_io(Box::new(io_old.clone()), 0xFF, true);
        t_old.features = vec![feat(4, FEAT_RGB_EFFECTS)];
        let mut old_kb = LogitechKeyboard::from_test_transport(t_old, 2, true);

        let mut t_new = HidppTransport::from_io(Box::new(io_new.clone()), 0xFF, true);
        t_new.features = vec![feat(4, FEAT_RGB_EFFECTS)];
        let mut new_kb = LogitechKeyboard::from_test_transport(t_new, 2, true);

        let col = Rgb::new(0xAA, 0xBB, 0xCC);
        old_kb
            .set_rgb_effect(KeyboardRgbMode::Fixed, col, 4000)
            .unwrap();
        old_kb
            .set_rgb_effect(KeyboardRgbMode::Wave, col, 4000)
            .unwrap();

        new_kb.set_frame(&[col, col]).unwrap();
        new_kb
            .apply_hardware_effect(&EffectRequest {
                effect: "Wave".into(),
                zone: None,
                colors: vec![],
                speed: None,
                brightness: None,
                reverse: false,
                save: false,
            })
            .unwrap();

        assert_eq!(
            *io_old.writes.lock().unwrap(),
            *io_new.writes.lock().unwrap()
        );
    }

    #[test]
    fn g513_keyboard_with_rgb_effects_is_keyboard_not_headset() {
        use crate::features::{FEAT_DISABLE_KEYS, FEAT_EQUALIZER, FEAT_SIDETONE};

        let io_kb = RecordingLogiIo::new();
        let mut t_kb = HidppTransport::from_io(Box::new(io_kb), 0xFF, true);
        t_kb.proto_major = 4;
        t_kb.features = vec![feat(4, FEAT_RGB_EFFECTS), feat(5, FEAT_DISABLE_KEYS)];
        assert!(LogitechKeyboard::is_keyboard(&t_kb));
        assert!(!LogitechHeadset::is_headset(&t_kb));

        let io_hs = RecordingLogiIo::new();
        let mut t_hs = HidppTransport::from_io(Box::new(io_hs), 0xFF, true);
        t_hs.proto_major = 2;
        t_hs.features = vec![feat(6, FEAT_SIDETONE), feat(7, FEAT_EQUALIZER)];
        assert!(!LogitechKeyboard::is_keyboard(&t_hs));
        assert!(LogitechHeadset::is_headset(&t_hs));
    }

    #[test]
    fn status_and_rated_battery_hours_work_cleanly() {
        use crate::hidpp::estimate_rated_battery_hours;
        // PRO X SUPERLIGHT (0x4093) at 100%, 1000Hz: 874 / (1.5456 + 2.3064 + 8.2951) ~= 72h
        let hours = estimate_rated_battery_hours(0x4093, "PRO X SUPERLIGHT", 100, Some(1000));
        assert_eq!(hours, Some(72));
        // PRO X WIRELESS headset (0x0ABA) at 50%: 10h
        assert_eq!(
            estimate_rated_battery_hours(0x0ABA, "PRO X WIRELESS", 50, None),
            Some(10)
        );
        let st_disch = status_of(Some(&BatteryInfo {
            percent: Some(50),
            voltage_mv: None,
            state: BatteryState::Discharging,
            estimated_hours: Some(10),
        }));
        assert_eq!(st_disch.battery_hours, Some(10));
        let st_chg = status_of(Some(&BatteryInfo {
            percent: Some(50),
            voltage_mv: None,
            state: BatteryState::Charging,
            estimated_hours: Some(10),
        }));
        assert_eq!(st_chg.battery_hours, None);
    }

    #[test]
    fn keyboard_ghub_parity_writes_verified_bytes() {
        use crate::features::{
            FEAT_DISABLE_KEYS, FEAT_FN_INVERSION, FEAT_GLOBAL_DEVICE_SETTINGS,
            FEAT_PER_KEY_LIGHTING, FEAT_PER_KEY_LIGHTING_V1,
        };

        let io = RecordingLogiIo::new();
        let mut t = HidppTransport::from_io(Box::new(io.clone()), 0xFF, true);
        t.features = vec![
            feat(1, FEAT_FN_INVERSION),
            feat(2, FEAT_DISABLE_KEYS),
            feat(3, FEAT_GLOBAL_DEVICE_SETTINGS),
            feat(4, FEAT_PER_KEY_LIGHTING),
            feat(5, FEAT_PER_KEY_LIGHTING_V1),
        ];
        let mut kb = LogitechKeyboard::from_test_transport(t, 1, true);
        kb.info.supports_fn_inversion = true;
        kb.info.supports_game_mode = true;
        kb.info.power_off_minutes = Some(15);

        kb.apply_setting(FN_INVERSION, &Value::Bool(true)).unwrap();
        kb.apply_setting(setting_ids::WIN_LOCK, &Value::Bool(true))
            .unwrap();
        kb.apply_setting(POWER_OFF_MINUTES, &Value::Text("30".into()))
            .unwrap();
        kb.set_all_per_key_color(Rgb::new(10, 20, 30)).unwrap();
        kb.set_per_key_batch(&[(0x04, Rgb::new(0x11, 0x22, 0x33))])
            .unwrap();

        let writes = io.writes.lock().unwrap();
        assert_eq!(writes[0][2..5], [1, 0x1A, 0x01]); // 0x40A2 fn 1 [1]
        assert_eq!(writes[1][2..5], [2, 0x2A, 0x07]); // 0x4521 fn 2 [0x07]
        assert_eq!(writes[2][2..7], [3, 0x5A, 0x00, 0x02, 30]); // 0x8103 fn 5 [0x0002, 30]
        assert_eq!(writes[3][2..9], [4, 0x4A, 0x00, 0x00, 10, 20, 30]); // 0x8080 fn 4 setAll
        assert_eq!(writes[4][2..5], [4, 0x5A, 0x00]); // 0x8080 fn 5 flush
        assert_eq!(
            writes[5][2..12],
            [5, 0x1A, 0x04, 0x11, 0x22, 0x33, 0xFF, 0xFF, 0xFF, 0xFF]
        ); // 0x8081 fn 1
        assert_eq!(writes[6][2..9], [5, 0x7A, 0x01, 0x00, 0x00, 0x00, 0x00]); // 0x8081 fn 7 FrameEnd
    }

    #[test]
    fn headset_ghub_parity_settings_and_lighting_work() {
        use crate::features::{
            FEAT_BASS_TONE, FEAT_CENTURION_MIC_MUTE, FEAT_EQUALIZER, FEAT_SIDETONE,
        };

        let io = RecordingLogiIo::new();
        let mut t = HidppTransport::from_io(Box::new(io.clone()), 0xFF, true);
        t.features = vec![
            feat(1, FEAT_SIDETONE),
            feat(2, FEAT_CENTURION_MIC_MUTE),
            feat(3, FEAT_BASS_TONE),
            feat(4, FEAT_EQUALIZER),
            feat(5, FEAT_RGB_EFFECTS),
        ];
        let mut hs = LogitechHeadset::from_test_transport(t);
        hs.info.supports_sidetone = true;
        hs.info.supports_mic_mute = true;
        hs.info.bass_tone = Some(25);
        hs.info.equalizer = Some(crate::EqualizerInfo {
            band_count: 5,
            min_db: -12,
            max_db: 12,
            frequencies_hz: vec![80, 240, 750, 2200, 6600],
            gains_db: vec![0; 5],
        });
        hs.info.led_zones_count = 1;
        io.writes.lock().unwrap().clear();

        hs.apply_setting(HEADSET_MIC_MUTE, &Value::Bool(true))
            .unwrap();
        hs.apply_setting(HEADSET_BASS_TONE, &Value::Number(60.0))
            .unwrap();
        hs.apply_setting(HEADSET_EQ_PRESET, &Value::Text("FPS (Pro G HUB)".into()))
            .unwrap();
        hs.set_frame(&[Rgb::new(0x01, 0x02, 0x03)]).unwrap();

        let writes = io.writes.lock().unwrap();
        assert_eq!(writes[0][2..5], [2, 0x1A, 0x01]);
        assert_eq!(writes[1][2..5], [3, 0x1A, 60]);
        assert_eq!(writes[2][2..10], [4, 0x3A, 0x02, 2, 2, 0, 4, 1]);
        assert_eq!(writes[3][2..9], [5, 0x1A, 0x00, 0x01, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn mouse_smartshift_and_power_off_settings_work() {
        use crate::features::{FEAT_GLOBAL_DEVICE_SETTINGS, FEAT_SMART_SHIFT_ENHANCED};

        let io = RecordingLogiIo::new();
        let mut t = HidppTransport::from_io(Box::new(io.clone()), 1, true);
        t.features = vec![
            feat(1, FEAT_SMART_SHIFT_ENHANCED),
            feat(2, FEAT_GLOBAL_DEVICE_SETTINGS),
        ];
        let mut mouse = LogitechMouse::from_test_transport(t, Vec::new());
        mouse.info.smart_shift = Some(crate::SmartShiftInfo {
            enhanced: true,
            wheel_mode: 2,
            auto_disengage: 25,
            torque: 50,
            max_torque: 100,
            tunable_torque: true,
        });
        mouse.info.power_off_minutes = Some(5);

        mouse
            .apply_setting(SMARTSHIFT_MODE, &Value::Text("freespin".into()))
            .unwrap();
        mouse
            .apply_setting(SMARTSHIFT_TORQUE, &Value::Number(75.0))
            .unwrap();
        mouse
            .apply_setting(POWER_OFF_MINUTES, &Value::Text("15".into()))
            .unwrap();

        let writes = io.writes.lock().unwrap();
        assert_eq!(writes[0][2..7], [1, 0x2A, 1, 25, 50]);
        assert_eq!(writes[1][2..7], [1, 0x2A, 1, 25, 75]);
        assert_eq!(writes[2][2..7], [2, 0x5A, 0x00, 0x02, 15]);
    }
}
