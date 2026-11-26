pub mod audio;
pub mod color;
pub mod cooling;
pub mod device;
pub mod driver;
pub mod fans;
pub mod keyboard;
pub mod lighting;
pub mod onboard;
pub mod pointer;
pub mod profile;
pub mod scanner;
pub mod serde_util;

pub use audio::{EqInfo, EqPresetInfo, Equalizer};
pub use color::{BlendMode, EffectKind, Rgb, RgbEffectConfig, RgbLayer, blend_rgb};
pub use device::{DetectedDevice, DeviceVendor, KeyboardLayout, OperatingMode, PollingRate};
pub use driver::DeviceSpec;
pub use fans::{Cooling, CurveCooling, FanInfo, FanTarget};
pub use keyboard::{FormFactor, PhysicalKey, physical_layout};
pub use onboard::{
    OnboardBackupEntry, OnboardButtonBinding, OnboardMacroStep, OnboardMemory, OnboardMemoryInfo,
    OnboardProfilePatch, OnboardProfileSummary,
};
pub use pointer::{PointerDevice, PointerDpiInfo};
pub use profile::{
    AppConfig, KeyAction, KeyboardProfile, MacroMode, MacroStep, SocdConfig, SocdPair,
};
pub use scanner::scan_hid_devices;
