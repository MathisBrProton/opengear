//! Registry of hardware backends: the one place where driver crates are plugged into the app.
//!
//! Supporting a new transport or vendor = implementing the relevant trait in its crate and adding
//! it to the matching list here. Nothing else in the app names a vendor implementation.

use opengear_asus::{AsusAuraDriver, AsusWmiRamBackend};
use opengear_core::driver::Driver;
use opengear_corsair::CorsairDriver;
use opengear_hp::HpOmenDriver;
use opengear_logitech::LogitechDriver;
use opengear_mainboard::{PawnIoRamBackend, RamRgb, RamRgbDriver};
use opengear_openrgb::OpenRgbRamBackend;

/// Device drivers discovered through the shared model (`opengear_core::driver`).
///
/// A new vendor plugs in here: its crate implements `Driver` + `Device`, and one line below makes
/// its devices appear with their status, settings and lighting in every view.
pub fn device_drivers() -> Vec<Box<dyn Driver>> {
    vec![
        Box::new(CorsairDriver),
        Box::new(LogitechDriver),
        Box::new(AsusAuraDriver),
        Box::new(RamRgbDriver::new(ram_rgb())),
        Box::new(HpOmenDriver),
    ]
}

/// RAM RGB backends, in probe order: native SMBus first, then the board vendor interface, then
/// OpenRGB (the only one usable without administrator rights).
pub fn ram_rgb() -> RamRgb {
    RamRgb::new(vec![
        Box::new(PawnIoRamBackend),
        Box::new(AsusWmiRamBackend),
        Box::new(OpenRgbRamBackend),
    ])
}
