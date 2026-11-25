pub mod color;
pub mod cooling;
pub mod device;
pub mod driver;
pub mod fans;
pub mod keyboard;
pub mod lighting;
pub mod profile;
pub mod scanner;
pub mod serde_util;

pub use color::{EffectKind, Rgb, RgbEffectConfig};
pub use device::{DetectedDevice, DeviceVendor, KeyboardLayout, OperatingMode, PollingRate};
pub use keyboard::{FormFactor, PhysicalKey, physical_layout};
pub use profile::{AppConfig, KeyAction, KeyboardProfile, MacroStep, SocdConfig, SocdPair};
pub use scanner::scan_hid_devices;
