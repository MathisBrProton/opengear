//! Built-in laptop panel as a shared [`Device`]: brightness and refresh rate as settings.
//!
//! Vendor-neutral: brightness goes through `WmiMonitorBrightnessMethods`, the refresh rate through
//! the Win32 display settings (see [`crate::laptop`]).

use crate::laptop::DisplayInfo;
use anyhow::{Context, Result};
use opengear_core::driver::{
    ChoiceOption, Connection, Device, DeviceInfo, DeviceKind, Driver, Section, Setting, Value,
    setting_ids,
};

/// Reads and writes of the panel (Windows calls in production, a recorder in tests).
pub(crate) trait DisplayIo: Send {
    fn read(&self) -> DisplayInfo;
    fn set_brightness(&self, pct: u8) -> Result<()>;
    fn set_refresh_rate(&self, hz: u32) -> Result<u32>;
}

struct WindowsDisplayIo;

impl DisplayIo for WindowsDisplayIo {
    fn read(&self) -> DisplayInfo {
        crate::laptop::read_display_info()
    }
    fn set_brightness(&self, pct: u8) -> Result<()> {
        crate::laptop::set_display_brightness(pct)
    }
    fn set_refresh_rate(&self, hz: u32) -> Result<u32> {
        crate::laptop::set_display_refresh_rate(hz)
    }
}

/// Discovers the built-in panel of a laptop (nothing on a desktop chassis).
pub struct LaptopDisplayDriver;

impl Driver for LaptopDisplayDriver {
    fn name(&self) -> &'static str {
        "laptop_display"
    }

    fn display_name(&self) -> &'static str {
        "Écran du PC portable"
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        if !crate::laptop::is_laptop_chassis() {
            return Vec::new();
        }
        LaptopDisplayDevice::open(Box::new(WindowsDisplayIo))
            .map(|d| Box::new(d) as Box<dyn Device>)
            .into_iter()
            .collect()
    }
}

/// The built-in panel of a laptop.
pub struct LaptopDisplayDevice {
    io: Box<dyn DisplayIo>,
    info: DisplayInfo,
}

impl LaptopDisplayDevice {
    /// `None` when the panel exposes neither brightness nor refresh rates.
    pub(crate) fn open(io: Box<dyn DisplayIo>) -> Option<Self> {
        let info = io.read();
        (info.brightness_pct.is_some() || !info.supported_hz.is_empty())
            .then_some(Self { io, info })
    }

    /// Last read panel state.
    pub fn display_info(&self) -> &DisplayInfo {
        &self.info
    }
}

impl Device for LaptopDisplayDevice {
    fn info(&self) -> DeviceInfo {
        let model = if self.info.width > 0 {
            format!("Écran intégré {}×{}", self.info.width, self.info.height)
        } else {
            "Écran intégré".into()
        };
        DeviceInfo {
            id: "laptop_display:internal".into(),
            vendor: "Écran".into(),
            model,
            kind: DeviceKind::Display,
            connection: Connection::Internal,
            firmware: None,
        }
    }

    fn settings(&self) -> Vec<Setting> {
        let mut out = Vec::new();
        if let Some(pct) = self.info.brightness_pct {
            out.push(Setting::slider(
                setting_ids::DISPLAY_BRIGHTNESS,
                "Luminosité de l'écran",
                Section::Power,
                (0.0, 100.0, 1.0),
                "%",
                f64::from(pct),
            ));
        }
        if !self.info.supported_hz.is_empty() {
            let options = self
                .info
                .supported_hz
                .iter()
                .map(|hz| ChoiceOption::new(hz.to_string(), format!("{hz} Hz")))
                .collect();
            let current = self
                .info
                .current_hz
                .map(|hz| hz.to_string())
                .unwrap_or_default();
            out.push(
                Setting::choice(
                    setting_ids::DISPLAY_REFRESH_RATE,
                    "Fréquence de l'écran",
                    Section::Power,
                    options,
                    &current,
                )
                .help("Une fréquence plus basse économise la batterie."),
            );
        }
        out
    }

    fn diagnostic_lines(&self) -> Vec<String> {
        let d = &self.info;
        vec![format!(
            "Écran {}x{} @ {:?} Hz (supportés {:?}, luminosité {:?} %)",
            d.width, d.height, d.current_hz, d.supported_hz, d.brightness_pct
        )]
    }

    fn refresh_status(&mut self) {
        self.info = self.io.read();
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        match id {
            setting_ids::DISPLAY_BRIGHTNESS => {
                let pct = value.as_f64().context("nombre attendu")?.clamp(0.0, 100.0) as u8;
                self.io.set_brightness(pct)?;
                self.info.brightness_pct = Some(pct);
                Ok(())
            }
            setting_ids::DISPLAY_REFRESH_RATE => {
                let hz: u32 = value
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .context("fréquence attendue")?;
                self.io.set_refresh_rate(hz)?;
                self.info = self.io.read();
                Ok(())
            }
            other => anyhow::bail!("Réglage d'écran inconnu : {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Recorder {
        state: Arc<Mutex<DisplayInfo>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl DisplayIo for Recorder {
        fn read(&self) -> DisplayInfo {
            self.state.lock().unwrap().clone()
        }
        fn set_brightness(&self, pct: u8) -> Result<()> {
            self.calls.lock().unwrap().push(format!("brightness {pct}"));
            self.state.lock().unwrap().brightness_pct = Some(pct);
            Ok(())
        }
        fn set_refresh_rate(&self, hz: u32) -> Result<u32> {
            self.calls.lock().unwrap().push(format!("hz {hz}"));
            self.state.lock().unwrap().current_hz = Some(hz);
            Ok(hz)
        }
    }

    fn panel() -> Recorder {
        let r = Recorder::default();
        *r.state.lock().unwrap() = DisplayInfo {
            brightness_pct: Some(80),
            current_hz: Some(144),
            supported_hz: vec![60, 144],
            width: 1920,
            height: 1080,
        };
        r
    }

    #[test]
    fn a_panel_without_controls_is_not_a_device() {
        assert!(LaptopDisplayDevice::open(Box::new(Recorder::default())).is_none());
    }

    #[test]
    fn settings_describe_brightness_and_refresh_rates() {
        let dev = LaptopDisplayDevice::open(Box::new(panel())).unwrap();
        let s = dev.settings();
        assert_eq!(s[0].id, setting_ids::DISPLAY_BRIGHTNESS);
        assert_eq!(s[0].value, Value::Number(80.0));
        assert_eq!(s[1].id, setting_ids::DISPLAY_REFRESH_RATE);
        assert_eq!(s[1].value, Value::Text("144".into()));
        assert_eq!(dev.info().kind, DeviceKind::Display);
        assert!(
            opengear_core::driver::validate_setting(
                &dev,
                setting_ids::DISPLAY_REFRESH_RATE,
                &Value::Text("75".into())
            )
            .is_err()
        );
    }

    #[test]
    fn applying_settings_updates_the_cached_state() {
        let rec = panel();
        let mut dev = LaptopDisplayDevice::open(Box::new(rec.clone())).unwrap();
        dev.apply_setting(setting_ids::DISPLAY_BRIGHTNESS, &Value::Number(35.0))
            .unwrap();
        dev.apply_setting(setting_ids::DISPLAY_REFRESH_RATE, &Value::Text("60".into()))
            .unwrap();
        assert_eq!(*rec.calls.lock().unwrap(), ["brightness 35", "hz 60"]);
        assert_eq!(dev.display_info().brightness_pct, Some(35));
        assert_eq!(dev.display_info().current_hz, Some(60));
    }
}
