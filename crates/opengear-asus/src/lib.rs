//! ASUS hardware: Aura USB RGB, ASUS WMI fans, ENE DDR RAM RGB, plus (pending extraction)
//! the PawnIO / SuperIO / sensor plumbing used by ASUS mainboards.

pub mod aura;
pub mod cooling;
pub mod pawnio;
pub mod ram_rgb;
pub mod sensors;
pub mod superio;

pub use aura::{
    ASUS_VID, AURA_MB_PIDS, AuraChannel, AuraChannelKind, AuraError, AuraHwMode,
    AuraMainboardController, AuraMainboardInfo,
};
pub use cooling::{
    AsusWmiCoolingStatus, CurvePoint, FanChannelConfig, FanHeaderKind, FanPolicyMode,
    FanTempSource, apply_asus_wmi_curve, probe_asus_wmi_cooling, read_asus_wmi_channel,
};
pub use pawnio::{
    PawnIoAvailability, diagnose_smbus_readonly, install_official_pawnio, pawnio_availability,
    read_native_cpu_temp,
};
pub use ram_rgb::{
    EneRamMode, EneRamModule, PhysicalDimmInfo, RamRgbStatus, apply_ram_rgb, probe_ram_rgb,
    read_physical_dimms, set_ram_modes_only,
};
pub use sensors::{FanRpmReading, HardwareSensorsSnapshot, read_hardware_sensors};
pub use superio::{
    SUPERIO_MIN_PWM_PCT, SuperIoFanChannel, SuperIoSavedState, SuperIoSnapshot,
    read_superio_snapshot, restore_all_superio_fans_auto, restore_superio_fan_auto,
    restore_superio_saved_states, safe_superio_duty, saved_superio_states, set_superio_fan_pwm,
    set_superio_fan_pwm_with_floor,
};
