use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceVendor {
    Corsair,
    Asus,
    Logitech,
    Alienware,
    Sony,
    Unknown(u16),
}

impl DeviceVendor {
    pub fn from_vid(vid: u16) -> Self {
        match vid {
            0x1B1C => Self::Corsair,
            0x0B05 => Self::Asus,
            0x046D => Self::Logitech,
            0x187C => Self::Alienware,
            0x054C => Self::Sony,
            other => Self::Unknown(other),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Corsair => "Corsair",
            Self::Asus => "ASUS ROG",
            Self::Logitech => "Logitech G",
            Self::Alienware => "Alienware",
            Self::Sony => "Sony",
            Self::Unknown(_) => "Autre",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PollingRate {
    Hz125,
    Hz250,
    Hz500,
    Hz1000,
    Hz2000,
    Hz4000,
    Hz8000,
}

impl PollingRate {
    pub const ALL: &'static [Self] = &[
        Self::Hz125,
        Self::Hz250,
        Self::Hz500,
        Self::Hz1000,
        Self::Hz2000,
        Self::Hz4000,
        Self::Hz8000,
    ];

    pub fn hz(self) -> u16 {
        match self {
            Self::Hz125 => 125,
            Self::Hz250 => 250,
            Self::Hz500 => 500,
            Self::Hz1000 => 1000,
            Self::Hz2000 => 2000,
            Self::Hz4000 => 4000,
            Self::Hz8000 => 8000,
        }
    }

    pub fn from_hz(hz: u16) -> Option<Self> {
        match hz {
            125 => Some(Self::Hz125),
            250 => Some(Self::Hz250),
            500 => Some(Self::Hz500),
            1000 => Some(Self::Hz1000),
            2000 => Some(Self::Hz2000),
            4000 => Some(Self::Hz4000),
            8000 => Some(Self::Hz8000),
            _ => None,
        }
    }

    /// Corsair Bragi wire value (property 0x01 / 0x96).
    /// Matches ckb-next: 1=8ms (125Hz), 2=4ms (250Hz), 3=2ms (500Hz),
    /// 4=1ms (1000Hz), 5=0.5ms (2000Hz), 6=0.25ms (4000Hz), 7=0.125ms (8000Hz).
    pub fn to_bragi_value(self) -> u16 {
        match self {
            Self::Hz125 => 1,
            Self::Hz250 => 2,
            Self::Hz500 => 3,
            Self::Hz1000 => 4,
            Self::Hz2000 => 5,
            Self::Hz4000 => 6,
            Self::Hz8000 => 7,
        }
    }

    pub fn from_bragi_value(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Hz125),
            2 => Some(Self::Hz250),
            3 => Some(Self::Hz500),
            4 => Some(Self::Hz1000),
            5 => Some(Self::Hz2000),
            6 => Some(Self::Hz4000),
            7 => Some(Self::Hz8000),
            _ => None,
        }
    }

    pub fn latency_label(self) -> &'static str {
        match self {
            Self::Hz125 => "8.0 ms",
            Self::Hz250 => "4.0 ms",
            Self::Hz500 => "2.0 ms",
            Self::Hz1000 => "1.0 ms",
            Self::Hz2000 => "0.5 ms",
            Self::Hz4000 => "0.25 ms",
            Self::Hz8000 => "0.125 ms (AXON)",
        }
    }
}

impl fmt::Display for PollingRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz ({})", self.hz(), self.latency_label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingMode {
    Hardware,
    Software,
}

impl fmt::Display for OperatingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hardware => write!(f, "Matériel (Onboard)"),
            Self::Software => write!(f, "Logiciel (Direct RGB / OpenGear)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyboardLayout {
    Ansi,
    Iso,
    Abnt,
    Jis,
    Unknown(u32),
}

impl KeyboardLayout {
    pub fn from_bragi_value(v: u32) -> Self {
        match v {
            1 => Self::Ansi,
            2 => Self::Iso,
            3 => Self::Abnt,
            4 => Self::Jis,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ansi => "ANSI (US)",
            Self::Iso => "ISO (FR / EU)",
            Self::Abnt => "ABNT (BR)",
            Self::Jis => "JIS (JP)",
            Self::Unknown(_) => "Inconnu",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedDevice {
    pub vendor: DeviceVendor,
    pub vid: u16,
    pub pid: u16,
    pub product_name: String,
    pub manufacturer: String,
    pub serial: String,
    pub interface_number: i32,
    pub usage_page: u16,
    pub usage: u16,
    pub path: String,
    pub driver_status: String,
}
