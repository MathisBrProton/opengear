pub mod aura;
pub mod cooling;
pub mod hp_omen;
pub mod laptop;
pub mod pawnio;
pub mod power;
pub mod ram_rgb;
pub mod sensors;
pub mod superio;
pub mod telemetry;
#[cfg(windows)]
pub(crate) mod wmi_com;

pub use aura::{
    ASUS_VID, AURA_MB_PIDS, AuraChannel, AuraChannelKind, AuraError, AuraHwMode,
    AuraMainboardController, AuraMainboardInfo,
};
pub use cooling::{
    AsusWmiCoolingStatus, CurvePoint, FanChannelConfig, FanHeaderKind, FanPolicyMode,
    FanTempSource, apply_asus_wmi_curve, is_elevated, probe_asus_wmi_cooling,
    read_asus_wmi_channel, relaunch_as_admin,
};
pub use hp_omen::{
    HPWMI_GM, HPWMI_KB, HpOmenStatus, OmenGpuPowerLevel, OmenKeyboardType, OmenThermalMode,
    probe_hp_omen, read_hp_omen_live_fans, set_hp_omen_fan_level, set_hp_omen_gpu_power,
    set_hp_omen_kb_backlight, set_hp_omen_kb_zones, set_hp_omen_max_fan, set_hp_omen_thermal_mode,
};
pub use laptop::{
    DisplayInfo, LaptopBatteryInfo, is_laptop_chassis, read_display_info, read_laptop_battery,
    set_display_brightness, set_display_refresh_rate,
};
pub use pawnio::{
    PawnIoAvailability, diagnose_smbus_readonly, install_official_pawnio, pawnio_availability,
    read_native_cpu_temp,
};
pub use superio::{
    SUPERIO_MIN_PWM_PCT, SuperIoFanChannel, SuperIoSavedState, SuperIoSnapshot,
    read_superio_snapshot, restore_all_superio_fans_auto, restore_superio_fan_auto,
    restore_superio_saved_states, safe_superio_duty, saved_superio_states, set_superio_fan_pwm,
    set_superio_fan_pwm_with_floor,
};
pub use power::{PowerPlan, list_power_plans, set_active_power_plan};
pub use ram_rgb::{
    EneRamMode, EneRamModule, PhysicalDimmInfo, RamRgbStatus, apply_ram_rgb, probe_ram_rgb,
    set_ram_modes_only,
    read_physical_dimms,
};
pub use sensors::{FanRpmReading, HardwareSensorsSnapshot, read_hardware_sensors};
pub use telemetry::{GpuTelemetry, StaticSystemInfo, SystemTelemetry, TelemetryCollector};
