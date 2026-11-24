pub mod color;
pub mod device;
pub mod profile;
pub mod scanner;
pub mod serde_util;

pub use color::{EffectKind, Rgb, RgbEffectConfig};
pub use device::{DeviceVendor, DetectedDevice, KeyboardLayout, OperatingMode, PollingRate};
pub use profile::{AppConfig, KeyboardProfile, KeyAction, MacroStep, SocdConfig, SocdPair};
pub use scanner::scan_hid_devices;
