//! Universal SuperIO (Nuvoton `NCT677x`/`NCT679xD`/`NCT6701D`/`NCT5585D`, Nuvoton eSIO EC
//! `NCT6683D`/`NCT6686D`/`NCT6687D`/`NCT6687DR`, and ITE `IT86xx`/`IT87xx`) live Fan RPM & PWM
//! controller via Microsoft WHQL-signed **PawnIO** (`LpcIO.bin`).

use anyhow::{Context, Result};
use opengear_core::driver::{Connection, Device, DeviceInfo, DeviceKind, DeviceSpec, Driver};
use opengear_core::fans::{Cooling, FanInfo, FanTarget, clamp_target};
#[cfg(windows)]
use opengear_platform::PawnIoAvailability;
use opengear_platform::is_elevated;
#[cfg(windows)]
use opengear_platform::named_mutex::{EC_MUTEX, ISA_MUTEX, NamedMutexGuard};
#[cfg(windows)]
use opengear_platform::pawnio::{PawnIoSession, load_pawnio_module, pawnio_availability};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

const MAX_SUPERIO_CHANNELS: usize = 16;

// Nuvoton banked LPC HWM registers (`base + 5` index / `base + 6` data)
const BANK_SELECT_REG: u8 = 0x4E;
const VENDOR_ID_HIGH_REG: u16 = 0x804F;
const VENDOR_ID_LOW_REG: u16 = 0x004F;
const NUVOTON_VENDOR_ID: u16 = 0x5CA3;

const FAN_RPM_REGS_6771: [u16; 3] = [0x656, 0x658, 0x65A];
const FAN_RPM_REGS_95: [u16; 7] = [0x4C0, 0x4C2, 0x4C4, 0x4C6, 0x4C8, 0x4CA, 0x4CE];
const FAN_RPM_REGS_96: [u16; 7] = [0x4B0, 0x4B2, 0x4B4, 0x4B6, 0x4B8, 0x4BA, 0x4CC];
const FAN_PWM_OUT_REGS_98: [u16; 7] = [0x001, 0x003, 0x011, 0x013, 0x015, 0xA09, 0xB09];
const FAN_PWM_OUT_REGS_95: [u16; 7] = [0x001, 0x003, 0x011, 0x013, 0x015, 0x017, 0x029];
const FAN_PWM_CMD_REGS: [u16; 7] = [0x109, 0x209, 0x309, 0x809, 0x909, 0xA09, 0xB09];
const FAN_CTRL_MODE_REGS: [u16; 7] = [0x102, 0x202, 0x302, 0x802, 0x902, 0xA02, 0xB02];

// Nuvoton NCT668x eSIO indirect EC space registers (`base + 4` page / `+ 5` index / `+ 6` data)
const EC_PAGE_REG_OFFSET: u16 = 0x04;
const EC_INDEX_REG_OFFSET: u16 = 0x05;
const EC_DATA_REG_OFFSET: u16 = 0x06;
const EC_PAGE_SELECT_FREE: u8 = 0xFF;
const EC_HWM_CFG_REG: u16 = 0x0180;
const EC_CUSTOMER_ID_REG: u16 = 0x0602;
const EC_FAN_MODE_REG: u16 = 0x0A00;
const EC_FAN_CFG_CTRL_REG: u16 = 0x0A01;
const EC_FAN_CFG_REQ_BIT: u8 = 0x80;
const EC_FAN_CFG_DONE_BIT: u8 = 0x40;
const EC_DR_ENGINE_STS_REG: u16 = 0x0CF8;
const EC_DR_CFG_LOCK: u8 = 1 << 6;
const EC_DR_CFG_CHECK_DONE: u8 = 1 << 5;
const EC_DR_CFG_INVALID: u8 = 1 << 4;
const EC_DR_CFG_PHASE: u8 = 1 << 3;

// ITE IT86xx / IT87xx EC registers
const ITE_VENDOR_ID_REG: u8 = 0x58;
const ITE_VENDOR_IDS: [u8; 2] = [0x90, 0x7F];
const ITE_FAN_MAIN_CTRL_REG: u8 = 0x13;

const ITE_TACH_LSB_REGS: [u8; 6] = [0x0D, 0x0E, 0x0F, 0x80, 0x82, 0x4C];
const ITE_TACH_MSB_REGS: [u8; 6] = [0x18, 0x19, 0x1A, 0x81, 0x83, 0x4D];
const ITE_TACH_LSB_ALT_REGS: [u8; 6] = [0x0D, 0x0E, 0x0F, 0x80, 0x82, 0x93];
const ITE_TACH_MSB_ALT_REGS: [u8; 6] = [0x18, 0x19, 0x1A, 0x81, 0x83, 0x94];
const ITE_PWM_CTRL_REGS: [u8; 6] = [0x15, 0x16, 0x17, 0x7F, 0xA7, 0xAF];
const ITE_PWM_CTRL_ALT_REGS: [u8; 6] = [0x15, 0x16, 0x17, 0x1E, 0x1F, 0x92];
const ITE_PWM_DUTY_REGS: [u8; 6] = [0x63, 0x6B, 0x73, 0x7B, 0xA3, 0xAB];

// Fintek F718xx LPC/HWM registers (`base + 5` index / `base + 6` data)
const FINTEK_VENDOR_ID_REG: u8 = 0x23;
const FINTEK_VENDOR_ID: u16 = 0x1934;
const FINTEK_TEMP_BASE_REG: u8 = 0x70;
const FINTEK_PWM_ENABLE_REG: u8 = 0x96;
const FINTEK_PWM_REGS: [u8; 4] = [0xA3, 0xB3, 0xC3, 0xD3];
const FINTEK_TACH_REGS: [u8; 4] = [0xA0, 0xB0, 0xC0, 0xD0];
const FINTEK_TARGET_REGS: [u8; 4] = [0xA2, 0xB2, 0xC2, 0xD2];

#[derive(Debug, Clone, Copy)]
pub(crate) struct EcChannelRegs {
    pub index: usize,
    pub rpm_reg: u16,
    pub pwm_out_reg: u16,
    pub pwm_cmd_reg: u16,
    pub mode_reg: u16,
    pub mode_bit: u8,
}

const EC_DR_CHANNEL_MAP: [EcChannelRegs; 11] = [
    EcChannelRegs {
        index: 0,
        rpm_reg: 0x0140,
        pwm_out_reg: 0x0160,
        pwm_cmd_reg: 0x0A28,
        mode_reg: 0x0A00,
        mode_bit: 0,
    },
    EcChannelRegs {
        index: 1,
        rpm_reg: 0x0142,
        pwm_out_reg: 0x0161,
        pwm_cmd_reg: 0x0A29,
        mode_reg: 0x0A00,
        mode_bit: 1,
    },
    EcChannelRegs {
        index: 2,
        rpm_reg: 0x0144,
        pwm_out_reg: 0x0162,
        pwm_cmd_reg: 0x0A2A,
        mode_reg: 0x0A00,
        mode_bit: 2,
    },
    EcChannelRegs {
        index: 3,
        rpm_reg: 0x0146,
        pwm_out_reg: 0x0163,
        pwm_cmd_reg: 0x0A2B,
        mode_reg: 0x0A00,
        mode_bit: 3,
    },
    EcChannelRegs {
        index: 9,
        rpm_reg: 0x0152,
        pwm_out_reg: 0x0C93,
        pwm_cmd_reg: 0x08E9,
        mode_reg: 0x080F,
        mode_bit: 1,
    },
    EcChannelRegs {
        index: 10,
        rpm_reg: 0x015E,
        pwm_out_reg: 0x0E05,
        pwm_cmd_reg: 0x0265,
        mode_reg: 0x080F,
        mode_bit: 7,
    },
    EcChannelRegs {
        index: 11,
        rpm_reg: 0x015C,
        pwm_out_reg: 0x0E04,
        pwm_cmd_reg: 0x0264,
        mode_reg: 0x080F,
        mode_bit: 6,
    },
    EcChannelRegs {
        index: 12,
        rpm_reg: 0x015A,
        pwm_out_reg: 0x0E03,
        pwm_cmd_reg: 0x0263,
        mode_reg: 0x080F,
        mode_bit: 5,
    },
    EcChannelRegs {
        index: 13,
        rpm_reg: 0x0158,
        pwm_out_reg: 0x0E02,
        pwm_cmd_reg: 0x0262,
        mode_reg: 0x080F,
        mode_bit: 4,
    },
    EcChannelRegs {
        index: 14,
        rpm_reg: 0x0156,
        pwm_out_reg: 0x0E01,
        pwm_cmd_reg: 0x0261,
        mode_reg: 0x080F,
        mode_bit: 3,
    },
    EcChannelRegs {
        index: 15,
        rpm_reg: 0x0154,
        pwm_out_reg: 0x0E00,
        pwm_cmd_reg: 0x0260,
        mode_reg: 0x080F,
        mode_bit: 2,
    },
];

pub(crate) fn ec_standard_channel(index: usize) -> Option<EcChannelRegs> {
    if index >= 8 {
        return None;
    }
    Some(EcChannelRegs {
        index,
        rpm_reg: 0x0140 + (index as u16) * 2,
        pwm_out_reg: 0x0160 + (index as u16),
        pwm_cmd_reg: 0x0A28 + (index as u16),
        mode_reg: EC_FAN_MODE_REG,
        mode_bit: index as u8,
    })
}

pub(crate) fn ec_dr_channel(index: usize) -> Option<EcChannelRegs> {
    EC_DR_CHANNEL_MAP.iter().copied().find(|c| c.index == index)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperIoFanChannel {
    /// 0-based physical SuperIO channel index (`0..16`)
    pub index: usize,
    pub label: String,
    pub rpm: u32,
    pub pwm_pct: u8,
    /// Raw control mode byte (meaning depends on chip family)
    pub mode_raw: u8,
    pub is_manual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperIoTempSensor {
    pub label: String,
    pub temp_c: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuperIoSnapshot {
    pub detected: bool,
    pub chip_name: String,
    pub chip_id_hex: String,
    pub hwm_base_hex: String,
    #[serde(default)]
    pub motherboard_temp_c: Option<f32>,
    #[serde(default)]
    pub temps: Vec<SuperIoTempSensor>,
    pub fans: Vec<SuperIoFanChannel>,
    pub diagnostic: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NuvotonRpmMode {
    /// Legacy NCT6771F / NCT6776F (`0x656..0x65A` direct 16-bit RPM)
    Rpm656Direct,
    /// NCT6791D..NCT6795D (`0x4C0..0x4CE` direct 16-bit RPM)
    Rpm4C0Direct,
    /// NCT6779D / NCT6796D+ / NCT6701D / NCT5585D (`0x4B0..0x4CC` 13-bit period counter)
    Rpm4B0Count13,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IteChipSpec {
    pub name: &'static str,
    pub fan_count: usize,
    pub pwm_8bit: bool,
    pub alt_regs: bool,
    pub ctrl_0x7f_quirk: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FintekChipSpec {
    pub name: &'static str,
    pub ldn: u8,
    pub tach_count: usize,
    pub pwm_count: usize,
    pub has_pwm_enable: bool,
    pub temp_count: usize,
    pub is_f71858: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipFamily {
    NuvotonBank {
        use_98_pwm_out: bool,
        rpm_mode: NuvotonRpmMode,
        allow_no_vendor_id: bool,
    },
    NuvotonEc {
        dr_handshake: bool,
    },
    Ite(IteChipSpec),
    Fintek(FintekChipSpec),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DetectedChip {
    slot: i64,
    hwm_base: u16,
    chip_id: u16,
    chip_name: &'static str,
    fan_count: usize,
    family: ChipFamily,
}

/// Minimum manual PWM duty OpenGear will ever write to a generic SuperIO header.
pub const SUPERIO_MIN_PWM_PCT: u8 = 30;

/// Clamp a requested manual duty into `floor_pct..=100` (never below `SUPERIO_MIN_PWM_PCT` unless explicit higher floor).
pub fn safe_superio_duty_with_floor(duty_pct: u8, floor_pct: u8) -> u8 {
    let floor = floor_pct.clamp(SUPERIO_MIN_PWM_PCT, 100);
    duty_pct.clamp(floor, 100)
}

/// Clamp a requested manual duty into the safe range `SUPERIO_MIN_PWM_PCT..=100`.
pub fn safe_superio_duty(duty_pct: u8) -> u8 {
    safe_superio_duty_with_floor(duty_pct, SUPERIO_MIN_PWM_PCT)
}

/// Preserve lower bits `3:0` when switching Nuvoton banked `FAN_CTRL_MODE_REGS` to manual mode (`bits 7:4 == 0`).
pub(crate) fn manual_mode_byte(cur_mode: u8) -> u8 {
    cur_mode & 0x0F
}

#[derive(Debug, Clone, Copy)]
struct SavedFanState {
    mode: u8,
    cmd: u8,
    extra: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperIoSavedState {
    pub index: usize,
    pub mode: u8,
    pub cmd: u8,
    /// Optional auxiliary register state (e.g. ITE `FAN_MAIN_CTRL` 0x13 bit).
    #[serde(default)]
    pub extra: u8,
}

pub fn saved_superio_states() -> Vec<SuperIoSavedState> {
    saved_states()
        .lock()
        .map(|g| {
            (0..MAX_SUPERIO_CHANNELS)
                .filter_map(|i| {
                    g[i].map(|s| SuperIoSavedState {
                        index: i,
                        mode: s.mode,
                        cmd: s.cmd,
                        extra: s.extra,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

static SAVED_INITIAL_STATES: OnceLock<Mutex<[Option<SavedFanState>; MAX_SUPERIO_CHANNELS]>> =
    OnceLock::new();

fn saved_states() -> &'static Mutex<[Option<SavedFanState>; MAX_SUPERIO_CHANNELS]> {
    SAVED_INITIAL_STATES.get_or_init(|| Mutex::new([None; MAX_SUPERIO_CHANNELS]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NuvotonBankSpec {
    pub name: &'static str,
    pub fan_count: usize,
    pub use_98_pwm_out: bool,
    pub rpm_mode: NuvotonRpmMode,
    pub allow_no_vendor_id: bool,
}

pub(crate) fn identify_nuvoton_bank(id: u16) -> Option<NuvotonBankSpec> {
    match id & 0xFFF0 {
        0xB470 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6771F",
            fan_count: 3,
            use_98_pwm_out: false,
            rpm_mode: NuvotonRpmMode::Rpm656Direct,
            allow_no_vendor_id: false,
        }),
        0xC330 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6776F",
            fan_count: 3,
            use_98_pwm_out: false,
            rpm_mode: NuvotonRpmMode::Rpm656Direct,
            allow_no_vendor_id: false,
        }),
        0xC560 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6779D",
            fan_count: 5,
            use_98_pwm_out: false,
            rpm_mode: NuvotonRpmMode::Rpm4B0Count13,
            allow_no_vendor_id: false,
        }),
        0xC800 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6791D",
            fan_count: 6,
            use_98_pwm_out: false,
            rpm_mode: NuvotonRpmMode::Rpm4C0Direct,
            allow_no_vendor_id: false,
        }),
        0xC910 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6792D",
            fan_count: 6,
            use_98_pwm_out: false,
            rpm_mode: NuvotonRpmMode::Rpm4C0Direct,
            allow_no_vendor_id: false,
        }),
        0xD120 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6793D/NCT6795D",
            fan_count: 6,
            use_98_pwm_out: false,
            rpm_mode: NuvotonRpmMode::Rpm4C0Direct,
            allow_no_vendor_id: false,
        }),
        0xD350 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6796D",
            fan_count: 6,
            use_98_pwm_out: false,
            rpm_mode: NuvotonRpmMode::Rpm4B0Count13,
            allow_no_vendor_id: false,
        }),
        0xD420 => {
            let (name, fan_count) = match id {
                0xD423 => ("Nuvoton NCT6796D", 6),
                0xD428 | 0xD42B => ("Nuvoton NCT6798D", 7),
                0xD42A => ("Nuvoton NCT6796D-R/NCT5585D", 7),
                _ => ("Nuvoton NCT6796D-R/S", 7),
            };
            Some(NuvotonBankSpec {
                name,
                fan_count,
                use_98_pwm_out: true,
                rpm_mode: NuvotonRpmMode::Rpm4B0Count13,
                allow_no_vendor_id: false,
            })
        }
        0xD450 => Some(NuvotonBankSpec {
            name: "Nuvoton NCT6797D",
            fan_count: 7,
            use_98_pwm_out: true,
            rpm_mode: NuvotonRpmMode::Rpm4B0Count13,
            allow_no_vendor_id: false,
        }),
        0xD800 => {
            let (name, allow_no_vendor_id) = match id {
                0xD806 => ("Nuvoton NCT6701D", true),
                0xD802 => ("Nuvoton NCT6799D/NCT6796D-S", false),
                _ => ("Nuvoton NCT6799D", false),
            };
            Some(NuvotonBankSpec {
                name,
                fan_count: 7,
                use_98_pwm_out: true,
                rpm_mode: NuvotonRpmMode::Rpm4B0Count13,
                allow_no_vendor_id,
            })
        }
        _ => None,
    }
}

/// Return `(name, fan_count, use_98_pwm_out, use_96_rpm_regs)` for banked Nuvoton chips.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn identify_nuvoton(id: u16) -> Option<(&'static str, usize, bool, bool)> {
    identify_nuvoton_bank(id).map(|s| {
        (
            s.name,
            s.fan_count,
            s.use_98_pwm_out,
            s.rpm_mode == NuvotonRpmMode::Rpm4B0Count13,
        )
    })
}

/// Return the base EC-space model name for Nuvoton `NCT6683D` / `NCT6686D` / `NCT6687D`.
pub(crate) fn identify_nuvoton_ec(id: u16) -> Option<&'static str> {
    match id & 0xFFF0 {
        0xC730 => Some("Nuvoton NCT6683D"),
        0xD440 => Some("Nuvoton NCT6686D"),
        0xD590 => Some("Nuvoton NCT6687D"),
        _ => None,
    }
}

/// True when the SMBIOS baseboard product string names an 800-series chipset known to pair
/// `NCT6687D` (`0xD592`) with the `0xCF8` handshake EC firmware (`NCT6687D-R`).
pub(crate) fn is_800_series_ec_dr_board(chip_id: u16, board_product: &str) -> bool {
    if chip_id != 0xD592 {
        return false;
    }
    let up = board_product.to_ascii_uppercase();
    ["B840", "B850", "B860", "X870", "Z890"]
        .iter()
        .any(|tag| up.contains(tag))
}

/// Return full chip capabilities for supported ITE EC/SuperIO chips.
pub(crate) fn identify_ite_spec(id: u16) -> Option<IteChipSpec> {
    let spec = |name, fan_count, pwm_8bit, alt_regs, ctrl_0x7f_quirk| IteChipSpec {
        name,
        fan_count,
        pwm_8bit,
        alt_regs,
        ctrl_0x7f_quirk,
    };
    match id {
        0x8613 => Some(spec("ITE IT8613E", 4, true, false, false)),
        0x8620 => Some(spec("ITE IT8620E", 5, true, false, false)),
        0x8622 => Some(spec("ITE IT8622E", 5, true, false, false)),
        0x8623 => Some(spec("ITE IT8623E", 3, true, false, false)),
        0x8625 => Some(spec("ITE IT8625E", 6, true, true, false)),
        0x8628 => Some(spec("ITE IT8628E", 6, true, false, false)),
        0x8631 => Some(spec("ITE IT8631E", 2, true, false, false)),
        0x8638 => Some(spec("ITE IT8638E", 2, true, false, false)),
        0x8655 => Some(spec("ITE IT8655E", 3, true, false, false)),
        0x8665 => Some(spec("ITE IT8665E", 6, true, true, false)),
        0x8686 => Some(spec("ITE IT8686E", 5, true, false, false)),
        0x8688 => Some(spec("ITE IT8688E", 5, true, false, false)),
        0x8689 => Some(spec("ITE IT8689E", 6, true, false, true)),
        0x8695 => Some(spec("ITE IT87952E", 3, true, false, false)),
        0x8696 => Some(spec("ITE IT8696E", 6, true, false, false)),
        0x8705 => Some(spec("ITE IT8705F", 3, false, false, false)),
        0x8712 => Some(spec("ITE IT8712F", 3, false, false, false)),
        0x8716 => Some(spec("ITE IT8716F", 3, false, false, false)),
        0x8718 => Some(spec("ITE IT8718F", 3, false, false, false)),
        0x8720 => Some(spec("ITE IT8720F", 3, false, false, false)),
        0x8721 => Some(spec("ITE IT8721F", 3, true, false, false)),
        0x8726 => Some(spec("ITE IT8726F", 3, false, false, false)),
        0x8728 => Some(spec("ITE IT8728F", 5, false, false, false)),
        0x8733 => Some(spec("ITE IT8792E/IT8795E", 3, true, false, false)),
        0x8771 => Some(spec("ITE IT8771E", 3, false, false, false)),
        0x8772 => Some(spec("ITE IT8772E", 3, true, false, false)),
        0x8790 => Some(spec("ITE IT8790E", 3, true, false, false)),
        0x8792 | 0x8795 => Some(spec("ITE IT8792E/IT8795E", 3, true, false, false)),
        _ => None,
    }
}

/// Return `(name, fan_count, pwm_8bit)` for supported ITE EC/SuperIO chips.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn identify_ite(id: u16) -> Option<(&'static str, usize, bool)> {
    identify_ite_spec(id).map(|s| (s.name, s.fan_count, s.pwm_8bit))
}

pub(crate) fn ite_rpm_from_count(count: u32) -> u32 {
    if count == 0 || count >= 0xFFFF {
        return 0;
    }
    let rpm = 1_350_000u32 / (count * 2);
    if (100..15_000).contains(&rpm) { rpm } else { 0 }
}

/// Decode Nuvoton fan RPM from `(hi, lo)` bytes.
///
/// NCT6779D / NCT6796D+ (`0x4B0..0x4CC`) expose a 13-bit tachometer period counter
/// (`(hi << 5) | (lo & 0x1F)`) clocked at 1.35 MHz, whereas NCT6771F/NCT6776F (`0x656..0x65A`),
/// NCT6791D..NCT6795D (`0x4C0..0x4CE`) and NCT668x (`0x140..0x15E`) report direct 16-bit RPM.
pub(crate) fn nuvoton_rpm_from_regs(hi: u8, lo: u8, is_13bit_count: bool) -> u32 {
    if is_13bit_count {
        let count = ((hi as u32) << 5) | ((lo as u32) & 0x1F);
        if count == 0 || count >= 0x1FFF {
            return 0;
        }
        let rpm = 1_350_000u32 / count;
        if (100..15_000).contains(&rpm) { rpm } else { 0 }
    } else {
        let raw_rpm = ((hi as u32) << 8) | (lo as u32);
        if (100..15_000).contains(&raw_rpm) {
            raw_rpm
        } else {
            0
        }
    }
}

pub(crate) fn ite_manual_target(
    cur_ctrl: u8,
    duty_pct: u8,
    pwm_8bit: bool,
    ctrl_0x7f_quirk: bool,
) -> (u8, u8) {
    if pwm_8bit {
        let raw = ((duty_pct.min(100) as u32 * 255 + 50) / 100).min(255) as u8;
        // IT8689E requires writing `0x7F` (detaching all 7 temp-map bits) before duty takes effect;
        // other ITE 8-bit chips preserve the lower 7 bits while clearing bit 7 (manual mode).
        let ctrl = if ctrl_0x7f_quirk {
            0x7F
        } else {
            cur_ctrl & 0x7F
        };
        (ctrl, raw)
    } else {
        let raw = ((duty_pct.min(100) as u32 * 127 + 50) / 100).min(127) as u8;
        (raw & 0x7F, raw)
    }
}

pub(crate) fn identify_fintek(id: u16) -> Option<FintekChipSpec> {
    let f =
        |name, ldn, tach_count, pwm_count, has_pwm_enable, temp_count, is_f71858| FintekChipSpec {
            name,
            ldn,
            tach_count,
            pwm_count,
            has_pwm_enable,
            temp_count,
            is_f71858,
        };
    match id {
        0x0507 => Some(f("Fintek F71858", 0x02, 4, 0, false, 3, true)),
        0x0541 => Some(f("Fintek F71882", 0x04, 4, 4, false, 3, false)),
        0x0601 => Some(f("Fintek F71862", 0x04, 3, 0, false, 3, false)),
        0x0723 => Some(f("Fintek F71889F", 0x04, 3, 0, false, 3, false)),
        0x0814 => Some(f("Fintek F71869", 0x04, 3, 0, false, 3, false)),
        0x0901 => Some(f("Fintek F71808E", 0x04, 3, 0, false, 2, false)),
        0x0909 => Some(f("Fintek F71889ED", 0x04, 3, 3, true, 3, false)),
        0x1005 => Some(f("Fintek F71889AD", 0x04, 3, 3, true, 3, false)),
        0x1007 => Some(f("Fintek F71869A", 0x04, 3, 0, false, 3, false)),
        0x1106 => Some(f("Fintek F71878AD", 0x04, 3, 3, true, 3, false)),
        0x1118 => Some(f("Fintek F71811", 0x04, 3, 0, false, 3, false)),
        _ => None,
    }
}

pub(crate) fn fintek_rpm_from_regs(hi: u8, lo: u8) -> u32 {
    let raw = ((hi as u32) << 8) | (lo as u32);
    if (1..0x0FFF).contains(&raw) {
        let rpm = 1_500_000u32 / raw;
        if (100..15_000).contains(&rpm) { rpm } else { 0 }
    } else {
        0
    }
}

/// Fintek `FAN_PWM_ENABLE_REG (0x96)` uses 2 bits per fan: bit 2n = duty mode, bit 2n+1 = manual.
pub(crate) fn fintek_pwm_enable_mask(ch: usize) -> u8 {
    0x03u8 << (2 * ch.min(3))
}

pub(crate) fn sanitize_superio_temp(t: f32) -> Option<f32> {
    if t.is_finite() && (5.0..=115.0).contains(&t) {
        Some((t * 10.0).round() / 10.0)
    } else {
        None
    }
}

pub(crate) fn decode_nuvoton_temp_half(
    int_byte: u8,
    half_byte: Option<u8>,
    half_bit: u8,
) -> Option<f32> {
    if matches!(int_byte, 0x00 | 0x7F | 0x80) {
        return None;
    }
    let whole = (int_byte as i8) as f32;
    let frac = half_byte
        .map(|b| {
            if ((b >> half_bit) & 0x01) != 0 {
                0.5
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);
    sanitize_superio_temp(whole + frac)
}

pub(crate) fn decode_signed_byte_temp(raw: u8) -> Option<f32> {
    if matches!(raw, 0x00 | 0x7F | 0x80) {
        return None;
    }
    sanitize_superio_temp((raw as i8) as f32)
}

#[cfg(windows)]
mod win_sio {
    use super::*;
    use std::time::{Duration, Instant};

    fn pio_outb(s: &PawnIoSession, port: u16, val: u8) -> Result<()> {
        s.execute("ioctl_pio_outb", &[port as i64, val as i64], 0)?;
        Ok(())
    }

    fn pio_inb(s: &PawnIoSession, port: u16) -> Result<u8> {
        let out = s.execute("ioctl_pio_inb", &[port as i64], 1)?;
        Ok((out.first().copied().unwrap_or(0) & 0xFF) as u8)
    }

    fn sio_inb(s: &PawnIoSession, reg: u8) -> Result<u8> {
        let out = s.execute("ioctl_superio_inb", &[reg as i64], 1)?;
        Ok((out.first().copied().unwrap_or(0) & 0xFF) as u8)
    }

    fn sio_inw(s: &PawnIoSession, reg: u8) -> Result<u16> {
        let out = s.execute("ioctl_superio_inw", &[reg as i64], 1)?;
        Ok((out.first().copied().unwrap_or(0) & 0xFFFF) as u16)
    }

    fn sio_outb(s: &PawnIoSession, reg: u8, val: u8) -> Result<()> {
        s.execute("ioctl_superio_outb", &[reg as i64, val as i64], 0)?;
        Ok(())
    }

    fn enter_nuvoton(s: &PawnIoSession, reg_port: u16) -> Result<()> {
        pio_outb(s, reg_port, 0x87)?;
        pio_outb(s, reg_port, 0x87)?;
        Ok(())
    }

    fn exit_nuvoton(s: &PawnIoSession, reg_port: u16) {
        let _ = pio_outb(s, reg_port, 0xAA);
    }

    fn hwm_read(s: &PawnIoSession, base: u16, addr: u16) -> Result<u8> {
        let bank = (addr >> 8) as u8;
        let reg = (addr & 0xFF) as u8;
        pio_outb(s, base + 5, BANK_SELECT_REG)?;
        pio_outb(s, base + 6, bank)?;
        pio_outb(s, base + 5, reg)?;
        pio_inb(s, base + 6)
    }

    fn hwm_write(s: &PawnIoSession, base: u16, addr: u16, val: u8) -> Result<()> {
        let bank = (addr >> 8) as u8;
        let reg = (addr & 0xFF) as u8;
        pio_outb(s, base + 5, BANK_SELECT_REG)?;
        pio_outb(s, base + 6, bank)?;
        pio_outb(s, base + 5, reg)?;
        pio_outb(s, base + 6, val)
    }

    fn verify_nuvoton_vendor(s: &PawnIoSession, base: u16) -> bool {
        let hi = hwm_read(s, base, VENDOR_ID_HIGH_REG).unwrap_or(0);
        let lo = hwm_read(s, base, VENDOR_ID_LOW_REG).unwrap_or(0);
        (((hi as u16) << 8) | (lo as u16)) == NUVOTON_VENDOR_ID
    }

    fn ec_acquire_page_port(s: &PawnIoSession, base: u16) -> Result<()> {
        let page_port = base + EC_PAGE_REG_OFFSET;
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            if pio_inb(s, page_port)? == EC_PAGE_SELECT_FREE {
                return Ok(());
            }
            if Instant::now() >= deadline {
                pio_outb(s, page_port, EC_PAGE_SELECT_FREE)?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn ec_read(s: &PawnIoSession, base: u16, addr: u16) -> Result<u8> {
        let page = (addr >> 8) as u8;
        let index = (addr & 0xFF) as u8;
        ec_acquire_page_port(s, base)?;
        pio_outb(s, base + EC_PAGE_REG_OFFSET, page)?;
        pio_outb(s, base + EC_INDEX_REG_OFFSET, index)?;
        let val = pio_inb(s, base + EC_DATA_REG_OFFSET);
        let _ = pio_outb(s, base + EC_PAGE_REG_OFFSET, EC_PAGE_SELECT_FREE);
        val
    }

    fn ec_write(s: &PawnIoSession, base: u16, addr: u16, val: u8) -> Result<()> {
        let page = (addr >> 8) as u8;
        let index = (addr & 0xFF) as u8;
        ec_acquire_page_port(s, base)?;
        pio_outb(s, base + EC_PAGE_REG_OFFSET, page)?;
        pio_outb(s, base + EC_INDEX_REG_OFFSET, index)?;
        let res = pio_outb(s, base + EC_DATA_REG_OFFSET, val);
        let _ = pio_outb(s, base + EC_PAGE_REG_OFFSET, EC_PAGE_SELECT_FREE);
        res
    }

    fn ec_update(s: &PawnIoSession, base: u16, addr: u16, and_mask: u8, or_mask: u8) -> Result<()> {
        let cur = ec_read(s, base, addr)?;
        ec_write(s, base, addr, (cur & and_mask) | or_mask)
    }

    fn ec_verify_alive(s: &PawnIoSession, base: u16) -> bool {
        let cfg = ec_read(s, base, EC_HWM_CFG_REG).unwrap_or(0xFF);
        let cid_hi = ec_read(s, base, EC_CUSTOMER_ID_REG).unwrap_or(0xFF);
        let cid_lo = ec_read(s, base, EC_CUSTOMER_ID_REG + 1).unwrap_or(0xFF);
        !(cfg == 0xFF && cid_hi == 0xFF && cid_lo == 0xFF)
    }

    fn ec_dr_start_cfg(s: &PawnIoSession, base: u16) -> Result<bool> {
        let sts = ec_read(s, base, EC_DR_ENGINE_STS_REG)?;
        if (sts & EC_DR_CFG_LOCK) == 0 && (sts & EC_DR_CFG_PHASE) != 0 {
            return Ok(true);
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            let sts = ec_read(s, base, EC_DR_ENGINE_STS_REG)?;
            if (sts & EC_DR_CFG_PHASE) == 0 {
                let req = ec_read(s, base, EC_FAN_CFG_CTRL_REG)?;
                if (req & EC_FAN_CFG_REQ_BIT) == 0 {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        ec_update(
            s,
            base,
            EC_FAN_CFG_CTRL_REG,
            !EC_FAN_CFG_REQ_BIT,
            EC_FAN_CFG_REQ_BIT,
        )?;
        std::thread::sleep(Duration::from_millis(10));
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            let sts = ec_read(s, base, EC_DR_ENGINE_STS_REG)?;
            if (sts & EC_DR_CFG_LOCK) == 0 && (sts & EC_DR_CFG_PHASE) != 0 {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(false)
    }

    fn ec_dr_complete_cfg(s: &PawnIoSession, base: u16) -> Result<bool> {
        let sts = ec_read(s, base, EC_DR_ENGINE_STS_REG)?;
        if (sts & EC_DR_CFG_LOCK) != 0 || (sts & EC_DR_CFG_PHASE) == 0 {
            return Ok(false);
        }
        ec_update(
            s,
            base,
            EC_FAN_CFG_CTRL_REG,
            !EC_FAN_CFG_DONE_BIT,
            EC_FAN_CFG_DONE_BIT,
        )?;
        std::thread::sleep(Duration::from_millis(10));
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            let sts = ec_read(s, base, EC_DR_ENGINE_STS_REG)?;
            if (sts & EC_DR_CFG_CHECK_DONE) != 0 {
                return Ok((sts & EC_DR_CFG_INVALID) == 0);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let sts = ec_read(s, base, EC_DR_ENGINE_STS_REG)?;
        Ok((sts & EC_DR_CFG_INVALID) == 0)
    }

    fn ec_channel_for(dr_handshake: bool, index: usize) -> Option<EcChannelRegs> {
        if dr_handshake {
            ec_dr_channel(index)
        } else {
            ec_standard_channel(index)
        }
    }

    fn ec_channels(dr_handshake: bool) -> Vec<EcChannelRegs> {
        if dr_handshake {
            EC_DR_CHANNEL_MAP.to_vec()
        } else {
            (0..8).filter_map(ec_standard_channel).collect()
        }
    }

    fn query_baseboard_product() -> String {
        opengear_platform::wmi::WmiConn::connect(r"root\cimv2")
            .ok()
            .and_then(|c| c.query("SELECT Product FROM Win32_BaseBoard").ok())
            .and_then(|rows| {
                rows.into_iter().next().and_then(|r| {
                    r.get("Product")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                })
            })
            .unwrap_or_default()
    }

    fn enter_ite(s: &PawnIoSession, reg_port: u16) -> Result<()> {
        let last = if reg_port == 0x4E { 0xAA } else { 0x55 };
        pio_outb(s, reg_port, 0x87)?;
        pio_outb(s, reg_port, 0x01)?;
        pio_outb(s, reg_port, 0x55)?;
        pio_outb(s, reg_port, last)?;
        Ok(())
    }

    fn exit_ite(s: &PawnIoSession) {
        let _ = sio_outb(s, 0x02, 0x02);
    }

    fn ite_ec_read(s: &PawnIoSession, base: u16, reg: u8) -> Result<u8> {
        pio_outb(s, base + 5, reg)?;
        pio_inb(s, base + 6)
    }

    fn ite_ec_write(s: &PawnIoSession, base: u16, reg: u8, val: u8) -> Result<()> {
        pio_outb(s, base + 5, reg)?;
        pio_outb(s, base + 6, val)
    }

    fn verify_ite_vendor(s: &PawnIoSession, base: u16) -> bool {
        let v = ite_ec_read(s, base, ITE_VENDOR_ID_REG).unwrap_or(0);
        ITE_VENDOR_IDS.contains(&v)
    }

    fn verify_chip_vendor(s: &PawnIoSession, chip: DetectedChip) -> bool {
        match chip.family {
            ChipFamily::NuvotonBank {
                allow_no_vendor_id, ..
            } => allow_no_vendor_id || verify_nuvoton_vendor(s, chip.hwm_base),
            ChipFamily::NuvotonEc { .. } => ec_verify_alive(s, chip.hwm_base),
            ChipFamily::Ite(_) => verify_ite_vendor(s, chip.hwm_base),
            ChipFamily::Fintek(_) => ite_ec_read(s, chip.hwm_base, 0x00).unwrap_or(0xFF) != 0xFF,
        }
    }

    fn detect_and_prepare() -> Result<(PawnIoSession, DetectedChip)> {
        let mod_bytes = load_pawnio_module("LpcIO.bin")?;
        let sess = PawnIoSession::load(&mod_bytes)?;
        for &(slot, reg_port) in &[(0i64, 0x2Eu16), (1i64, 0x4Eu16)] {
            if sess.execute("ioctl_select_slot", &[slot], 0).is_err() {
                continue;
            }
            if enter_nuvoton(&sess, reg_port).is_ok() {
                let chip_id = sio_inw(&sess, 0x20).unwrap_or(0xFFFF);
                if let Some(base_name) = identify_nuvoton_ec(chip_id) {
                    let _ = sess.execute("ioctl_find_bars", &[], 0);
                    let _ = sio_outb(&sess, 0x07, 0x0B);
                    let hwm_base = sio_inw(&sess, 0x60).unwrap_or(0);
                    exit_nuvoton(&sess, reg_port);

                    if hwm_base >= 0x100 && hwm_base != 0xFFFF && ec_verify_alive(&sess, hwm_base) {
                        let board = query_baseboard_product();
                        let dr_handshake = is_800_series_ec_dr_board(chip_id, &board);
                        let (chip_name, fan_count) = if dr_handshake {
                            ("Nuvoton NCT6687D-R", EC_DR_CHANNEL_MAP.len())
                        } else {
                            (base_name, 8)
                        };
                        return Ok((
                            sess,
                            DetectedChip {
                                slot,
                                hwm_base,
                                chip_id,
                                chip_name,
                                fan_count,
                                family: ChipFamily::NuvotonEc { dr_handshake },
                            },
                        ));
                    }
                } else if let Some(spec) = identify_nuvoton_bank(chip_id) {
                    let _ = sess.execute("ioctl_find_bars", &[], 0);
                    let _ = sio_outb(&sess, 0x07, 0x0B);
                    let hwm_base = sio_inw(&sess, 0x60).unwrap_or(0);
                    exit_nuvoton(&sess, reg_port);

                    if hwm_base >= 0x100 && hwm_base != 0xFFFF {
                        if !verify_nuvoton_vendor(&sess, hwm_base)
                            && let Some(_ec_lock) = NamedMutexGuard::acquire(EC_MUTEX, 500)
                            && enter_nuvoton(&sess, reg_port).is_ok()
                        {
                            let _ = sio_outb(&sess, 0x07, 0x0B);
                            if let Ok(opt) = sio_inb(&sess, 0x28)
                                && (opt & 0x10) != 0
                            {
                                let _ = sio_outb(&sess, 0x28, opt & !0x10);
                            }
                            exit_nuvoton(&sess, reg_port);
                        }

                        if spec.allow_no_vendor_id || verify_nuvoton_vendor(&sess, hwm_base) {
                            return Ok((
                                sess,
                                DetectedChip {
                                    slot,
                                    hwm_base,
                                    chip_id,
                                    chip_name: spec.name,
                                    fan_count: spec.fan_count,
                                    family: ChipFamily::NuvotonBank {
                                        use_98_pwm_out: spec.use_98_pwm_out,
                                        rpm_mode: spec.rpm_mode,
                                        allow_no_vendor_id: spec.allow_no_vendor_id,
                                    },
                                },
                            ));
                        }
                    }
                } else if let Some(spec) = identify_fintek(chip_id)
                    && sio_inw(&sess, FINTEK_VENDOR_ID_REG).unwrap_or(0) == FINTEK_VENDOR_ID
                {
                    let _ = sess.execute("ioctl_find_bars", &[], 0);
                    let _ = sio_outb(&sess, 0x07, spec.ldn);
                    let mut hwm_base = sio_inw(&sess, 0x60).unwrap_or(0);
                    exit_nuvoton(&sess, reg_port);
                    if (hwm_base & 0x07) == 0x05 {
                        hwm_base &= 0xFFF8;
                    }
                    if hwm_base >= 0x100
                        && hwm_base != 0xFFFF
                        && ite_ec_read(&sess, hwm_base, 0x00).unwrap_or(0xFF) != 0xFF
                    {
                        return Ok((
                            sess,
                            DetectedChip {
                                slot,
                                hwm_base,
                                chip_id,
                                chip_name: spec.name,
                                fan_count: spec.tach_count,
                                family: ChipFamily::Fintek(spec),
                            },
                        ));
                    }
                } else {
                    exit_nuvoton(&sess, reg_port);
                }
            }

            if enter_ite(&sess, reg_port).is_ok() {
                let chip_id = sio_inw(&sess, 0x20).unwrap_or(0xFFFF);
                if let Some(spec) = identify_ite_spec(chip_id) {
                    let _ = sess.execute("ioctl_find_bars", &[], 0);
                    let _ = sio_outb(&sess, 0x07, 0x04);
                    let hwm_base = sio_inw(&sess, 0x60).unwrap_or(0);
                    exit_ite(&sess);
                    if hwm_base >= 0x100 && hwm_base != 0xFFFF && verify_ite_vendor(&sess, hwm_base)
                    {
                        return Ok((
                            sess,
                            DetectedChip {
                                slot,
                                hwm_base,
                                chip_id,
                                chip_name: spec.name,
                                fan_count: spec.fan_count,
                                family: ChipFamily::Ite(spec),
                            },
                        ));
                    }
                } else {
                    exit_ite(&sess);
                }
            }
        }
        anyhow::bail!(
            "Aucune puce SuperIO Nuvoton NCT668x/NCT677x-NCT679xD ou ITE IT86xx-IT87xx détectée sur 0x2E/0x4E"
        )
    }

    static SNAPSHOT_SESSION: OnceLock<Mutex<Option<(PawnIoSession, DetectedChip)>>> =
        OnceLock::new();

    pub fn read_snapshot() -> SuperIoSnapshot {
        if !is_elevated() {
            return SuperIoSnapshot {
                diagnostic: "Admin UAC requis pour lire le SuperIO via PawnIO LpcIO.".into(),
                ..Default::default()
            };
        }
        if pawnio_availability() != PawnIoAvailability::Ready {
            return SuperIoSnapshot {
                diagnostic: "Pilote signé PawnIO non disponible.".into(),
                ..Default::default()
            };
        }

        let Some(_lock) = NamedMutexGuard::acquire(ISA_MUTEX, 1000) else {
            return SuperIoSnapshot {
                diagnostic: "Bus SuperIO ISA occupé par un autre outil — lecture annulée.".into(),
                ..Default::default()
            };
        };

        let cache = SNAPSHOT_SESSION.get_or_init(|| Mutex::new(None));
        let Ok(mut guard) = cache.lock() else {
            return SuperIoSnapshot {
                diagnostic: "Verrou interne SuperIO indisponible.".into(),
                ..Default::default()
            };
        };

        if guard.is_none() {
            match detect_and_prepare() {
                Ok(v) => *guard = Some(v),
                Err(e) => {
                    return SuperIoSnapshot {
                        diagnostic: e.to_string(),
                        ..Default::default()
                    };
                }
            }
        }

        let Some((ref sess, chip)) = *guard else {
            return SuperIoSnapshot::default();
        };

        if !verify_chip_vendor(sess, chip) {
            *guard = None;
            return SuperIoSnapshot {
                diagnostic: "Réponse SuperIO invalide — session réinitialisée.".into(),
                ..Default::default()
            };
        }

        let mut fans = Vec::new();
        let mut temps = Vec::new();
        let mut motherboard_temp_c: Option<f32> = None;
        match chip.family {
            ChipFamily::NuvotonBank {
                use_98_pwm_out,
                rpm_mode,
                ..
            } => {
                // Read SYSTIN & auxiliary board / T_Sensor temperatures natively.
                let systin = match rpm_mode {
                    NuvotonRpmMode::Rpm656Direct => {
                        hwm_read(sess, chip.hwm_base, 0x077).ok().and_then(|hi| {
                            decode_nuvoton_temp_half(
                                hi,
                                hwm_read(sess, chip.hwm_base, 0x078).ok(),
                                7,
                            )
                        })
                    }
                    NuvotonRpmMode::Rpm4C0Direct | NuvotonRpmMode::Rpm4B0Count13 => {
                        let direct = hwm_read(sess, chip.hwm_base, 0x490)
                            .ok()
                            .and_then(decode_signed_byte_temp);
                        direct.or_else(|| {
                            let src = hwm_read(sess, chip.hwm_base, 0x300).unwrap_or(0) & 0x1F;
                            if src == 1 {
                                hwm_read(sess, chip.hwm_base, 0x077).ok().and_then(|hi| {
                                    decode_nuvoton_temp_half(
                                        hi,
                                        hwm_read(sess, chip.hwm_base, 0x078).ok(),
                                        7,
                                    )
                                })
                            } else {
                                None
                            }
                        })
                    }
                };
                if let Some(t) = systin {
                    motherboard_temp_c = Some(t);
                    temps.push(SuperIoTempSensor {
                        label: "Carte mère (SYSTIN)".into(),
                        temp_c: t,
                    });
                }
                if use_98_pwm_out && let Ok(ts_hi) = hwm_read(sess, chip.hwm_base, 0x4A2) {
                    let ts_t = decode_nuvoton_temp_half(
                        ts_hi,
                        hwm_read(sess, chip.hwm_base, 0x4A1).ok(),
                        7,
                    )
                    .or_else(|| {
                        hwm_read(sess, chip.hwm_base, 0x496)
                            .ok()
                            .and_then(decode_signed_byte_temp)
                    });
                    if let Some(t) = ts_t {
                        temps.push(SuperIoTempSensor {
                            label: "Sonde T_Sensor".into(),
                            temp_c: t,
                        });
                    }
                }
                for aux_idx in 0..2usize {
                    let reg = match rpm_mode {
                        NuvotonRpmMode::Rpm656Direct => 0x075,
                        _ => 0x492 + aux_idx as u16,
                    };
                    if let Some(t) = hwm_read(sess, chip.hwm_base, reg)
                        .ok()
                        .and_then(decode_signed_byte_temp)
                    {
                        temps.push(SuperIoTempSensor {
                            label: format!("AUXTIN{aux_idx}"),
                            temp_c: t,
                        });
                        if motherboard_temp_c.is_none() {
                            motherboard_temp_c = Some(t);
                        }
                    }
                    if rpm_mode == NuvotonRpmMode::Rpm656Direct {
                        break;
                    }
                }
                let rpm_regs: &[u16] = match rpm_mode {
                    NuvotonRpmMode::Rpm656Direct => &FAN_RPM_REGS_6771,
                    NuvotonRpmMode::Rpm4C0Direct => &FAN_RPM_REGS_95,
                    NuvotonRpmMode::Rpm4B0Count13 => &FAN_RPM_REGS_96,
                };
                let pwm_out_regs = if use_98_pwm_out {
                    &FAN_PWM_OUT_REGS_98
                } else {
                    &FAN_PWM_OUT_REGS_95
                };
                let is_13bit = rpm_mode == NuvotonRpmMode::Rpm4B0Count13;

                for i in 0..chip.fan_count {
                    let Ok(hi) = hwm_read(sess, chip.hwm_base, rpm_regs[i]) else {
                        *guard = None;
                        return SuperIoSnapshot {
                            diagnostic: "Erreur lecture RPM SuperIO.".into(),
                            ..Default::default()
                        };
                    };
                    let Ok(lo) = hwm_read(sess, chip.hwm_base, rpm_regs[i] + 1) else {
                        *guard = None;
                        return SuperIoSnapshot {
                            diagnostic: "Erreur lecture RPM SuperIO.".into(),
                            ..Default::default()
                        };
                    };
                    let rpm = nuvoton_rpm_from_regs(hi, lo, is_13bit);

                    let pwm_raw = hwm_read(sess, chip.hwm_base, pwm_out_regs[i]).unwrap_or(0);
                    let pwm_pct = ((pwm_raw as u32 * 100 + 127) / 255).min(100) as u8;
                    let mode_raw =
                        hwm_read(sess, chip.hwm_base, FAN_CTRL_MODE_REGS[i]).unwrap_or(0xFF);

                    fans.push(SuperIoFanChannel {
                        index: i,
                        label: format!("Fan Header #{}", i + 1),
                        rpm,
                        pwm_pct,
                        mode_raw,
                        is_manual: (mode_raw >> 4) == 0,
                    });
                }
            }
            ChipFamily::NuvotonEc { dr_handshake } => {
                for &(reg, label, is_sys) in &[
                    (0x0102u16, "Carte mère (Système)", true),
                    (0x0104u16, "MOS / VRM", false),
                    (0x0106u16, "Chipset (PCH)", false),
                ] {
                    if let (Ok(hi), Ok(lo)) = (
                        ec_read(sess, chip.hwm_base, reg),
                        ec_read(sess, chip.hwm_base, reg + 1),
                    ) && let Some(t) = decode_nuvoton_temp_half(hi, Some(lo), 7)
                    {
                        if is_sys || motherboard_temp_c.is_none() {
                            motherboard_temp_c = Some(t);
                        }
                        temps.push(SuperIoTempSensor {
                            label: label.into(),
                            temp_c: t,
                        });
                    }
                }
                for (disp_idx, ch) in ec_channels(dr_handshake).into_iter().enumerate() {
                    let Ok(hi) = ec_read(sess, chip.hwm_base, ch.rpm_reg) else {
                        *guard = None;
                        return SuperIoSnapshot {
                            diagnostic: "Erreur lecture RPM SuperIO EC.".into(),
                            ..Default::default()
                        };
                    };
                    let Ok(lo) = ec_read(sess, chip.hwm_base, ch.rpm_reg + 1) else {
                        *guard = None;
                        return SuperIoSnapshot {
                            diagnostic: "Erreur lecture RPM SuperIO EC.".into(),
                            ..Default::default()
                        };
                    };
                    let rpm = nuvoton_rpm_from_regs(hi, lo, false);
                    let pwm_raw = ec_read(sess, chip.hwm_base, ch.pwm_out_reg).unwrap_or(0);
                    let pwm_pct = ((pwm_raw as u32 * 100 + 127) / 255).min(100) as u8;
                    let mode_raw = ec_read(sess, chip.hwm_base, ch.mode_reg).unwrap_or(0);
                    let is_manual = (mode_raw & (1 << ch.mode_bit)) != 0;

                    fans.push(SuperIoFanChannel {
                        index: ch.index,
                        label: format!("Fan Header #{}", disp_idx + 1),
                        rpm,
                        pwm_pct,
                        mode_raw,
                        is_manual,
                    });
                }
            }
            ChipFamily::Ite(spec) => {
                for (idx, reg) in [0x29u8, 0x2Au8, 0x2Bu8].into_iter().enumerate() {
                    if let Ok(raw) = ite_ec_read(sess, chip.hwm_base, reg)
                        && let Some(t) = decode_signed_byte_temp(raw)
                    {
                        if idx == 0 || motherboard_temp_c.is_none() {
                            motherboard_temp_c = Some(t);
                        }
                        temps.push(SuperIoTempSensor {
                            label: format!("Sonde CM #{}", idx + 1),
                            temp_c: t,
                        });
                    }
                }
                let reg_16bit = ite_ec_read(sess, chip.hwm_base, 0x0C).unwrap_or(0);
                let (tach_lsb, tach_msb, pwm_ctrl) = if spec.alt_regs {
                    (
                        &ITE_TACH_LSB_ALT_REGS,
                        &ITE_TACH_MSB_ALT_REGS,
                        &ITE_PWM_CTRL_ALT_REGS,
                    )
                } else {
                    (&ITE_TACH_LSB_REGS, &ITE_TACH_MSB_REGS, &ITE_PWM_CTRL_REGS)
                };
                for i in 0..chip.fan_count {
                    let Ok(lsb) = ite_ec_read(sess, chip.hwm_base, tach_lsb[i]) else {
                        *guard = None;
                        return SuperIoSnapshot {
                            diagnostic: "Erreur lecture RPM ITE SuperIO.".into(),
                            ..Default::default()
                        };
                    };
                    let is_16bit = i >= 3 || (reg_16bit & (1 << i)) != 0;
                    let count = if is_16bit {
                        let Ok(msb) = ite_ec_read(sess, chip.hwm_base, tach_msb[i]) else {
                            *guard = None;
                            return SuperIoSnapshot {
                                diagnostic: "Erreur lecture RPM ITE SuperIO.".into(),
                                ..Default::default()
                            };
                        };
                        ((msb as u32) << 8) | (lsb as u32)
                    } else if lsb == 0xFF {
                        0xFFFF
                    } else {
                        lsb as u32
                    };
                    let rpm = ite_rpm_from_count(count);
                    let ctrl = ite_ec_read(sess, chip.hwm_base, pwm_ctrl[i]).unwrap_or(0x80);
                    let is_manual = (ctrl & 0x80) == 0;
                    let pwm_pct = if spec.pwm_8bit {
                        let raw =
                            ite_ec_read(sess, chip.hwm_base, ITE_PWM_DUTY_REGS[i]).unwrap_or(0);
                        ((raw as u32 * 100 + 127) / 255).min(100) as u8
                    } else if is_manual {
                        (((ctrl & 0x7F) as u32 * 100 + 63) / 127).min(100) as u8
                    } else {
                        0
                    };
                    fans.push(SuperIoFanChannel {
                        index: i,
                        label: format!("Fan Header #{}", i + 1),
                        rpm,
                        pwm_pct,
                        mode_raw: ctrl,
                        is_manual,
                    });
                }
            }
            ChipFamily::Fintek(spec) => {
                for i in 0..spec.temp_count {
                    let temp_c = if spec.is_f71858 {
                        let hi =
                            ite_ec_read(sess, chip.hwm_base, FINTEK_TEMP_BASE_REG + 2 * i as u8)
                                .unwrap_or(0);
                        let lo = ite_ec_read(
                            sess,
                            chip.hwm_base,
                            FINTEK_TEMP_BASE_REG + 2 * i as u8 + 1,
                        )
                        .unwrap_or(0);
                        let raw = (((hi as i16) << 8) | lo as i16) & 0x07FF;
                        sanitize_superio_temp(raw as f32 / 128.0)
                    } else {
                        ite_ec_read(
                            sess,
                            chip.hwm_base,
                            FINTEK_TEMP_BASE_REG + 2 * (i as u8 + 1),
                        )
                        .ok()
                        .and_then(decode_signed_byte_temp)
                    };
                    if let Some(t) = temp_c {
                        if i == 0 || motherboard_temp_c.is_none() {
                            motherboard_temp_c = Some(t);
                        }
                        temps.push(SuperIoTempSensor {
                            label: format!("Sonde CM #{}", i + 1),
                            temp_c: t,
                        });
                    }
                }
                let enable_reg = if spec.has_pwm_enable {
                    ite_ec_read(sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG).unwrap_or(0)
                } else {
                    0
                };
                for i in 0..spec.pwm_count.min(FINTEK_PWM_REGS.len()) {
                    let Ok(hi) = ite_ec_read(sess, chip.hwm_base, FINTEK_TACH_REGS[i]) else {
                        *guard = None;
                        return SuperIoSnapshot {
                            diagnostic: "Erreur lecture RPM Fintek SuperIO.".into(),
                            ..Default::default()
                        };
                    };
                    let Ok(lo) = ite_ec_read(sess, chip.hwm_base, FINTEK_TACH_REGS[i] + 1) else {
                        *guard = None;
                        return SuperIoSnapshot {
                            diagnostic: "Erreur lecture RPM Fintek SuperIO.".into(),
                            ..Default::default()
                        };
                    };
                    let rpm = fintek_rpm_from_regs(hi, lo);
                    let pwm_raw = ite_ec_read(sess, chip.hwm_base, FINTEK_PWM_REGS[i]).unwrap_or(0);
                    let pwm_pct = ((pwm_raw as u32 * 100 + 127) / 255).min(100) as u8;
                    let mask = fintek_pwm_enable_mask(i);
                    let is_manual = !spec.has_pwm_enable || (enable_reg & mask) == mask;
                    fans.push(SuperIoFanChannel {
                        index: i,
                        label: format!("Fan Header #{}", i + 1),
                        rpm,
                        pwm_pct,
                        mode_raw: enable_reg,
                        is_manual,
                    });
                }
            }
        }

        let vendor_note = match chip.family {
            ChipFamily::NuvotonBank { .. } => "Nuvoton Bank vérifié",
            ChipFamily::NuvotonEc { dr_handshake: true } => "Nuvoton eSIO EC (DR) vérifié",
            ChipFamily::NuvotonEc {
                dr_handshake: false,
            } => "Nuvoton eSIO EC vérifié",
            ChipFamily::Ite(_) => "Vendor ITE vérifié",
            ChipFamily::Fintek(_) => "Vendor Fintek vérifié",
        };
        SuperIoSnapshot {
            detected: true,
            chip_name: chip.chip_name.to_string(),
            chip_id_hex: format!("0x{:04X}", chip.chip_id),
            hwm_base_hex: format!("0x{:04X} (slot {})", chip.hwm_base, chip.slot),
            motherboard_temp_c,
            temps,
            diagnostic: format!(
                "{} (0x{:04X}) @ 0x{:04X} — {} canaux PWM/RPM actifs ({vendor_note})",
                chip.chip_name,
                chip.chip_id,
                chip.hwm_base,
                fans.len()
            ),
            fans,
        }
    }

    fn ec_apply_state(
        sess: &PawnIoSession,
        base: u16,
        dr_handshake: bool,
        ch: EcChannelRegs,
        set_manual_bit: bool,
        cmd_raw: u8,
    ) -> Result<()> {
        let bit_mask = 1u8 << ch.mode_bit;
        let or_mask = if set_manual_bit { bit_mask } else { 0 };
        if dr_handshake {
            for _attempt in 0..3 {
                if !ec_dr_start_cfg(sess, base)? {
                    continue;
                }
                ec_update(sess, base, ch.mode_reg, !bit_mask, or_mask)?;
                ec_write(sess, base, ch.pwm_cmd_reg, cmd_raw)?;
                if ec_dr_complete_cfg(sess, base)? {
                    return Ok(());
                }
            }
            anyhow::bail!("Handshake EC NCT6687D-R refusé par le contrôleur");
        } else {
            ec_write(sess, base, EC_FAN_CFG_CTRL_REG, EC_FAN_CFG_REQ_BIT)?;
            std::thread::sleep(std::time::Duration::from_millis(25));
            ec_update(sess, base, ch.mode_reg, !bit_mask, or_mask)?;
            ec_write(sess, base, ch.pwm_cmd_reg, cmd_raw)?;
            ec_write(sess, base, EC_FAN_CFG_CTRL_REG, EC_FAN_CFG_DONE_BIT)?;
            std::thread::sleep(std::time::Duration::from_millis(25));
            Ok(())
        }
    }

    pub fn set_fan_pwm(channel_idx: usize, duty_pct: u8, min_floor_pct: u8) -> Result<String> {
        let duty_pct = safe_superio_duty_with_floor(duty_pct, min_floor_pct);
        if channel_idx >= MAX_SUPERIO_CHANNELS {
            anyhow::bail!("Canal SuperIO invalide ({channel_idx})");
        }

        let _lock = NamedMutexGuard::acquire(ISA_MUTEX, 1500).context("Bus SuperIO ISA occupé")?;
        let (sess, chip) = detect_and_prepare()?;
        match chip.family {
            ChipFamily::NuvotonBank { use_98_pwm_out, .. } => {
                if channel_idx >= chip.fan_count {
                    anyhow::bail!("Canal #{channel_idx} hors plage pour {}", chip.chip_name);
                }
                let mode_reg = FAN_CTRL_MODE_REGS[channel_idx];
                let cmd_reg = FAN_PWM_CMD_REGS[channel_idx];
                let pwm_out_reg = if use_98_pwm_out {
                    FAN_PWM_OUT_REGS_98[channel_idx]
                } else {
                    FAN_PWM_OUT_REGS_95[channel_idx]
                };

                let cur_mode = hwm_read(&sess, chip.hwm_base, mode_reg)?;
                let cur_cmd = hwm_read(&sess, chip.hwm_base, cmd_reg)?;
                if let Ok(mut guard) = saved_states().lock()
                    && guard[channel_idx].is_none()
                {
                    guard[channel_idx] = Some(SavedFanState {
                        mode: cur_mode,
                        cmd: cur_cmd,
                        extra: 0,
                    });
                }

                let target_raw = ((duty_pct.min(100) as u32 * 255 + 50) / 100).min(255) as u8;
                let new_mode = manual_mode_byte(cur_mode);
                hwm_write(&sess, chip.hwm_base, mode_reg, new_mode)?;
                if let Err(e) = hwm_write(&sess, chip.hwm_base, cmd_reg, target_raw) {
                    let _ = hwm_write(&sess, chip.hwm_base, mode_reg, cur_mode);
                    return Err(e);
                }
                drop(_lock);
                std::thread::sleep(std::time::Duration::from_millis(25));
                let _lock = NamedMutexGuard::acquire(ISA_MUTEX, 1500)
                    .context("Bus SuperIO ISA occupé lors de la vérification")?;
                let rb_mode = hwm_read(&sess, chip.hwm_base, mode_reg)?;
                let rb_cmd = hwm_read(&sess, chip.hwm_base, cmd_reg)?;
                let rb_out = hwm_read(&sess, chip.hwm_base, pwm_out_reg)?;
                let rb_pct = ((rb_out as u32 * 100 + 127) / 255).min(100) as u8;

                if (rb_mode >> 4) != 0 || rb_cmd.abs_diff(target_raw) > 2 {
                    let _ = hwm_write(&sess, chip.hwm_base, cmd_reg, cur_cmd);
                    let _ = hwm_write(&sess, chip.hwm_base, mode_reg, cur_mode);
                    anyhow::bail!(
                        "Échec vérification SuperIO ch#{} (rollback appliqué): écrit mode=0x{:02X}/cmd={target_raw}, relu mode=0x{rb_mode:02X}/cmd={rb_cmd} (sortie={rb_pct}%)",
                        channel_idx + 1,
                        new_mode
                    );
                }
                Ok(format!(
                    "SuperIO {} ch#{} -> {}% vérifié (cmd={}, sortie={}%, mode=Manuel)",
                    chip.chip_name,
                    channel_idx + 1,
                    duty_pct.min(100),
                    rb_cmd,
                    rb_pct
                ))
            }
            ChipFamily::NuvotonEc { dr_handshake } => {
                let ch = ec_channel_for(dr_handshake, channel_idx).ok_or_else(|| {
                    anyhow::anyhow!("Canal #{channel_idx} hors plage pour {}", chip.chip_name)
                })?;
                let bit_mask = 1u8 << ch.mode_bit;
                let cur_mode = ec_read(&sess, chip.hwm_base, ch.mode_reg)?;
                let cur_cmd = ec_read(&sess, chip.hwm_base, ch.pwm_cmd_reg)?;
                if let Ok(mut guard) = saved_states().lock()
                    && guard[channel_idx].is_none()
                {
                    guard[channel_idx] = Some(SavedFanState {
                        mode: cur_mode & bit_mask,
                        cmd: cur_cmd,
                        extra: 0,
                    });
                }

                let target_raw = ((duty_pct.min(100) as u32 * 255 + 50) / 100).min(255) as u8;
                ec_apply_state(&sess, chip.hwm_base, dr_handshake, ch, true, target_raw)?;

                let rb_mode = ec_read(&sess, chip.hwm_base, ch.mode_reg)?;
                let rb_cmd = ec_read(&sess, chip.hwm_base, ch.pwm_cmd_reg)?;
                let rb_out = ec_read(&sess, chip.hwm_base, ch.pwm_out_reg)?;
                let rb_pct = ((rb_out as u32 * 100 + 127) / 255).min(100) as u8;
                if (rb_mode & bit_mask) == 0 || rb_cmd.abs_diff(target_raw) > 2 {
                    let _ = ec_apply_state(
                        &sess,
                        chip.hwm_base,
                        dr_handshake,
                        ch,
                        (cur_mode & bit_mask) != 0,
                        cur_cmd,
                    );
                    anyhow::bail!(
                        "Échec vérification SuperIO EC ch#{} (rollback appliqué): relu mode=0x{rb_mode:02X}/cmd={rb_cmd}",
                        channel_idx + 1
                    );
                }
                Ok(format!(
                    "SuperIO {} ch#{} -> {}% vérifié (cmd={}, sortie={}%, mode=Manuel)",
                    chip.chip_name,
                    channel_idx + 1,
                    duty_pct.min(100),
                    rb_cmd,
                    rb_pct
                ))
            }
            ChipFamily::Ite(spec) => {
                if channel_idx >= chip.fan_count {
                    anyhow::bail!("Canal #{channel_idx} hors plage pour {}", chip.chip_name);
                }
                let ctrl_regs = if spec.alt_regs {
                    &ITE_PWM_CTRL_ALT_REGS
                } else {
                    &ITE_PWM_CTRL_REGS
                };
                let ctrl_reg = ctrl_regs[channel_idx];
                let duty_reg = ITE_PWM_DUTY_REGS[channel_idx];
                let cur_ctrl = ite_ec_read(&sess, chip.hwm_base, ctrl_reg)?;
                let cur_duty = if spec.pwm_8bit {
                    ite_ec_read(&sess, chip.hwm_base, duty_reg)?
                } else {
                    cur_ctrl & 0x7F
                };
                let main_ctrl = if channel_idx < 3 {
                    ite_ec_read(&sess, chip.hwm_base, ITE_FAN_MAIN_CTRL_REG).unwrap_or(0)
                } else {
                    0
                };
                let main_was_enabled = channel_idx < 3 && (main_ctrl & (1 << channel_idx)) != 0;
                if let Ok(mut guard) = saved_states().lock()
                    && guard[channel_idx].is_none()
                {
                    guard[channel_idx] = Some(SavedFanState {
                        mode: cur_ctrl,
                        cmd: cur_duty,
                        extra: u8::from(channel_idx < 3 && !main_was_enabled),
                    });
                }

                if channel_idx < 3 && !main_was_enabled {
                    ite_ec_write(
                        &sess,
                        chip.hwm_base,
                        ITE_FAN_MAIN_CTRL_REG,
                        main_ctrl | (1 << channel_idx),
                    )?;
                }

                let (target_ctrl, target_duty) =
                    ite_manual_target(cur_ctrl, duty_pct, spec.pwm_8bit, spec.ctrl_0x7f_quirk);
                ite_ec_write(&sess, chip.hwm_base, ctrl_reg, target_ctrl)?;
                if spec.pwm_8bit
                    && let Err(e) = ite_ec_write(&sess, chip.hwm_base, duty_reg, target_duty)
                {
                    let _ = ite_ec_write(&sess, chip.hwm_base, ctrl_reg, cur_ctrl);
                    return Err(e);
                }
                drop(_lock);
                std::thread::sleep(std::time::Duration::from_millis(25));
                let _lock = NamedMutexGuard::acquire(ISA_MUTEX, 1500)
                    .context("Bus SuperIO ISA occupé lors de la vérification")?;
                let rb_ctrl = ite_ec_read(&sess, chip.hwm_base, ctrl_reg)?;
                let rb_duty = if spec.pwm_8bit {
                    ite_ec_read(&sess, chip.hwm_base, duty_reg)?
                } else {
                    rb_ctrl & 0x7F
                };
                if (rb_ctrl & 0x80) != 0 || rb_duty.abs_diff(target_duty) > 2 {
                    if spec.pwm_8bit {
                        let _ = ite_ec_write(&sess, chip.hwm_base, duty_reg, cur_duty);
                    }
                    let _ = ite_ec_write(&sess, chip.hwm_base, ctrl_reg, cur_ctrl);
                    if channel_idx < 3 && !main_was_enabled {
                        let _ =
                            ite_ec_write(&sess, chip.hwm_base, ITE_FAN_MAIN_CTRL_REG, main_ctrl);
                    }
                    anyhow::bail!(
                        "Échec vérification SuperIO ITE ch#{} (rollback appliqué): ctrl=0x{rb_ctrl:02X}, duty={rb_duty}",
                        channel_idx + 1
                    );
                }
                Ok(format!(
                    "SuperIO {} ch#{} -> {}% vérifié (duty={}, mode=Manuel)",
                    chip.chip_name,
                    channel_idx + 1,
                    duty_pct.min(100),
                    rb_duty
                ))
            }
            ChipFamily::Fintek(spec) => {
                if spec.pwm_count == 0 {
                    anyhow::bail!(
                        "SuperIO {} : contrôle PWM non pris en charge sur ce modèle",
                        chip.chip_name
                    );
                }
                let ch_cap = spec.pwm_count.min(FINTEK_PWM_REGS.len());
                if channel_idx >= ch_cap {
                    anyhow::bail!(
                        "Canal SuperIO #{} hors plage pour {} ({ch_cap} canaux PWM)",
                        channel_idx + 1,
                        chip.chip_name
                    );
                }
                let pwm_reg = FINTEK_PWM_REGS[channel_idx];
                let target_reg = FINTEK_TARGET_REGS[channel_idx];
                let mask = fintek_pwm_enable_mask(channel_idx);
                let cur_pwm = ite_ec_read(&sess, chip.hwm_base, pwm_reg)?;
                let cur_target = if spec.has_pwm_enable {
                    ite_ec_read(&sess, chip.hwm_base, target_reg)?
                } else {
                    0
                };
                let cur_enable = if spec.has_pwm_enable {
                    ite_ec_read(&sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG)?
                } else {
                    mask
                };
                if let Ok(mut guard) = saved_states().lock()
                    && guard[channel_idx].is_none()
                {
                    guard[channel_idx] = Some(SavedFanState {
                        mode: cur_enable & mask,
                        cmd: cur_pwm,
                        extra: cur_target,
                    });
                }
                let target_duty = ((duty_pct as u32 * 255 + 50) / 100).min(255) as u8;
                if spec.has_pwm_enable
                    && let Err(e) = ite_ec_write(
                        &sess,
                        chip.hwm_base,
                        FINTEK_PWM_ENABLE_REG,
                        (cur_enable & !mask) | mask,
                    )
                {
                    let _ = ite_ec_write(&sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG, cur_enable);
                    return Err(e);
                }
                if let Err(e) = ite_ec_write(&sess, chip.hwm_base, pwm_reg, target_duty) {
                    let _ = ite_ec_write(&sess, chip.hwm_base, pwm_reg, cur_pwm);
                    if spec.has_pwm_enable {
                        let _ = ite_ec_write(&sess, chip.hwm_base, target_reg, cur_target);
                        let _ =
                            ite_ec_write(&sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG, cur_enable);
                    }
                    return Err(e);
                }
                drop(_lock);
                std::thread::sleep(std::time::Duration::from_millis(25));
                let _lock = NamedMutexGuard::acquire(ISA_MUTEX, 1500)
                    .context("Bus SuperIO ISA occupé lors de la vérification")?;
                let rb_duty = ite_ec_read(&sess, chip.hwm_base, pwm_reg)?;
                let rb_enable = if spec.has_pwm_enable {
                    ite_ec_read(&sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG)?
                } else {
                    mask
                };
                if (spec.has_pwm_enable && (rb_enable & mask) != mask)
                    || rb_duty.abs_diff(target_duty) > 2
                {
                    let _ = ite_ec_write(&sess, chip.hwm_base, pwm_reg, cur_pwm);
                    if spec.has_pwm_enable {
                        let _ = ite_ec_write(&sess, chip.hwm_base, target_reg, cur_target);
                        let _ =
                            ite_ec_write(&sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG, cur_enable);
                    }
                    anyhow::bail!(
                        "Échec vérification SuperIO Fintek ch#{} (rollback appliqué): enable=0x{rb_enable:02X}, duty={rb_duty}",
                        channel_idx + 1
                    );
                }
                Ok(format!(
                    "SuperIO {} ch#{} -> {}% vérifié (duty={}, mode=Manuel)",
                    chip.chip_name,
                    channel_idx + 1,
                    duty_pct,
                    rb_duty
                ))
            }
        }
    }

    pub fn restore_fan_auto(channel_idx: usize) -> Result<String> {
        if channel_idx >= MAX_SUPERIO_CHANNELS {
            anyhow::bail!("Canal SuperIO invalide ({channel_idx})");
        }
        let _lock = NamedMutexGuard::acquire(ISA_MUTEX, 1500).context("Bus SuperIO ISA occupé")?;
        let saved = saved_states().lock().ok().and_then(|g| g[channel_idx]);
        let Some(saved) = saved else {
            return Ok(format!(
                "SuperIO ch#{} jamais modifié par OpenGear — réglage BIOS inchangé",
                channel_idx + 1
            ));
        };
        let (sess, chip) = detect_and_prepare()?;
        match chip.family {
            ChipFamily::NuvotonBank { .. } => {
                hwm_write(
                    &sess,
                    chip.hwm_base,
                    FAN_PWM_CMD_REGS[channel_idx],
                    saved.cmd,
                )?;
                hwm_write(
                    &sess,
                    chip.hwm_base,
                    FAN_CTRL_MODE_REGS[channel_idx],
                    saved.mode,
                )?;
            }
            ChipFamily::NuvotonEc { dr_handshake } => {
                if let Some(ch) = ec_channel_for(dr_handshake, channel_idx) {
                    let bit_mask = 1u8 << ch.mode_bit;
                    ec_apply_state(
                        &sess,
                        chip.hwm_base,
                        dr_handshake,
                        ch,
                        (saved.mode & bit_mask) != 0,
                        saved.cmd,
                    )?;
                }
            }
            ChipFamily::Ite(spec) => {
                let ctrl_regs = if spec.alt_regs {
                    &ITE_PWM_CTRL_ALT_REGS
                } else {
                    &ITE_PWM_CTRL_REGS
                };
                if spec.pwm_8bit {
                    ite_ec_write(
                        &sess,
                        chip.hwm_base,
                        ITE_PWM_DUTY_REGS[channel_idx],
                        saved.cmd,
                    )?;
                }
                ite_ec_write(&sess, chip.hwm_base, ctrl_regs[channel_idx], saved.mode)?;
                if saved.extra != 0 && channel_idx < 3 {
                    let cur_main =
                        ite_ec_read(&sess, chip.hwm_base, ITE_FAN_MAIN_CTRL_REG).unwrap_or(0);
                    let _ = ite_ec_write(
                        &sess,
                        chip.hwm_base,
                        ITE_FAN_MAIN_CTRL_REG,
                        cur_main & !(1 << channel_idx),
                    );
                }
            }
            ChipFamily::Fintek(spec) => {
                if channel_idx < FINTEK_PWM_REGS.len() {
                    if spec.has_pwm_enable {
                        let mask = fintek_pwm_enable_mask(channel_idx);
                        let cur_enable =
                            ite_ec_read(&sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG).unwrap_or(0);
                        ite_ec_write(
                            &sess,
                            chip.hwm_base,
                            FINTEK_PWM_ENABLE_REG,
                            (cur_enable & !mask) | (saved.mode & mask),
                        )?;
                        ite_ec_write(
                            &sess,
                            chip.hwm_base,
                            FINTEK_TARGET_REGS[channel_idx],
                            saved.extra,
                        )?;
                    }
                    ite_ec_write(
                        &sess,
                        chip.hwm_base,
                        FINTEK_PWM_REGS[channel_idx],
                        saved.cmd,
                    )?;
                }
            }
        }
        if let Ok(mut guard) = saved_states().lock() {
            guard[channel_idx] = None;
        }
        Ok(format!(
            "SuperIO {} ch#{} restauré en mode BIOS d'origine (mode=0x{:02X}, cmd={})",
            chip.chip_name,
            channel_idx + 1,
            saved.mode,
            saved.cmd
        ))
    }

    pub fn restore_saved_states(states: &[SuperIoSavedState]) -> Vec<String> {
        if states.is_empty() {
            return Vec::new();
        }
        let Some(_lock) = NamedMutexGuard::acquire(ISA_MUTEX, 3000) else {
            return vec!["SuperIO : bus ISA occupé, restauration impossible".into()];
        };
        let (sess, chip) = match detect_and_prepare() {
            Ok(v) => v,
            Err(e) => return vec![format!("SuperIO : {e}")],
        };
        states
            .iter()
            .map(|s| {
                let r = match chip.family {
                    ChipFamily::NuvotonBank { .. } => {
                        if s.index >= chip.fan_count {
                            Ok(())
                        } else {
                            hwm_write(&sess, chip.hwm_base, FAN_PWM_CMD_REGS[s.index], s.cmd)
                                .and_then(|_| {
                                    hwm_write(
                                        &sess,
                                        chip.hwm_base,
                                        FAN_CTRL_MODE_REGS[s.index],
                                        s.mode,
                                    )
                                })
                        }
                    }
                    ChipFamily::NuvotonEc { dr_handshake } => {
                        match ec_channel_for(dr_handshake, s.index) {
                            Some(ch) => {
                                let bit_mask = 1u8 << ch.mode_bit;
                                ec_apply_state(
                                    &sess,
                                    chip.hwm_base,
                                    dr_handshake,
                                    ch,
                                    (s.mode & bit_mask) != 0,
                                    s.cmd,
                                )
                            }
                            None => Ok(()),
                        }
                    }
                    ChipFamily::Ite(spec) => {
                        if s.index >= chip.fan_count {
                            Ok(())
                        } else {
                            let ctrl_regs = if spec.alt_regs {
                                &ITE_PWM_CTRL_ALT_REGS
                            } else {
                                &ITE_PWM_CTRL_REGS
                            };
                            let res = if spec.pwm_8bit {
                                ite_ec_write(
                                    &sess,
                                    chip.hwm_base,
                                    ITE_PWM_DUTY_REGS[s.index],
                                    s.cmd,
                                )
                            } else {
                                Ok(())
                            };
                            res.and_then(|_| {
                                ite_ec_write(&sess, chip.hwm_base, ctrl_regs[s.index], s.mode)
                            })
                            .and_then(|_| {
                                if s.extra != 0 && s.index < 3 {
                                    let cur_main =
                                        ite_ec_read(&sess, chip.hwm_base, ITE_FAN_MAIN_CTRL_REG)
                                            .unwrap_or(0);
                                    ite_ec_write(
                                        &sess,
                                        chip.hwm_base,
                                        ITE_FAN_MAIN_CTRL_REG,
                                        cur_main & !(1 << s.index),
                                    )
                                } else {
                                    Ok(())
                                }
                            })
                        }
                    }
                    ChipFamily::Fintek(spec) => {
                        if s.index >= FINTEK_PWM_REGS.len() {
                            Ok(())
                        } else {
                            let res = if spec.has_pwm_enable {
                                let mask = fintek_pwm_enable_mask(s.index);
                                let cur_enable =
                                    ite_ec_read(&sess, chip.hwm_base, FINTEK_PWM_ENABLE_REG)
                                        .unwrap_or(0);
                                ite_ec_write(
                                    &sess,
                                    chip.hwm_base,
                                    FINTEK_PWM_ENABLE_REG,
                                    (cur_enable & !mask) | (s.mode & mask),
                                )
                                .and_then(|_| {
                                    ite_ec_write(
                                        &sess,
                                        chip.hwm_base,
                                        FINTEK_TARGET_REGS[s.index],
                                        s.extra,
                                    )
                                })
                            } else {
                                Ok(())
                            };
                            res.and_then(|_| {
                                ite_ec_write(&sess, chip.hwm_base, FINTEK_PWM_REGS[s.index], s.cmd)
                            })
                        }
                    }
                };
                match r {
                    Ok(()) => {
                        format!(
                            "SuperIO ch#{} restauré (mode=0x{:02X})",
                            s.index + 1,
                            s.mode
                        )
                    }
                    Err(e) => format!("SuperIO ch#{} : restauration échouée : {e}", s.index + 1),
                }
            })
            .collect()
    }

    pub fn restore_all_fans_auto() -> Vec<String> {
        let states = saved_superio_states();
        if states.is_empty() {
            return Vec::new();
        }
        let out = restore_saved_states(&states);
        if let Ok(mut guard) = saved_states().lock() {
            for s in &states {
                if s.index < guard.len() {
                    guard[s.index] = None;
                }
            }
        }
        out
    }
}

pub fn read_superio_snapshot() -> SuperIoSnapshot {
    #[cfg(windows)]
    {
        win_sio::read_snapshot()
    }
    #[cfg(not(windows))]
    SuperIoSnapshot::default()
}

pub fn set_superio_fan_pwm(channel_idx: usize, duty_pct: u8) -> Result<String> {
    set_superio_fan_pwm_with_floor(channel_idx, duty_pct, SUPERIO_MIN_PWM_PCT)
}

pub fn set_superio_fan_pwm_with_floor(
    channel_idx: usize,
    duty_pct: u8,
    min_floor_pct: u8,
) -> Result<String> {
    #[cfg(windows)]
    {
        win_sio::set_fan_pwm(channel_idx, duty_pct, min_floor_pct)
    }
    #[cfg(not(windows))]
    {
        let _ = (channel_idx, duty_pct, min_floor_pct);
        anyhow::bail!("Non supporté hors Windows")
    }
}

pub fn restore_superio_saved_states(states: &[SuperIoSavedState]) -> Vec<String> {
    #[cfg(windows)]
    {
        win_sio::restore_saved_states(states)
    }
    #[cfg(not(windows))]
    {
        let _ = states;
        Vec::new()
    }
}

pub fn restore_all_superio_fans_auto() -> Vec<String> {
    #[cfg(windows)]
    {
        win_sio::restore_all_fans_auto()
    }
    #[cfg(not(windows))]
    Vec::new()
}

pub fn restore_superio_fan_auto(channel_idx: usize) -> Result<String> {
    #[cfg(windows)]
    {
        win_sio::restore_fan_auto(channel_idx)
    }
    #[cfg(not(windows))]
    {
        let _ = channel_idx;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Shared `Driver` exposing SuperIO (Nuvoton / ITE) fan headers via the common `Cooling` trait.
pub struct SuperIoCoolingDriver;

impl Driver for SuperIoCoolingDriver {
    fn name(&self) -> &'static str {
        "superio"
    }

    fn display_name(&self) -> &'static str {
        "Contrôleur SuperIO carte mère (PawnIO)"
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        let snap = read_superio_snapshot();
        if !snap.detected {
            return Vec::new();
        }
        vec![Box::new(SuperIoCoolingDevice { snap })]
    }
}

/// Detected SuperIO fan controller exposed behind the shared `Cooling` capability.
pub struct SuperIoCoolingDevice {
    snap: SuperIoSnapshot,
}

impl SuperIoCoolingDevice {
    fn parse_fan_index(id: &str) -> Option<usize> {
        id.strip_prefix("fan_")?.parse::<usize>().ok()
    }

    fn fan_info(f: &SuperIoFanChannel) -> FanInfo {
        FanInfo {
            id: format!("fan_{}", f.index),
            name: f.label.clone(),
            rpm: Some(f.rpm),
            max_rpm: None,
            min_pct: SUPERIO_MIN_PWM_PCT,
            manual_pct: f.is_manual.then_some(f.pwm_pct),
        }
    }
}

impl Device for SuperIoCoolingDevice {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: "superio:mainboard".into(),
            vendor: "Carte mère".into(),
            model: format!("SuperIO {}", self.snap.chip_name),
            kind: DeviceKind::Motherboard,
            connection: Connection::Internal,
            firmware: Some(self.snap.chip_id_hex.clone()),
        }
    }

    fn specs(&self) -> Vec<DeviceSpec> {
        vec![
            DeviceSpec::expert(
                "Puce de contrôle",
                format!("{} ({})", self.snap.chip_name, self.snap.chip_id_hex),
                true,
            ),
            DeviceSpec::simple("Canaux PWM", self.snap.fans.len().to_string()),
            DeviceSpec::simple("Diagnostic SuperIO", self.snap.diagnostic.clone()),
        ]
    }

    fn cooling(&mut self) -> Option<&mut dyn Cooling> {
        Some(self)
    }
}

impl Cooling for SuperIoCoolingDevice {
    fn fans(&self) -> Vec<FanInfo> {
        self.snap.fans.iter().map(Self::fan_info).collect()
    }

    fn refresh(&mut self) {
        self.snap = read_superio_snapshot();
    }

    fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()> {
        let channels: Vec<(usize, FanInfo)> = match fan {
            Some(id) => {
                let idx = Self::parse_fan_index(id)
                    .ok_or_else(|| anyhow::anyhow!("Canal SuperIO invalide : {id}"))?;
                let f = self
                    .snap
                    .fans
                    .iter()
                    .find(|c| c.index == idx)
                    .ok_or_else(|| anyhow::anyhow!("Canal SuperIO introuvable : {id}"))?;
                vec![(idx, Self::fan_info(f))]
            }
            None => self
                .snap
                .fans
                .iter()
                .map(|f| (f.index, Self::fan_info(f)))
                .collect(),
        };
        for (idx, info) in channels {
            match clamp_target(&info, target) {
                FanTarget::Auto => {
                    restore_superio_fan_auto(idx)?;
                }
                FanTarget::Fixed(pct) => {
                    set_superio_fan_pwm_with_floor(idx, pct, SUPERIO_MIN_PWM_PCT)?;
                }
            }
        }
        self.refresh();
        Ok(())
    }

    fn overridden(&self) -> bool {
        !saved_superio_states().is_empty()
    }

    fn restore_firmware_control(&mut self) -> Result<()> {
        let _ = restore_all_superio_fans_auto();
        self.refresh();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_duty_is_floored_and_capped() {
        assert_eq!(safe_superio_duty(0), SUPERIO_MIN_PWM_PCT);
        assert_eq!(
            safe_superio_duty(SUPERIO_MIN_PWM_PCT - 1),
            SUPERIO_MIN_PWM_PCT
        );
        assert_eq!(safe_superio_duty(55), 55);
        assert_eq!(safe_superio_duty(250), 100);
        assert_eq!(safe_superio_duty_with_floor(10, 45), 45);
    }

    #[test]
    fn mode_lower_nibble_and_rpm_table_selection() {
        assert_eq!(manual_mode_byte(0x5A), 0x0A);
        let (_, _, _, use_96_old) = identify_nuvoton(0xD121).unwrap();
        let (_, _, _, use_96_new) = identify_nuvoton(0xD42B).unwrap();
        assert!(!use_96_old);
        assert!(use_96_new);
    }

    #[test]
    fn expanded_nuvoton_ids_and_ec_distinction() {
        // Legacy NCT6771F / NCT6776F & NCT6779D & NCT6701D / NCT5585D
        let b71 = identify_nuvoton_bank(0xB470).unwrap();
        assert_eq!(b71.rpm_mode, NuvotonRpmMode::Rpm656Direct);
        let b79 = identify_nuvoton_bank(0xC560).unwrap();
        assert_eq!(b79.rpm_mode, NuvotonRpmMode::Rpm4B0Count13);
        let b01 = identify_nuvoton_bank(0xD806).unwrap();
        assert_eq!(b01.name, "Nuvoton NCT6701D");
        assert!(b01.allow_no_vendor_id);
        let b5585 = identify_nuvoton_bank(0xD42A).unwrap();
        assert_eq!(b5585.fan_count, 7);

        // 0xD592 must resolve to NCT6687D eSIO EC rather than banked NCT6796D-S
        assert!(identify_nuvoton_bank(0xD592).is_none());
        assert_eq!(identify_nuvoton_ec(0xC732), Some("Nuvoton NCT6683D"));
        assert_eq!(identify_nuvoton_ec(0xD441), Some("Nuvoton NCT6686D"));
        assert_eq!(identify_nuvoton_ec(0xD592), Some("Nuvoton NCT6687D"));
        assert!(is_800_series_ec_dr_board(0xD592, "MAG B850 TOMAHAWK WIFI"));
        assert!(!is_800_series_ec_dr_board(0xD592, "B550-A PRO"));

        // EC channel register mappings stay bounded and non-overlapping
        assert_eq!(ec_standard_channel(2).unwrap().pwm_cmd_reg, 0x0A2A);
        assert!(ec_standard_channel(8).is_none());
        assert_eq!(ec_dr_channel(10).unwrap().mode_reg, 0x080F);
        assert_eq!(ec_dr_channel(10).unwrap().mode_bit, 7);
        assert!(ec_dr_channel(4).is_none());
    }

    #[test]
    fn ite_chip_identification_rpm_and_manual_pwm() {
        assert_eq!(identify_ite(0x8688), Some(("ITE IT8688E", 5, true)));
        assert_eq!(identify_ite(0x8728), Some(("ITE IT8728F", 5, false)));
        assert_eq!(identify_ite(0x8733), Some(("ITE IT8792E/IT8795E", 3, true)));
        assert_eq!(identify_ite(0x8695), Some(("ITE IT87952E", 3, true)));
        assert!(identify_ite_spec(0x8689).unwrap().ctrl_0x7f_quirk);
        assert!(identify_ite_spec(0x8665).unwrap().alt_regs);
        assert_eq!(identify_ite(0x9999), None);
        assert_eq!(ite_rpm_from_count(0), 0);
        assert_eq!(ite_rpm_from_count(0xFFFF), 0);
        assert_eq!(ite_rpm_from_count(675), 1000);
        assert_eq!(ite_manual_target(0x90, 50, true, false), (0x10, 128));
        assert_eq!(ite_manual_target(0x90, 50, true, true), (0x7F, 128));
        assert_eq!(ite_manual_target(0x80, 100, false, false), (127, 127));
    }

    #[test]
    fn superio_cooling_device_parses_ids_and_enforces_floor() {
        assert_eq!(SuperIoCoolingDevice::parse_fan_index("fan_2"), Some(2));
        assert_eq!(SuperIoCoolingDevice::parse_fan_index("cpu_fan"), None);
        let ch = SuperIoFanChannel {
            index: 1,
            label: "FAN2".into(),
            rpm: 1200,
            pwm_pct: 40,
            mode_raw: 0x01,
            is_manual: true,
        };
        let info = SuperIoCoolingDevice::fan_info(&ch);
        assert_eq!(info.id, "fan_1");
        assert_eq!(info.min_pct, SUPERIO_MIN_PWM_PCT);
        assert_eq!(info.manual_pct, Some(40));
    }

    #[test]
    fn nuvoton_13bit_count_decodes_real_nct6798d_samples() {
        // Real NCT6798D register pairs captured at 30 % and 80 % PWM.
        assert_eq!(nuvoton_rpm_from_regs(0x2D, 0x07, true), 932);
        assert_eq!(nuvoton_rpm_from_regs(0x14, 0x10, true), 2057);
        assert_eq!(nuvoton_rpm_from_regs(0xFF, 0x1F, true), 0);
        // Legacy direct RPM mode (NCT6791D..NCT6795D, NCT6771F/6776F, NCT668x).
        assert_eq!(nuvoton_rpm_from_regs(0x04, 0xB0, false), 1200);
    }

    #[test]
    fn fintek_f718xx_identification_and_decoding() {
        let f89 = identify_fintek(0x1005).unwrap();
        assert_eq!(f89.name, "Fintek F71889AD");
        assert!(f89.has_pwm_enable);
        assert_eq!(f89.pwm_count, 3);
        let f82 = identify_fintek(0x0541).unwrap();
        assert!(!f82.has_pwm_enable);
        assert_eq!(f82.pwm_count, 4);
        assert_eq!(fintek_rpm_from_regs(0x04, 0xB0), 1250);
        assert_eq!(fintek_rpm_from_regs(0x0F, 0xFF), 0);
        assert_eq!(fintek_pwm_enable_mask(0), 0x03);
        assert_eq!(fintek_pwm_enable_mask(2), 0x30);
    }

    #[test]
    fn native_superio_temperature_decoders_filter_sentinels() {
        assert_eq!(decode_nuvoton_temp_half(35, Some(0x80), 7), Some(35.5));
        assert_eq!(decode_nuvoton_temp_half(35, Some(0x00), 7), Some(35.0));
        assert_eq!(decode_nuvoton_temp_half(0x80, Some(0x80), 7), None);
        assert_eq!(decode_nuvoton_temp_half(0x7F, None, 0), None);
        assert_eq!(decode_signed_byte_temp(42), Some(42.0));
        assert_eq!(decode_signed_byte_temp(0x00), None);
        assert_eq!(sanitize_superio_temp(128.0), None);
    }

    #[test]
    #[ignore = "requires local PawnIO driver and physical SuperIO chip"]
    fn live_probe_superio_temps() {
        let snap = read_superio_snapshot();
        eprintln!(
            "SuperIO detected={} chip={} mb_temp={:?} temps={:?} fans={}",
            snap.detected,
            snap.chip_name,
            snap.motherboard_temp_c,
            snap.temps,
            snap.fans.len()
        );
        assert!(snap.detected, "{}", snap.diagnostic);
        assert!(
            snap.motherboard_temp_c.is_some(),
            "expected native motherboard temp sensor on {}",
            snap.chip_name
        );
    }
}
