//! Fan control capability of a device, shared by every vendor.
//!
//! A device whose fans OpenGear can drive (laptop embedded controller, AIO, fan hub…) exposes them
//! through [`Cooling`]: live speed, a manual target with a safety floor, and a way to give control
//! back to the firmware. The app keeps the crash watchdog in sync with [`Cooling::overridden`] and
//! calls [`Cooling::restore_firmware_control`] on exit or after a crash.

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

/// One fan of a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FanInfo {
    /// Stable within the device (`"cpu"`, `"gpu"`, `"fan_0"`…).
    pub id: String,
    pub name: String,
    /// Live speed, when the device reports it.
    pub rpm: Option<u32>,
    /// Highest speed the fan reaches, when known.
    pub max_rpm: Option<u32>,
    /// Lowest manual duty allowed (safety floor), in %.
    pub min_pct: u8,
    /// Manual duty currently forced by OpenGear, `None` = firmware control.
    pub manual_pct: Option<u8>,
}

/// Requested fan behaviour. Serialised as `"auto"` or a duty in %.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanTarget {
    /// Firmware / BIOS curve.
    Auto,
    /// Fixed duty in %.
    Fixed(u8),
}

impl Serialize for FanTarget {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::Fixed(p) => s.serialize_u8(*p),
        }
    }
}

impl<'de> Deserialize<'de> for FanTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Number(f64),
        }
        match Raw::deserialize(d)? {
            Raw::Text(t) if t.eq_ignore_ascii_case("auto") => Ok(Self::Auto),
            Raw::Text(t) => Err(serde::de::Error::custom(format!(
                "cible de ventilateur invalide : « {t} »"
            ))),
            Raw::Number(n) if n.is_finite() => Ok(Self::Fixed(n.round().clamp(0.0, 100.0) as u8)),
            Raw::Number(_) => Err(serde::de::Error::custom("cible de ventilateur invalide")),
        }
    }
}

/// Clamp a fixed target to the fan's safety floor (and 100 %).
pub fn clamp_target(info: &FanInfo, target: FanTarget) -> FanTarget {
    match target {
        FanTarget::Auto => FanTarget::Auto,
        FanTarget::Fixed(p) => FanTarget::Fixed(p.clamp(info.min_pct.min(100), 100)),
    }
}

/// Fans of a device that OpenGear can drive.
pub trait Cooling {
    fn fans(&self) -> Vec<FanInfo>;

    /// `fan = None` = every fan of the device. Fixed values are already clamped to the floor
    /// (see [`clamp_target`]).
    fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()>;

    /// Re-read live speeds.
    fn refresh(&mut self) {}

    /// How often [`Cooling::keep_alive`] must run while overridden (`None` = never).
    fn keep_alive_interval(&self) -> Option<Duration> {
        None
    }

    /// Re-send the manual state to firmware that reverts it after a while.
    fn keep_alive(&mut self) -> Result<()> {
        Ok(())
    }

    /// True while the firmware is NOT in control (manual speed, max fan…).
    fn overridden(&self) -> bool;

    /// Give fan control back to the firmware. Must work on a freshly discovered device too
    /// (crash watchdog), so it may not rely on remembered state.
    fn restore_firmware_control(&mut self) -> Result<()>;
}

/// Firmware/BIOS fan-curve capability shared by motherboard controllers (ASUS Q-Fan WMI, etc.).
pub trait CurveCooling: Cooling {
    /// Current firmware/Smart-Cooling configuration of every header.
    fn curve_channels(&self) -> Vec<crate::cooling::FanChannelConfig>;

    /// Re-read the live curves & policies directly from BIOS/firmware.
    fn read_firmware_curves(&mut self) -> Result<Vec<crate::cooling::FanChannelConfig>>;

    /// Apply one channel configuration (`Manual` curve, preset policy, pump limits…).
    fn apply_channel_curve(&mut self, cfg: &crate::cooling::FanChannelConfig) -> Result<()>;

    /// Diagnostic pair `(firmware_curves_available, human_status_message)`.
    fn cooling_diagnostic(&self) -> (bool, String);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fan(min: u8) -> FanInfo {
        FanInfo {
            id: "cpu".into(),
            name: "CPU".into(),
            rpm: Some(2000),
            max_rpm: Some(5500),
            min_pct: min,
            manual_pct: None,
        }
    }

    #[test]
    fn fixed_targets_are_clamped_to_the_floor() {
        let f = fan(30);
        assert_eq!(clamp_target(&f, FanTarget::Fixed(5)), FanTarget::Fixed(30));
        assert_eq!(clamp_target(&f, FanTarget::Fixed(65)), FanTarget::Fixed(65));
        assert_eq!(
            clamp_target(&f, FanTarget::Fixed(250)),
            FanTarget::Fixed(100)
        );
        assert_eq!(clamp_target(&f, FanTarget::Auto), FanTarget::Auto);
        assert_eq!(
            clamp_target(&fan(200), FanTarget::Fixed(10)),
            FanTarget::Fixed(100)
        );
    }

    #[test]
    fn fan_target_serde_uses_auto_or_a_number() {
        assert_eq!(serde_json::to_string(&FanTarget::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&FanTarget::Fixed(65)).unwrap(), "65");
        let a: FanTarget = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(a, FanTarget::Auto);
        let n: FanTarget = serde_json::from_str("70").unwrap();
        assert_eq!(n, FanTarget::Fixed(70));
        let r: FanTarget = serde_json::from_str("70.6").unwrap();
        assert_eq!(r, FanTarget::Fixed(71));
        let big: FanTarget = serde_json::from_str("1000").unwrap();
        assert_eq!(big, FanTarget::Fixed(100));
        assert!(serde_json::from_str::<FanTarget>("\"max\"").is_err());
        assert!(serde_json::from_str::<FanTarget>("true").is_err());
    }
}
