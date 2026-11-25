//! Razer peripherals (experimental), implemented from `docs/protocols/razer.md` only.
//!
//! Clean-room rule: this crate is written from the protocol sheet; no code from other projects
//! (OpenRGB, OpenRazer…) is used. Disabled unless the user enables the experimental driver.

pub mod model;
pub mod proto;
mod tables;

pub use model::{DRIVER_NAME, RazerDevice, RazerDriver};
