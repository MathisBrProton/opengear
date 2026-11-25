//! Corsair Bragi devices in the shared device model (`opengear_core::driver`).

use crate::device::{CorsairDevice, CorsairDeviceKind};
use anyhow::{Context, Result, bail};
use opengear_core::PollingRate;
use opengear_core::driver::{
    ChoiceOption, Connection, Device, DeviceInfo, DeviceKind, DeviceStatus, Section, Setting,
    Value, polling_rate_options, setting_ids,
};

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
            charging: None,
        }
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
        out
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
