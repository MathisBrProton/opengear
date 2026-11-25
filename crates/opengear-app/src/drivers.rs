//! Registry of hardware backends: the one place where driver crates are plugged into the app.
//!
//! Supporting a new transport or vendor = implementing the relevant trait in its crate and adding
//! it to the matching list here. Nothing else in the app names a vendor implementation.

use opengear_asus::AsusWmiRamBackend;
use opengear_core::driver::Driver;
use opengear_mainboard::{PawnIoRamBackend, RamRgb};
use opengear_openrgb::OpenRgbRamBackend;

/// Device drivers discovered through the shared model (`opengear_core::driver`).
///
/// A new vendor plugs in here: its crate implements `Driver` + `Device`, and one line below makes
/// its devices appear with their status and settings in every view. (Corsair and Logitech are
/// still opened by the lighting engine directly and join the shared list from there.)
pub fn device_drivers() -> Vec<Box<dyn Driver>> {
    Vec::new()
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
