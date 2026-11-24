use thiserror::Error;

pub const CORSAIR_VID: u16 = 0x1B1C;
pub const PID_K70_RGB_TKL: u16 = 0x1B73;
pub const PID_K70_RGB_TKL_CS: u16 = 0x1BB9;

pub const BRAGI_USAGE_PAGE: u16 = 0xFF42;
pub const BRAGI_USAGE_CMD: u16 = 0x0001;
pub const BRAGI_USAGE_NOTIFY: u16 = 0x0002;

/// Wired/host Bragi header byte (`0x08 | subdevice_id` where `0` = host/wired, `1..=7` = Slipstream subdevice).
pub const BRAGI_MAGIC: u8 = 0x08;

// Commands (verified via REA + Ghidra 12.2 against Corsair iCUE5 BragiProtocols.dll CommandId enum)
pub const CMD_SET: u8 = 0x01;
pub const CMD_GET: u8 = 0x02;
pub const CMD_CLOSE_HANDLE: u8 = 0x05; // UnbindBuffers
pub const CMD_WRITE_DATA: u8 = 0x06; // WriteBufferBegin
pub const CMD_CONTINUE_WRITE: u8 = 0x07; // WriteBuffer
pub const CMD_READ_DATA: u8 = 0x08; // ReadBuffer
pub const CMD_PROBE_HANDLE: u8 = 0x09; // DescribeBuffer
pub const CMD_CREATE_FILE: u8 = 0x0B; // CreateFile
pub const CMD_REMOVE_FILE: u8 = 0x0C; // RemoveFile
pub const CMD_OPEN_HANDLE: u8 = 0x0D; // OpenFile
pub const CMD_RESET_DEFAULT: u8 = 0x0F; // ResetDefault
pub const CMD_KEEPALIVE: u8 = 0x12; // Ping

// Bragi v2 Onboard Flash File IDs (verified from BragiProtocols.dll QMetaObject FileId enum)
pub const FILE_REALTIME_RGB: u16 = 0x0001;
pub const FILE_KEY_MAPPINGS: u16 = 0x0002;
pub const FILE_STORED_PROFILE_1: u16 = 0x0007;
pub const FILE_STORED_PROFILE_2: u16 = 0x0008;
pub const FILE_STORED_PROFILE_3: u16 = 0x0009;
pub const FILE_PROFILES_LIST: u16 = 0x000F;
pub const FILE_LISTING: u16 = 0x0011;
pub const FILE_REALTIME_TRIPLETS: u16 = 0x0022;
pub const FILE_FREE_MIN: u16 = 0x6D60; // 28000
pub const FILE_FREE_MAX: u16 = 0x7FFF; // 32767

// Bragi v2 HWP Layout Headers (2-byte LE magic at offset 0 of every HWP flash file)
pub const LAYOUT_PROFILE: u16 = 0xF11E;
pub const LAYOUT_ASSIGNMENT_MAP: u16 = 0xAC3D;
pub const LAYOUT_MACRO_ACTION: u16 = 0x30AC;
pub const LAYOUT_TEXT_ACTION: u16 = 0x03AC;
pub const LAYOUT_REMAP_ACTION: u16 = 0x076E;
pub const LAYOUT_PROPERTY_MAP: u16 = 0xD43D;
pub const LAYOUT_BLOB_CONTAINER: u16 = 0xB10B;
pub const LAYOUT_LIGHTINGS_LIST: u16 = 0x6C69;
pub const LAYOUT_SPIRAL_RAINBOW: u16 = 0xAB87;
pub const LAYOUT_SPIRAL: u16 = 0x5BA1;
pub const LAYOUT_COLOR_WAVE: u16 = 0x7BFF;
pub const LAYOUT_VISOR: u16 = 0x90C0;
pub const LAYOUT_RAIN: u16 = 0xA07E;
pub const LAYOUT_RIPPLE_TYPE: u16 = 0x09A2;
pub const LAYOUT_GRADIENT_TYPE: u16 = 0xB1F9;
pub const LAYOUT_RAINBOW_WAVE: u16 = 0xB94C;
pub const LAYOUT_RAINBOW: u16 = 0xA402;
pub const LAYOUT_COLOR_SHIFT: u16 = 0xA5FA;
pub const LAYOUT_COLOR_PULSE: u16 = 0xAD4F;
pub const LAYOUT_STATIC_COLOR: u16 = 0x207E;
pub const LAYOUT_SOLID: u16 = 0x0087;
pub const LAYOUT_GRADIENT: u16 = 0xEBF4;
pub const LAYOUT_RIPPLE: u16 = 0x0AAE;
pub const LAYOUT_WAVE: u16 = 0x1FEA;

// Property IDs (verified via REA + Ghidra 12.2 against Corsair iCUE5 BragiProtocols.dll)
pub const PROP_POLLRATE: u8 = 0x01;
pub const PROP_BRIGHTNESS: u8 = 0x02; // 0..=1000
pub const PROP_MODE: u8 = 0x03; // 1 = Hardware, 2 = Software
pub const PROP_LIFT_HEIGHT: u8 = 0x07;
pub const PROP_ANGLE_SNAPPING_ANGLE: u8 = 0x08;
pub const PROP_PRESS_DEBOUNCE_ENABLED: u8 = 0x09;
pub const PROP_PRESS_DEBOUNCE_TIMEOUT: u8 = 0x0A; // raw ms
pub const PROP_POWER_SAVING_ENABLED: u8 = 0x0B;
pub const PROP_AUTO_SLEEP_ENABLED: u8 = 0x0D;
pub const PROP_AUTO_SLEEP_TIMEOUT: u8 = 0x0E;
pub const PROP_BATTERY_LEVEL: u8 = 0x0F; // 0..=1000 (tenths of percent)
pub const PROP_BATTERY_STATUS: u8 = 0x10;
pub const PROP_VID: u8 = 0x11;
pub const PROP_PID: u8 = 0x12;
pub const PROP_APP_VER: u8 = 0x13;
pub const PROP_BLD_VER: u8 = 0x14;

// Mouse DPI stages & live resolution
pub const PROP_DPI_STAGE_1: u8 = 0x18;
pub const PROP_DPI_STAGE_2: u8 = 0x19;
pub const PROP_DPI_STAGE_3: u8 = 0x1A;
pub const PROP_DPI_STAGE_4: u8 = 0x1B;
pub const PROP_DPI_STAGE_5: u8 = 0x1C;
pub const PROP_DPI_STAGE_SNIPER: u8 = 0x1D;
pub const PROP_DPI_CURRENT_STAGE: u8 = 0x1E;
pub const PROP_DPI_ENABLED_STAGES: u8 = 0x1F;
pub const PROP_DPI_CURRENT: u8 = 0x20;
pub const PROP_DPI_CURRENT_X: u8 = 0x21;
pub const PROP_DPI_CURRENT_Y: u8 = 0x22;
pub const PROP_DPI_STAGE_COLOR_1: u8 = 0x2F;
pub const PROP_DPI_STAGE_COLOR_2: u8 = 0x30;
pub const PROP_DPI_STAGE_COLOR_3: u8 = 0x31;
pub const PROP_DPI_STAGE_COLOR_4: u8 = 0x32;
pub const PROP_DPI_STAGE_COLOR_5: u8 = 0x33;
pub const PROP_DPI_STAGE_COLOR_SNIPER: u8 = 0x34;

// Wireless subdevices & storage
pub const PROP_CONNECTED_SUBDEVICES: u8 = 0x36;
pub const PROP_RELEASE_DEBOUNCE_TIMEOUT: u8 = 0x38;
pub const PROP_RELEASE_DEBOUNCE_ENABLED: u8 = 0x39;
pub const PROP_TOTAL_STORAGE_SIZE: u8 = 0x3D;
pub const PROP_FREE_STORAGE_SIZE: u8 = 0x3E;
pub const PROP_HWLAYOUT: u8 = 0x41;
pub const PROP_BRIGHTNESS_COARSE: u8 = 0x44; // 0..=3
pub const PROP_WINLOCK_ENABLED: u8 = 0x45;

// Headset audio
pub const PROP_SIDETONE_MUTED: u8 = 0x46;
pub const PROP_SIDETONE_LEVEL: u8 = 0x47; // 0..=1000
pub const PROP_PROFILE_INDICATOR_COLOR: u8 = 0x49;
pub const PROP_WINLOCK_DISABLED_SHORTCUTS: u8 = 0x4A; // bitmask: Win=1, AltTab=2, AltF4=4, ShiftTab=8
pub const PROP_BATTERY_VOLTAGE: u8 = 0x53;

// Cooler / AIO hub properties
pub const PROP_COOLER_PRODUCT_LINE: u8 = 0x55;
pub const PROP_COOLER_PUMP_VERSION: u8 = 0x57;
pub const PROP_COOLER_RADIATOR_SIZE: u8 = 0x58;

pub const PROP_BRIGHTNESS_INDICATOR_COLOR: u8 = 0x5C;
pub const PROP_WINLOCK_ON_COLOR: u8 = 0x5D;
pub const PROP_WINLOCK_OFF_COLOR: u8 = 0x5E;
pub const PROP_LIGHTING_CTRL_REG: u8 = 0x5F;
pub const PROP_ACTIVE_HW_PROFILE: u8 = 0x6E;
pub const PROP_ESPORTS_SWITCH_POS: u8 = 0x76;
pub const PROP_ESPORTS_BACKLIGHT_COLOR: u8 = 0x77;
pub const PROP_MIC_MUTED: u8 = 0x8E;
pub const PROP_MIC_LEVEL: u8 = 0x8F;
pub const PROP_MAX_POLLRATE: u8 = 0x96;
pub const PROP_ACTIVE_EQ_PRESET: u8 = 0xC4;
pub const PROP_ANC_MODE: u8 = 0xCD;
pub const PROP_RAPID_TRIGGER_ENABLED: u8 = 0xD2;
pub const PROP_GAME_MODE_ENABLED: u8 = 0xDD;
pub const PROP_FLASHTAP_ENABLED: u8 = 0xF5;

// 16-bit extended Bragi properties (> 0x00FF)
pub const PROP_EXT_MOTION_SYNC: u16 = 0x010D;
pub const PROP_EXT_RIPPLE_CONTROL: u16 = 0x010E;
pub const PROP_EXT_ANGLE_SNAPPING: u16 = 0x011F;

// Modes
pub const MODE_HARDWARE: u16 = 0x01;
pub const MODE_SOFTWARE: u16 = 0x02;

// Resource IDs for handles
pub const RES_LIGHTING_PLANAR: u16 = 0x0001;
pub const RES_LIGHTING_TRIPLETS: u16 = 0x0022;
pub const RES_LIGHTING_EXTRA: u16 = 0x002E;

// Handle slots
pub const HANDLE_LIGHTING: u8 = 0x00;

// Protocol error codes returned in response[2]
pub const ERR_OK: u8 = 0x00;
pub const ERR_INVALID_VALUE: u8 = 0x01;
pub const ERR_FAILED_OR_OPEN: u8 = 0x03;
pub const ERR_NOT_SUPPORTED: u8 = 0x05;
pub const ERR_ALT_NOT_SUPPORTED: u8 = 0x06;

#[derive(Debug, Error)]
pub enum BragiError {
    #[error("Aucun périphérique Corsair Bragi détecté (VID 1B1C, UsagePage FF42)")]
    DeviceNotFound,
    #[error("Erreur HID : {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("Timeout de communication avec le périphérique Corsair")]
    Timeout,
    #[error("Le périphérique Corsair a renvoyé une erreur protocole (code 0x{0:02X} : {1})")]
    Protocol(u8, &'static str),
    #[error("Propriété 0x{0:04X} non supportée par ce firmware")]
    UnsupportedProperty(u16),
    #[error("Fréquence {0} Hz supérieure au maximum supporté ({1} Hz)")]
    PollrateExceedsMax(u16, u16),
}

impl BragiError {
    pub fn from_code(code: u8) -> Self {
        let desc = match code {
            ERR_INVALID_VALUE => "Invalid Value",
            ERR_FAILED_OR_OPEN => "Failed / Handle already open",
            ERR_NOT_SUPPORTED => "Not Supported",
            ERR_ALT_NOT_SUPPORTED => "Resource Not Supported",
            _ => "Unknown Protocol Error",
        };
        Self::Protocol(code, desc)
    }
}

/// Format Bragi 24-bit firmware version (matches ckb-next byte order).
pub fn format_fw_version(raw: u32) -> String {
    let major = raw & 0xFF;
    let minor = (raw >> 8) & 0xFF;
    let patch = (raw >> 16) & 0xFF;
    format!("{major}.{minor}.{patch}")
}
