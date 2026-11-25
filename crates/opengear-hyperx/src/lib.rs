//! HyperX peripherals (experimental), implemented strictly from `docs/protocols/hyperx.md`.
//!
//! Clean-room rule: zero code from third-party projects; all PIDs, report formats, and key
//! tables are taken exclusively from `docs/protocols/hyperx.md`.

pub mod model;
pub mod ops;
pub mod proto;
pub mod proto_ngenuity;
pub mod tables;

pub use model::{DRIVER_NAME, HyperXDevice, HyperXDriver};
