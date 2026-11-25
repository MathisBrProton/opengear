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
/// HID++ device index of a device plugged in by cable (receivers use 1..=6).
const WIRED_INDEX: u8 = 0xFF;

fn status_of(battery: Option<&BatteryInfo>) -> DeviceStatus {
    DeviceStatus {
        battery_pct: battery.and_then(|b| b.percent),
        charging: battery.map(|b| {
            matches!(
                b.state,
                BatteryState::Charging
                    | BatteryState::SlowCharging
                    | BatteryState::AlmostFull
                    | BatteryState::Full
            )
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
        out
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        (!self.info.led_zones.is_empty()).then_some(self as &mut dyn Lighting)
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
        if !i.supports_brightness {
            return Vec::new();
        }
        vec![Setting::slider(
            setting_ids::HW_BRIGHTNESS,
            "Luminosité matérielle",
            Section::Lighting,
            (0.0, 100.0, 1.0),
            "%",
            f64::from(i.brightness_pct.unwrap_or(100)),
        )]
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        self.info
            .supports_rgb_effects
            .then_some(self as &mut dyn Lighting)
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
            other => return unknown(other),
        }
        Ok(())
    }
}

impl Device for LogitechHeadset {
    fn info(&self) -> DeviceInfo {
        let i = &self.info;
        info(
            DeviceKind::Headset,
            i.pid,
            WIRED_INDEX,
            &i.name,
            &i.firmware,
        )
    }

    fn status(&self) -> DeviceStatus {
        status_of(self.info.battery.as_ref())
    }

    fn settings(&self) -> Vec<Setting> {
        let i = &self.info;
        if !i.supports_sidetone {
            return Vec::new();
        }
        vec![
            Setting::slider(
                setting_ids::SIDETONE,
                "Retour micro (sidetone)",
                Section::Audio,
                (0.0, 100.0, 1.0),
                "%",
                f64::from(i.sidetone_pct),
            )
            .help("T'entendre dans le casque quand tu parles."),
        ]
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
}
