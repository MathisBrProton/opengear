pub mod features;
pub mod headset;
pub mod hidpp;
pub mod keyboard;
pub mod model;
pub mod mouse;
pub mod onboard;

pub use features::{FeatureEntry, feature_name};
pub use headset::{EQ_PRESETS, EqPreset, EqualizerInfo, LogitechHeadset, LogitechHeadsetInfo};
pub use hidpp::{HidppError, HidppTransport};
pub use keyboard::{KeyboardRgbMode, LogitechKeyboard, LogitechKeyboardInfo};
pub use model::LogitechDriver;
pub use mouse::{
    BatteryInfo, BatteryPowerModel, BatteryState, DpiSensorInfo, LedZoneInfo, LodLevel,
    LogitechMouse, LogitechMouseInfo, MouseLedMode, OnboardMode, OnboardProfilesSummary,
    ReportRateInfo, SmartShiftInfo,
};
pub use onboard::{
    MouseButtonAction, OnboardMacroStep, OnboardMemoryDesc, OnboardProfile, SectorBackup,
    backup_dir, backup_path_for, crc16_ccitt, list_sector_backups,
};
