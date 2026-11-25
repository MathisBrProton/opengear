use serde::{Deserialize, Serialize};

pub const LOGITECH_VID: u16 = 0x046D;

// Core HID++ 2.0 & Centurion Feature IDs (verified from G HUB lghub_agent.exe & hardware traces)
pub const FEAT_ROOT: u16 = 0x0000;
pub const FEAT_FEATURE_SET: u16 = 0x0001;
pub const FEAT_DEVICE_FW_VERSION: u16 = 0x0003;
pub const FEAT_DEVICE_NAME: u16 = 0x0005;
pub const FEAT_DEVICE_FRIENDLY_NAME: u16 = 0x0007;
pub const FEAT_BATTERY_STATUS: u16 = 0x1000;
pub const FEAT_BATTERY_VOLTAGE: u16 = 0x1001;
pub const FEAT_UNIFIED_BATTERY: u16 = 0x1004;
pub const FEAT_LED_CONTROL: u16 = 0x1300;
pub const FEAT_BACKLIGHT: u16 = 0x1982;
pub const FEAT_KEYBOARD_BACKLIGHT: u16 = 0x1983;
pub const FEAT_ILLUMINATION_LIGHT: u16 = 0x1990;
pub const FEAT_REPROG_CONTROLS_V4: u16 = 0x1B04;
pub const FEAT_WIRELESS_DEVICE_STATUS: u16 = 0x1D4B;
pub const FEAT_ADC_MEASUREMENT: u16 = 0x1F20;
pub const FEAT_LEFT_RIGHT_SWAP: u16 = 0x2001;
pub const FEAT_SMART_SHIFT: u16 = 0x2110;
pub const FEAT_SMART_SHIFT_ENHANCED: u16 = 0x2111;
pub const FEAT_HIRES_WHEEL: u16 = 0x2121;
pub const FEAT_RATCHET_WHEEL: u16 = 0x2130;
pub const FEAT_ADJUSTABLE_DPI: u16 = 0x2201;
pub const FEAT_EXTENDED_ADJUSTABLE_DPI: u16 = 0x2202;
pub const FEAT_POINTER_SPEED: u16 = 0x2205;
pub const FEAT_ANGLE_SNAPPING: u16 = 0x2230;
pub const FEAT_SURFACE_TUNING: u16 = 0x2240;
pub const FEAT_FN_INVERSION: u16 = 0x40A2;
pub const FEAT_MULTI_HOST_FN_INVERSION: u16 = 0x40A3;
pub const FEAT_DISABLE_KEYS: u16 = 0x4521;
pub const FEAT_DISABLE_KEYS_BY_USAGE: u16 = 0x4522;
pub const FEAT_DISABLE_CONTROLS: u16 = 0x4523;
pub const FEAT_GKEY: u16 = 0x8010;
pub const FEAT_BRIGHTNESS_CONTROL: u16 = 0x8040;
pub const FEAT_REPORT_RATE: u16 = 0x8060;
pub const FEAT_EXTENDED_REPORT_RATE: u16 = 0x8061;
pub const FEAT_COLOR_LED_EFFECTS: u16 = 0x8070;
pub const FEAT_RGB_EFFECTS: u16 = 0x8071;
pub const FEAT_PER_KEY_LIGHTING: u16 = 0x8080;
pub const FEAT_PER_KEY_LIGHTING_V1: u16 = 0x8081;
pub const FEAT_ONBOARD_PROFILES: u16 = 0x8100;
pub const FEAT_GLOBAL_DEVICE_SETTINGS: u16 = 0x8103;
pub const FEAT_MOUSE_BUTTON_SPY: u16 = 0x8110;
pub const FEAT_LATENCY_MONITORING: u16 = 0x8111;
pub const FEAT_SIDETONE: u16 = 0x8300;
pub const FEAT_BASS_TONE: u16 = 0x8305;
pub const FEAT_EQUALIZER: u16 = 0x8310;
pub const FEAT_HEADSET_OUT: u16 = 0x8320;
// Centurion audio / power features (also used on newer Centurion headsets)
pub const FEAT_CENTURION_AUTO_SLEEP: u16 = 0x0108;
pub const FEAT_CENTURION_RGB_EFFECTS: u16 = 0x0600;
pub const FEAT_CENTURION_MIC_MUTE: u16 = 0x0601;
pub const FEAT_HEADSET_AUDIO_SIDETONE: u16 = 0x0604;
pub const FEAT_CENTURION_ANC: u16 = 0x0605;
pub const FEAT_CENTURION_HEADSET_TONES: u16 = 0x060B;
pub const FEAT_HEADSET_MIC_GAIN: u16 = 0x0611;
pub const FEAT_CENTURION_BATTERY_SAVER: u16 = 0x0618;
pub const FEAT_CENTURION_ONBOARD_EQ: u16 = 0x0636;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureEntry {
    pub index: u8,
    pub id: u16,
    pub flags: u8,
    #[serde(default)]
    pub version: u8,
    pub name: String,
}

pub fn feature_name(id: u16) -> &'static str {
    match id {
        0x0000 => "ROOT",
        0x0001 => "FEATURE_SET",
        0x0002 => "FEATURE_INFO",
        0x0003 => "DEVICE_FW_VERSION",
        0x0004 => "DEVICE_UNIT_ID",
        0x0005 => "DEVICE_NAME",
        0x0006 => "DEVICE_GROUPS",
        0x0007 => "DEVICE_FRIENDLY_NAME",
        0x0008 => "KEEP_ALIVE",
        0x0011 => "PROPERTY_ACCESS",
        0x0020 => "CONFIG_CHANGE",
        0x0021 => "CRYPTO_ID",
        0x0030 => "TARGET_SOFTWARE",
        0x0080 => "WIRELESS_SIGNAL_STRENGTH",
        0x00C0..=0x00C3 => "DFU_CONTROL",
        0x00D0 => "DFU",
        0x1000 => "BATTERY_STATUS",
        0x1001 => "BATTERY_VOLTAGE",
        0x1004 => "UNIFIED_BATTERY",
        0x1010 => "CHARGING_CONTROL",
        0x1300 => "LED_CONTROL",
        0x1982 => "BACKLIGHT",
        0x1983 => "KEYBOARD_BACKLIGHT",
        0x1990 => "ILLUMINATION_LIGHT",
        0x1802 => "DEVICE_RESET",
        0x1805 => "OOB_STATE",
        0x1806 => "CONFIG_DEVICE_PROPS",
        0x1814 => "CHANGE_HOST",
        0x1815 => "HOSTS_INFO",
        0x1B00..=0x1B04 => "REPROG_CONTROLS",
        0x1B05 => "FULL_KEY_CUSTOMIZATION",
        0x1B0C => "ANALOG_BUTTONS",
        0x1BC0 => "REPORT_HID_USAGE",
        0x1C00 => "PERSISTENT_REMAPPABLE_ACTION",
        0x1D4B => "WIRELESS_DEVICE_STATUS",
        0x1E00 => "ENABLE_HIDDEN_FEATURES",
        0x1F1F => "FIRMWARE_PROPERTIES",
        0x1F20 => "ADC_MEASUREMENT",
        0x2001 => "LEFT_RIGHT_SWAP",
        0x2100 => "VERTICAL_SCROLLING",
        0x2110 => "SMART_SHIFT",
        0x2111 => "SMART_SHIFT_ENHANCED",
        0x2120 => "HI_RES_SCROLLING",
        0x2121 => "HIRES_WHEEL",
        0x2130 => "RATCHET_WHEEL",
        0x2150 => "THUMB_WHEEL",
        0x2200 => "MOUSE_POINTER",
        0x2201 => "ADJUSTABLE_DPI",
        0x2202 => "EXTENDED_ADJUSTABLE_DPI",
        0x2205 => "POINTER_SPEED",
        0x2230 => "ANGLE_SNAPPING",
        0x2240 => "SURFACE_TUNING",
        0x2250 => "XY_STATS",
        0x2251 => "WHEEL_STATS",
        0x2400 => "HYBRID_TRACKING",
        0x40A2 => "FN_INVERSION",
        0x40A3 => "MULTI_HOST_FN_INVERSION",
        0x4521 => "DISABLE_KEYS",
        0x4522 => "DISABLE_KEYS_BY_USAGE",
        0x4523 => "DISABLE_CONTROLS",
        0x8010 => "GKEY",
        0x8040 => "BRIGHTNESS_CONTROL",
        0x8060 => "REPORT_RATE",
        0x8061 => "EXTENDED_REPORT_RATE",
        0x8070 => "COLOR_LED_EFFECTS",
        0x8071 => "RGB_EFFECTS",
        0x8080 => "PER_KEY_LIGHTING",
        0x8081 => "PER_KEY_LIGHTING_V1",
        0x8090 => "MODE_STATUS",
        0x8100 => "ONBOARD_PROFILES",
        0x8101 => "PROFILE_MANAGEMENT",
        0x8103 => "GLOBAL_DEVICE_SETTINGS",
        0x8110 => "MOUSE_BUTTON_SPY",
        0x8111 => "LATENCY_MONITORING",
        0x8300 => "SIDETONE",
        0x8305 => "BASS_TONE",
        0x8310 => "EQUALIZER",
        0x8320 => "HEADSET_OUT",
        0x0108 => "CENTURION_AUTO_SLEEP",
        0x0600 => "CENTURION_RGB_EFFECTS",
        0x0601 => "CENTURION_MIC_MUTE",
        0x0604 => "HEADSET_AUDIO_SIDETONE",
        0x0605 => "CENTURION_ANC",
        0x060B => "CENTURION_HEADSET_TONES",
        0x0611 => "HEADSET_MIC_GAIN",
        0x0618 => "CENTURION_BATTERY_SAVER",
        0x0636 => "CENTURION_ONBOARD_EQ",
        _ => "UNKNOWN_FEATURE",
    }
}
