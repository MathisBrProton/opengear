//! ASUS hardware: Aura USB RGB controllers, `ASUSManagement` WMI fan curves and the ASUS WMI
//! SMBus transport for RAM RGB.

pub mod aura;
pub mod cooling;
pub mod ram_wmi;

pub use aura::{
    ASUS_VID, AURA_MB_PIDS, AsusAuraDriver, AuraChannel, AuraChannelKind, AuraError, AuraHwMode,
    AuraMainboardController, AuraMainboardInfo,
};
pub use cooling::{
    AsusWmiCoolingStatus, apply_asus_wmi_curve, probe_asus_wmi_cooling, read_asus_wmi_channel,
};
pub use ram_wmi::AsusWmiRamBackend;
