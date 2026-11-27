//! Clean-room NZXT USB HID AIO (`Kraken X53/Z53/2023`) and fan hub (`Smart Device V2` /
//! `RGB & Fan Controller`) cooling driver implementing `opengear_core::driver`.

pub mod device;
pub mod proto;

pub use device::{NzxtCoolingDevice, NzxtDriver};
