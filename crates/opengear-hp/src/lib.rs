//! HP OMEN / Victus support through the HP BIOS WMI interface (`root\wmi\hpqBIntM`).

pub mod omen;

pub use omen::{
    HPWMI_GM, HPWMI_KB, HpOmenDevice, HpOmenDriver, HpOmenStatus, OmenGpuPowerLevel,
    OmenKeyboardType, OmenThermalMode, probe_hp_omen, read_hp_omen_live_fans,
    set_hp_omen_fan_level, set_hp_omen_gpu_power, set_hp_omen_kb_backlight, set_hp_omen_kb_zones,
    set_hp_omen_max_fan, set_hp_omen_thermal_mode,
};
