//! SteelSeries peripherals (experimental), implemented strictly from `docs/protocols/steelseries.md`.
//!
//! Clean-room rule: zero code from third-party projects; data tables and byte sequences are
//! derived exclusively from the project's protocol sheet.

pub mod model;
pub mod proto;
pub mod tables;

pub use model::{DRIVER_NAME, SteelSeriesDevice, SteelSeriesDriver};
