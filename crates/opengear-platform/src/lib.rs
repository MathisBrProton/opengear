//! Vendor-neutral Windows services shared by every OpenGear driver crate.
//!
//! Nothing in this crate may depend on a hardware brand: vendor-specific protocols (ASUS
//! `ASUSManagement`, HP `hpqBIntM`, …) live in their own crate and build on top of [`wmi`].

pub mod elevation;
pub mod keyboard;
pub mod laptop;
pub mod power;
pub mod telemetry;
pub mod wmi;

pub use elevation::{is_elevated, relaunch_as_admin};
pub use keyboard::{KeyResolver, OsKeyboardLayout};
pub use laptop::{
    DisplayInfo, LaptopBatteryInfo, is_laptop_chassis, read_display_info, read_laptop_battery,
    set_display_brightness, set_display_refresh_rate,
};
pub use power::{PowerPlan, list_power_plans, set_active_power_plan};
pub use telemetry::{GpuTelemetry, StaticSystemInfo, SystemTelemetry, TelemetryCollector};
