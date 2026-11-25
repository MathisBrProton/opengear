//! Device model shared by every driver crate.
//!
//! A driver crate (`opengear-<vendor>`) exposes its hardware through two traits:
//! - [`Driver`]: discovers that vendor's devices;
//! - [`Device`]: one physical device — identity, live status and its **settings**.
//!
//! Settings are described as data ([`Setting`]): the app and the UI render, validate and apply
//! them without knowing the vendor. Vendor-specific features stay vendor-specific (their own
//! setting ids, labels and protocol code in the vendor crate) but plug into the same pipeline.
//! Cross-cutting behaviours the app must react to use the shared ids in [`setting_ids`].
//!
//! Richer capabilities (synchronised lighting, equalizers, DPI stage editors, onboard memory, fan
//! curves) are added as further traits next to [`Device`], each with one generic UI.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// What a device is, for icons, grouping and default views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Headset,
    Mousepad,
    Controller,
    Motherboard,
    Memory,
    Cooler,
    LedController,
    Display,
    Laptop,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Usb,
    Wireless,
    Bluetooth,
    Internal,
}

/// Stable identity of a device.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceInfo {
    /// Stable across reconnections and restarts: `<driver>:<vid>:<pid>[:<serial or slot>]`.
    pub id: String,
    pub vendor: String,
    pub model: String,
    pub kind: DeviceKind,
    pub connection: Connection,
    pub firmware: Option<String>,
}

/// Live state that changes without user action.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DeviceStatus {
    pub battery_pct: Option<u8>,
    pub charging: Option<bool>,
    /// Estimated remaining battery runtime in hours when known by the driver.
    pub battery_hours: Option<u16>,
}

/// Where a setting is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    General,
    Performance,
    Lighting,
    Input,
    Audio,
    Power,
}

/// Who sees a setting: `Simple` for everyone, `Expert` only in expert mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Simple,
    Expert,
}

/// Where a new value lives once applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Persistence {
    /// Applied immediately, lost when the device is unplugged / reset.
    Live,
    /// Written to the device's own memory (survives unplugging).
    Device,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
}

impl ChoiceOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// How a setting is edited.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    Toggle,
    Slider {
        min: f64,
        max: f64,
        step: f64,
        unit: String,
    },
    Choice {
        options: Vec<ChoiceOption>,
    },
    /// `#RRGGBB`.
    Color,
}

/// A setting value. Choices and colours travel as text (`"1000"`, `"#FF0000"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// One user-facing setting of a device, with its current value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Setting {
    /// Unique within the device. Shared ids come from [`setting_ids`]; vendor-specific ones
    /// are prefixed with the vendor (`"corsair.flashtap"`).
    pub id: String,
    pub label: String,
    pub help: Option<String>,
    pub section: Section,
    pub level: Level,
    pub persistence: Persistence,
    pub control: Control,
    pub value: Value,
}

impl Setting {
    /// A setting in `section`, shown to everyone and applied live; refine with the builders.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        section: Section,
        control: Control,
        value: Value,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            help: None,
            section,
            level: Level::Simple,
            persistence: Persistence::Live,
            control,
            value,
        }
    }

    pub fn toggle(id: &str, label: &str, section: Section, on: bool) -> Self {
        Self::new(id, label, section, Control::Toggle, Value::Bool(on))
    }

    pub fn slider(
        id: &str,
        label: &str,
        section: Section,
        range: (f64, f64, f64),
        unit: &str,
        value: f64,
    ) -> Self {
        let (min, max, step) = range;
        Self::new(
            id,
            label,
            section,
            Control::Slider {
                min,
                max,
                step,
                unit: unit.to_string(),
            },
            Value::Number(value),
        )
    }

    pub fn choice(
        id: &str,
        label: &str,
        section: Section,
        options: Vec<ChoiceOption>,
        current: &str,
    ) -> Self {
        Self::new(
            id,
            label,
            section,
            Control::Choice { options },
            Value::Text(current.to_string()),
        )
    }

    pub fn color(id: &str, label: &str, section: Section, hex: &str) -> Self {
        Self::new(
            id,
            label,
            section,
            Control::Color,
            Value::Text(hex.to_string()),
        )
    }

    pub fn help(mut self, text: &str) -> Self {
        self.help = Some(text.to_string());
        self
    }

    pub fn expert(mut self) -> Self {
        self.level = Level::Expert;
        self
    }

    pub fn stored_in_device(mut self) -> Self {
        self.persistence = Persistence::Device;
        self
    }

    /// Check `value` against the control and normalise it (slider snapped and clamped, choice
    /// and colour verified). Drivers only ever receive validated values.
    pub fn validate(&self, value: &Value) -> Result<Value> {
        match (&self.control, value) {
            (Control::Toggle, Value::Bool(b)) => Ok(Value::Bool(*b)),
            (Control::Slider { min, max, step, .. }, Value::Number(n)) if n.is_finite() => {
                let snapped = if *step > 0.0 {
                    min + ((n - min) / step).round() * step
                } else {
                    *n
                };
                Ok(Value::Number(snapped.clamp(*min, *max)))
            }
            (Control::Choice { options }, Value::Text(id)) => {
                if options.iter().any(|o| &o.id == id) {
                    Ok(Value::Text(id.clone()))
                } else {
                    bail!("Valeur « {id} » invalide pour « {} »", self.label)
                }
            }
            (Control::Color, Value::Text(hex)) => crate::Rgb::from_hex(hex)
                .map(|c| Value::Text(c.to_hex()))
                .ok_or_else(|| anyhow::anyhow!("Couleur « {hex} » invalide")),
            _ => bail!("Type de valeur inattendu pour « {} »", self.label),
        }
    }
}

/// Setting ids with a meaning shared across vendors (the app reacts to them).
pub mod setting_ids {
    /// USB polling rate in Hz (choice of `"125"` … `"8000"`).
    pub const POLLING_RATE: &str = "polling_rate";
    /// Hardware backlight brightness in % (slider).
    pub const HW_BRIGHTNESS: &str = "hw_brightness";
    /// Hardware Windows-key lock (toggle).
    pub const WIN_LOCK: &str = "win_lock";
    /// Hardware SOCD / snap-tap handled by the keyboard itself (toggle). While on, the software
    /// SOCD of the input engine is turned off to avoid double processing.
    pub const HW_SOCD: &str = "hw_socd";
    /// Microphone sidetone in % (slider).
    pub const SIDETONE: &str = "sidetone";
    /// Angle snapping on a mouse sensor (toggle).
    pub const ANGLE_SNAPPING: &str = "angle_snapping";
    /// Performance / thermal profile of the machine (choice). Shared choice ids are in
    /// [`PROFILE_QUIET`], [`PROFILE_BALANCED`] and [`PROFILE_PERFORMANCE`]; devices may add
    /// their own extra options. The app's presets and battery saver use it.
    pub const PERFORMANCE_PROFILE: &str = "performance_profile";
    pub const PROFILE_QUIET: &str = "quiet";
    pub const PROFILE_BALANCED: &str = "balanced";
    pub const PROFILE_PERFORMANCE: &str = "performance";
    /// Built-in display brightness in % (slider).
    pub const DISPLAY_BRIGHTNESS: &str = "display_brightness";
    /// Built-in display refresh rate (choice of Hz strings, `"60"`, `"144"`…).
    pub const DISPLAY_REFRESH_RATE: &str = "display_refresh_rate";
    /// Keyboard backlight on/off (toggle). The battery saver and "lights off" preset turn it off.
    pub const BACKLIGHT: &str = "backlight";
}

/// One physical device handled by a driver crate.
pub trait Device: Send {
    fn info(&self) -> DeviceInfo;

    fn status(&self) -> DeviceStatus {
        DeviceStatus::default()
    }

    /// Settings with their current values. Must be cheap (cached state, no I/O).
    fn settings(&self) -> Vec<Setting> {
        Vec::new()
    }

    /// Lighting of the device, if it has LEDs (see [`crate::lighting`]).
    fn lighting(&mut self) -> Option<&mut dyn crate::lighting::Lighting> {
        None
    }

    /// Fans of the device, if OpenGear can drive them (see [`crate::fans`]).
    fn cooling(&mut self) -> Option<&mut dyn crate::fans::Cooling> {
        None
    }

    /// Extra lines for the diagnostic report (probe details, firmware state…).
    fn diagnostic_lines(&self) -> Vec<String> {
        Vec::new()
    }

    /// Refresh live telemetry/status (battery, etc.) if needed.
    fn refresh_status(&mut self) {}

    /// Optional downcast hook for capabilities not yet migrated to traits.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Optional mutable downcast hook for capabilities not yet migrated to traits.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    /// Apply a value already validated against the setting returned by [`Device::settings`].
    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        let _ = value;
        bail!("Réglage inconnu : {id}")
    }
}

/// Discovers the devices of one vendor / protocol.
pub trait Driver: Send + Sync {
    /// Short identifier, also the prefix of the device ids it produces.
    fn name(&self) -> &'static str;
    /// USB vendor ids this driver handles: a hot-plug of one of them triggers a new discovery.
    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[]
    }
    /// Name shown to the user (defaults to [`Driver::name`]).
    fn display_name(&self) -> &'static str {
        self.name()
    }
    /// Untested on real hardware: only discovered when the user enables it
    /// (`AppConfig::experimental_drivers`).
    fn experimental(&self) -> bool {
        false
    }
    /// Open every device currently connected. Called at start-up and on hot-plug.
    fn discover(&self) -> Vec<Box<dyn Device>>;
}

/// Find `id` among `device`'s settings and validate `value` for it.
pub fn validate_setting(device: &dyn Device, id: &str, value: &Value) -> Result<Value> {
    let Some(setting) = device.settings().into_iter().find(|s| s.id == id) else {
        bail!("Réglage « {id} » indisponible sur {}", device.info().model);
    };
    setting.validate(value)
}

/// Polling-rate choices up to `max_hz`.
pub fn polling_rate_options(rates_hz: impl IntoIterator<Item = u16>) -> Vec<ChoiceOption> {
    rates_hz
        .into_iter()
        .map(|hz| {
            let label = if hz >= 1000 {
                format!("{} kHz", f64::from(hz) / 1000.0)
            } else {
                format!("{hz} Hz")
            };
            ChoiceOption::new(hz.to_string(), label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        on: bool,
    }

    impl Device for Fake {
        fn info(&self) -> DeviceInfo {
            DeviceInfo {
                id: "fake:1".into(),
                vendor: "Fake".into(),
                model: "Fake 1".into(),
                kind: DeviceKind::Mouse,
                connection: Connection::Usb,
                firmware: None,
            }
        }
        fn settings(&self) -> Vec<Setting> {
            vec![
                Setting::toggle("t", "T", Section::General, self.on),
                Setting::slider("s", "S", Section::General, (0.0, 100.0, 5.0), "%", 50.0),
                Setting::choice(
                    "c",
                    "C",
                    Section::General,
                    polling_rate_options([125, 1000]),
                    "1000",
                ),
                Setting::color("k", "K", Section::Lighting, "#000000"),
            ]
        }
        fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
            if id == "t" {
                self.on = value.as_bool().unwrap();
            }
            Ok(())
        }
    }

    #[test]
    fn values_are_validated_and_normalised() {
        let d = Fake { on: false };
        assert_eq!(
            validate_setting(&d, "t", &Value::Bool(true)).unwrap(),
            Value::Bool(true)
        );
        assert!(validate_setting(&d, "t", &Value::Number(1.0)).is_err());
        assert_eq!(
            validate_setting(&d, "s", &Value::Number(123.0)).unwrap(),
            Value::Number(100.0)
        );
        assert_eq!(
            validate_setting(&d, "s", &Value::Number(42.0)).unwrap(),
            Value::Number(40.0)
        );
        assert!(validate_setting(&d, "s", &Value::Number(f64::NAN)).is_err());
        assert!(validate_setting(&d, "c", &Value::Text("500".into())).is_err());
        assert_eq!(
            validate_setting(&d, "c", &Value::Text("125".into())).unwrap(),
            Value::Text("125".into())
        );
        assert_eq!(
            validate_setting(&d, "k", &Value::Text("#ff0000".into())).unwrap(),
            Value::Text("#FF0000".into())
        );
        assert!(validate_setting(&d, "k", &Value::Text("rouge".into())).is_err());
        assert!(validate_setting(&d, "nope", &Value::Bool(true)).is_err());
    }

    #[test]
    fn settings_serialise_for_the_ui() {
        let json = serde_json::to_value(Fake { on: true }.settings()).unwrap();
        assert_eq!(json[0]["control"]["type"], "toggle");
        assert_eq!(json[0]["value"], true);
        assert_eq!(json[1]["control"]["unit"], "%");
        assert_eq!(json[2]["control"]["options"][1]["label"], "1 kHz");
        assert_eq!(json[0]["level"], "simple");
        assert_eq!(polling_rate_options([8000])[0].label, "8 kHz");
    }
}
