//! Motherboard chips that are not tied to one board vendor: SMBus host controllers, ENE DRAM RGB
//! controllers, Nuvoton SuperIO fan control, CPU package temperature and the sensor snapshot.
//!
//! Board-vendor specifics (ASUS WMI…) live in their vendor crate and plug in through the traits
//! defined here (`smbus::SmbusBus`, `ram::RamRgbBackend`).

pub mod cpu_temp;
pub mod ene;
pub mod ram;
pub mod sensors;
pub mod smbus;
pub mod superio;

pub use cpu_temp::read_native_cpu_temp;
pub use ram::{
    EneRamMode, EneRamModule, PawnIoRamBackend, PhysicalDimmInfo, RamEffect, RamRgb, RamRgbBackend,
    RamRgbDevice, RamRgbDriver, RamRgbStatus, read_physical_dimms,
};
pub use sensors::{FanRpmReading, HardwareSensorsSnapshot, read_hardware_sensors};
pub use smbus::diagnose_smbus_readonly;
pub use superio::{
    SUPERIO_MIN_PWM_PCT, SuperIoFanChannel, SuperIoSavedState, SuperIoSnapshot,
    read_superio_snapshot, restore_all_superio_fans_auto, restore_superio_fan_auto,
    restore_superio_saved_states, safe_superio_duty, saved_superio_states, set_superio_fan_pwm,
    set_superio_fan_pwm_with_floor,
};
