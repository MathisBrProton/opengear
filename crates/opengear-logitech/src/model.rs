//! Logitech HID++ devices in the shared device model (`opengear_core::driver`).

use crate::hidpp::{BatteryInfo, BatteryState};
use crate::{LogitechHeadset, LogitechKeyboard, LogitechMouse, OnboardMode};
use anyhow::{Context, Result, bail};
use opengear_core::driver::{
    ChoiceOption, Connection, Device, DeviceInfo, DeviceKind, DeviceStatus, Section, Setting,
    Value, polling_rate_options, setting_ids,
};

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
