pub mod device;
pub mod hwp;
pub mod k70_tkl;
pub mod keymap;
pub mod model;
pub mod proto;
pub mod transport;

pub use device::{
    CorsairCoolerCaps, CorsairDevice, CorsairDeviceInfo, CorsairDeviceKind, CorsairHeadsetCaps,
    CorsairMouseCaps,
};
pub use hwp::{
    CorsairFlashBackup, HwpLightingKind, HwpLightingLayer, HwpProfileInfo, corsair_backup_path_for,
    list_corsair_backups, restore_corsair_backup,
};
pub use k70_tkl::{CorsairKeyboard, CorsairKeyboardInfo, LightingEndpoint};
pub use keymap::{
    K70_TKL_KEYS, K70_TKL_LED_COUNT, KeyDef, KeyboardLayoutDesc, key_code_for_led_index,
    keys_for_layout, layout_for_device,
};
pub use model::CorsairDriver;
pub use proto::BragiError;
