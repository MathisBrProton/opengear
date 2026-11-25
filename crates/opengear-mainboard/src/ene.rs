//! ENE DRAM RGB controller (used by many DDR4 / DDR5 RGB modules), over any [`SmbusBus`].
//!
//! The register protocol lives here once; transports (PawnIO, ASUS WMI…) only provide the bus.
//!
//! Safety: nothing is written unless the 16-byte read-only ENE fingerprint
//! (`0xA0..0xAF == 0x00..0x0F`) matches and the module is not a Micron part (`0x1030 == "Micron"`).

use crate::smbus::SmbusBus;
use anyhow::Result;
use opengear_core::Rgb;
use serde::{Deserialize, Serialize};

/// One ENE RAM controller found on a bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EneRamModule {
    pub address_7bit: u8,
    /// Transport-specific locator (PIIX4 port, WMI slave argument, OpenRGB device index…).
    #[serde(alias = "wmi_slave_arg")]
    pub bus_arg: u16,
    pub firmware: String,
    pub is_v2: bool,
    pub led_count: u8,
    pub current_mode: u8,
    /// Tag of the backend that found the module and must drive it (see `RamRgbBackend`).
    #[serde(default = "default_backend")]
    pub backend: String,
}

fn default_backend() -> String {
    "asus_wmi".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EneRamMode {
    Off = 0,
    Static = 1,
    Breathing = 2,
    Flashing = 3,
    SpectrumCycle = 4,
    Rainbow = 5,
    SpectrumCycleBreathing = 6,
    ChaseFade = 7,
    SpectrumCycleChaseFade = 8,
    Chase = 9,
    SpectrumCycleChase = 10,
    Wave = 11,
    ChaseRainbowPulse = 12,
    RandomFlicker = 13,
}

impl EneRamMode {
    pub const ALL: &'static [Self] = &[
        Self::Static,
        Self::Rainbow,
        Self::SpectrumCycle,
        Self::Breathing,
        Self::Wave,
        Self::ChaseFade,
        Self::SpectrumCycleChaseFade,
        Self::Chase,
        Self::SpectrumCycleBreathing,
        Self::ChaseRainbowPulse,
        Self::Flashing,
        Self::RandomFlicker,
        Self::Off,
    ];

    pub fn from_str_id(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "off" => Self::Off,
            "static" => Self::Static,
            "breathing" => Self::Breathing,
            "flashing" => Self::Flashing,
            "spectrum" | "spectrumcycle" | "spectrum_cycle" => Self::SpectrumCycle,
            "rainbow" => Self::Rainbow,
            "rainbowbreathing" | "spectrumcyclebreathing" => Self::SpectrumCycleBreathing,
            "chasefade" | "chase_fade" => Self::ChaseFade,
            "spectrumcyclechasefade" => Self::SpectrumCycleChaseFade,
            "chase" => Self::Chase,
            "spectrumcyclechase" => Self::SpectrumCycleChase,
            "wave" => Self::Wave,
            "chaserainbowpulse" => Self::ChaseRainbowPulse,
            "randomflicker" | "flicker" => Self::RandomFlicker,
            _ => Self::Static,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Static => "Static",
            Self::Breathing => "Breathing",
            Self::Flashing => "Flashing",
            Self::SpectrumCycle => "SpectrumCycle",
            Self::Rainbow => "Rainbow",
            Self::SpectrumCycleBreathing => "SpectrumCycleBreathing",
            Self::ChaseFade => "ChaseFade",
            Self::SpectrumCycleChaseFade => "SpectrumCycleChaseFade",
            Self::Chase => "Chase",
            Self::SpectrumCycleChase => "SpectrumCycleChase",
            Self::Wave => "Wave",
            Self::ChaseRainbowPulse => "ChaseRainbowPulse",
            Self::RandomFlicker => "RandomFlicker",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Éteint",
            Self::Static => "Statique",
            Self::Breathing => "Respiration",
            Self::Flashing => "Flash",
            Self::SpectrumCycle => "Cycle spectral",
            Self::Rainbow => "Arc-en-ciel",
            Self::SpectrumCycleBreathing => "Resp. Arc-en-ciel",
            Self::ChaseFade => "Poursuite Fondu",
            Self::SpectrumCycleChaseFade => "Poursuite RGB",
            Self::Chase => "Poursuite",
            Self::SpectrumCycleChase => "Comète RGB",
            Self::Wave => "Vague RGB",
            Self::ChaseRainbowPulse => "Impulsion RGB",
            Self::RandomFlicker => "Scintillement",
        }
    }

    pub fn uses_color(self) -> bool {
        matches!(
            self,
            Self::Static | Self::Breathing | Self::Flashing | Self::ChaseFade | Self::Chase
        )
    }
}

/// Hardware effect requested for RAM modules.
#[derive(Debug, Clone, Copy)]
pub struct RamEffect {
    pub mode: EneRamMode,
    pub color: Rgb,
    /// 0 (fastest) ..= 4.
    pub speed: u8,
    pub reverse: bool,
    /// Also store the effect in the module EEPROM (survives power cycles).
    pub save_eeprom: bool,
}

/// Addresses ENE controllers answer on (after [`remap_from_0x77`]).
pub const ENE_RAM_ADDRESSES: &[u8] = &[
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x66, 0x67, 0x39, 0x3A, 0x3B, 0x3C, 0x3D,
];

const REG_FIRMWARE: u16 = 0x1000;
const REG_VENDOR: u16 = 0x1030;
const REG_LED_COUNT: u16 = 0x1C02;
const REG_DIRECT: u16 = 0x8020;
const REG_MODE: u16 = 0x8021;
const REG_SPEED: u16 = 0x8022;
const REG_DIRECTION: u16 = 0x8023;
const REG_APPLY: u16 = 0x80A0;
const REG_SLOT_INDEX: u16 = 0x80F8;
const REG_I2C_ADDRESS: u16 = 0x80F9;
const COLORS_V1: u16 = 0x8010;
const COLORS_V2: u16 = 0x8160;
const APPLY_NOW: u8 = 0x01;
const APPLY_SAVE: u8 = 0xAA;
/// Firmware of first-generation controllers, which keep their effect colours at `0x8010`.
const V1_FIRMWARES: &[&str] = &[
    "LED-0116",
    "DIMM_LED-0102",
    "DIMM_LED-0103",
    "AUMA0-E8K4-0101",
];

fn read_reg(bus: &(impl SmbusBus + ?Sized), addr: u8, reg: u16) -> Option<u8> {
    bus.write_word_data(addr, 0x00, reg.swap_bytes()).ok()?;
    bus.read_byte_data(addr, 0x81)
}

fn write_reg(bus: &(impl SmbusBus + ?Sized), addr: u8, reg: u16, val: u8) -> Result<()> {
    bus.write_word_data(addr, 0x00, reg.swap_bytes())?;
    bus.write_byte_data(addr, 0x01, val)
}

/// Read-only ENE fingerprint check.
pub fn fingerprint_ok(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    (0u8..16).all(|i| bus.read_byte_data(addr, 0xA0 + i) == Some(i))
}

fn is_micron(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    let mut vendor = [0u8; 6];
    for (i, b) in vendor.iter_mut().enumerate() {
        *b = read_reg(bus, addr, REG_VENDOR + i as u16).unwrap_or(0);
    }
    &vendor == b"Micron"
}

/// A controller that is safe to write to.
fn is_writable_ene(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    fingerprint_ok(bus, addr) && !is_micron(bus, addr)
}

/// Fingerprint check limited to the addresses ENE controllers use (diagnostics).
pub fn looks_like_ene(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    ENE_RAM_ADDRESSES.contains(&addr) && fingerprint_ok(bus, addr)
}

/// Identify the controller at `addr`; `None` if it is not a writable ENE controller.
pub fn read_module(
    bus: &(impl SmbusBus + ?Sized),
    addr: u8,
    bus_arg: u16,
    backend: &str,
) -> Option<EneRamModule> {
    if !is_writable_ene(bus, addr) {
        return None;
    }
    let mut name = Vec::with_capacity(16);
    for i in 0..16u16 {
        match read_reg(bus, addr, REG_FIRMWARE + i) {
            Some(b) if (32..=126).contains(&b) => name.push(b),
            _ => break,
        }
    }
    let firmware = String::from_utf8_lossy(&name).trim().to_string();
    if firmware.len() < 4 {
        return None;
    }
    let led_count = read_reg(bus, addr, REG_LED_COUNT)
        .filter(|n| (1..=20).contains(n))
        .unwrap_or(8);
    let current_mode = read_reg(bus, addr, REG_MODE).unwrap_or(1);
    Some(EneRamModule {
        address_7bit: addr,
        bus_arg,
        is_v2: !V1_FIRMWARES.contains(&firmware.as_str()),
        firmware,
        led_count,
        current_mode,
        backend: backend.to_string(),
    })
}

/// Every writable ENE controller among `addresses`.
pub fn scan(
    bus: &(impl SmbusBus + ?Sized),
    addresses: &[u8],
    bus_arg: u16,
    backend: &str,
) -> Vec<EneRamModule> {
    addresses
        .iter()
        .filter_map(|&a| read_module(bus, a, bus_arg, backend))
        .collect()
}

fn address_free(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    bus.receive_byte(addr).is_none() && bus.read_byte_data(addr, 0x00).is_none()
}

/// Unconfigured controllers all answer on `0x77`: move each one to a free address of
/// [`ENE_RAM_ADDRESSES`] so they can be driven individually.
pub fn remap_from_0x77(bus: &(impl SmbusBus + ?Sized)) {
    let mut next = 0usize;
    for slot in 0u8..8 {
        if address_free(bus, 0x77) || !is_writable_ene(bus, 0x77) {
            break;
        }
        while next < ENE_RAM_ADDRESSES.len() {
            let cand = ENE_RAM_ADDRESSES[next];
            if cand != 0x77 && address_free(bus, cand) {
                break;
            }
            next += 1;
        }
        let Some(&target) = ENE_RAM_ADDRESSES.get(next) else {
            break;
        };
        next += 1;
        let _ = write_reg(bus, 0x77, REG_SLOT_INDEX, slot);
        let _ = write_reg(bus, 0x77, REG_I2C_ADDRESS, target << 1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Apply `effect` to one module. Returns `Ok(false)` (nothing written) when the controller no
/// longer passes the safety checks.
pub fn apply(
    bus: &(impl SmbusBus + ?Sized),
    module: &EneRamModule,
    effect: &RamEffect,
) -> Result<bool> {
    let addr = module.address_7bit;
    if !is_writable_ene(bus, addr) {
        return Ok(false);
    }
    let colors = if module.is_v2 { COLORS_V2 } else { COLORS_V1 };
    write_reg(bus, addr, REG_DIRECT, 0)?;
    for i in 0..module.led_count.clamp(1, 16) as u16 {
        let base = colors + i * 3;
        // ENE stores colours as R, B, G.
        write_reg(bus, addr, base, effect.color.r)?;
        write_reg(bus, addr, base + 1, effect.color.b)?;
        write_reg(bus, addr, base + 2, effect.color.g)?;
    }
    write_reg(bus, addr, REG_MODE, effect.mode as u8)?;
    write_reg(bus, addr, REG_SPEED, effect.speed.min(4))?;
    write_reg(bus, addr, REG_DIRECTION, u8::from(effect.reverse))?;
    write_reg(bus, addr, REG_APPLY, APPLY_NOW)?;
    if effect.save_eeprom {
        write_reg(bus, addr, REG_APPLY, APPLY_SAVE)?;
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Ok(true)
}

/// Switch one module to a raw ENE mode (used to turn the lights off and restore them).
/// Returns `Ok(false)` when the controller does not pass the safety checks.
pub fn set_mode(bus: &(impl SmbusBus + ?Sized), module: &EneRamModule, mode: u8) -> Result<bool> {
    let addr = module.address_7bit;
    if !is_writable_ene(bus, addr) {
        return Ok(false);
    }
    write_reg(bus, addr, REG_DIRECT, 0)?;
    write_reg(bus, addr, REG_MODE, mode)?;
    write_reg(bus, addr, REG_APPLY, APPLY_NOW)?;
    Ok(true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// In-memory ENE controller(s): answers the fingerprint, exposes registers through the
    /// `0x00` (address) / `0x01` (write) / `0x81` (read) indirection and logs every write.
    #[derive(Default)]
    pub(crate) struct FakeEneBus {
        pub regs: RefCell<HashMap<(u8, u16), u8>>,
        pointer: RefCell<HashMap<u8, u16>>,
        pub present: Vec<u8>,
        pub writes: RefCell<Vec<(u8, u16, u8)>>,
    }

    impl FakeEneBus {
        pub(crate) fn with_module(addr: u8, firmware: &str, leds: u8) -> Self {
            let bus = Self {
                present: vec![addr],
                ..Default::default()
            };
            bus.load(addr, firmware, leds);
            bus
        }

        pub(crate) fn load(&self, addr: u8, firmware: &str, leds: u8) {
            let mut regs = self.regs.borrow_mut();
            for i in 0..16 {
                regs.remove(&(addr, REG_FIRMWARE + i));
            }
            for (i, b) in firmware.bytes().enumerate() {
                regs.insert((addr, REG_FIRMWARE + i as u16), b);
            }
            regs.insert((addr, REG_LED_COUNT), leds);
            regs.insert((addr, REG_MODE), 1);
        }
    }

    impl SmbusBus for FakeEneBus {
        fn read_byte_data(&self, addr7: u8, cmd: u8) -> Option<u8> {
            if !self.present.contains(&addr7) {
                return None;
            }
            match cmd {
                0xA0..=0xAF => Some(cmd - 0xA0),
                0x81 => {
                    let reg = *self.pointer.borrow().get(&addr7)?;
                    Some(*self.regs.borrow().get(&(addr7, reg)).unwrap_or(&0))
                }
                _ => Some(0),
            }
        }

        fn write_byte_data(&self, addr7: u8, cmd: u8, val: u8) -> Result<()> {
            anyhow::ensure!(self.present.contains(&addr7), "no device");
            anyhow::ensure!(cmd == 0x01, "unexpected command {cmd:#x}");
            let reg = *self.pointer.borrow().get(&addr7).expect("pointer set");
            self.regs.borrow_mut().insert((addr7, reg), val);
            self.writes.borrow_mut().push((addr7, reg, val));
            Ok(())
        }

        fn write_word_data(&self, addr7: u8, cmd: u8, word: u16) -> Result<()> {
            anyhow::ensure!(self.present.contains(&addr7), "no device");
            anyhow::ensure!(cmd == 0x00, "unexpected command {cmd:#x}");
            self.pointer.borrow_mut().insert(addr7, word.swap_bytes());
            Ok(())
        }
    }

    fn effect(save: bool) -> RamEffect {
        RamEffect {
            mode: EneRamMode::Breathing,
            color: Rgb::new(0x11, 0x22, 0x33),
            speed: 9,
            reverse: true,
            save_eeprom: save,
        }
    }

    #[test]
    fn scan_reads_firmware_leds_and_generation() {
        let bus = FakeEneBus::with_module(0x71, "AUDA0-E6K5-0101", 5);
        let found = scan(&bus, ENE_RAM_ADDRESSES, 2, "test");
        assert_eq!(found.len(), 1);
        let m = &found[0];
        assert_eq!(
            (m.address_7bit, m.bus_arg, m.led_count, m.current_mode),
            (0x71, 2, 5, 1)
        );
        assert!(m.is_v2);
        assert_eq!(m.backend, "test");
        bus.load(0x71, "LED-0116", 5);
        assert!(!read_module(&bus, 0x71, 0, "t").unwrap().is_v2);
    }

    #[test]
    fn apply_writes_exactly_the_historical_register_sequence() {
        let bus = FakeEneBus::with_module(0x72, "AUDA0-E6K5-0101", 2);
        let m = read_module(&bus, 0x72, 0, "t").unwrap();
        assert!(apply(&bus, &m, &effect(true)).unwrap());
        let expected = vec![
            (0x72, 0x8020, 0),
            (0x72, 0x8160, 0x11),
            (0x72, 0x8161, 0x33),
            (0x72, 0x8162, 0x22),
            (0x72, 0x8163, 0x11),
            (0x72, 0x8164, 0x33),
            (0x72, 0x8165, 0x22),
            (0x72, 0x8021, EneRamMode::Breathing as u8),
            (0x72, 0x8022, 4),
            (0x72, 0x8023, 1),
            (0x72, 0x80A0, 0x01),
            (0x72, 0x80A0, 0xAA),
        ];
        assert_eq!(*bus.writes.borrow(), expected);

        bus.writes.borrow_mut().clear();
        assert!(set_mode(&bus, &m, 0).unwrap());
        assert_eq!(
            *bus.writes.borrow(),
            vec![(0x72, 0x8020, 0), (0x72, 0x8021, 0), (0x72, 0x80A0, 0x01)]
        );
    }

    #[test]
    fn nothing_is_written_to_unverified_or_micron_modules() {
        let bus = FakeEneBus::with_module(0x73, "AUDA0-E6K5-0101", 8);
        let m = read_module(&bus, 0x73, 0, "t").unwrap();
        for (i, b) in b"Micron".iter().enumerate() {
            bus.regs
                .borrow_mut()
                .insert((0x73, REG_VENDOR + i as u16), *b);
        }
        assert!(read_module(&bus, 0x73, 0, "t").is_none());
        assert!(!apply(&bus, &m, &effect(false)).unwrap());
        assert!(!set_mode(&bus, &m, 0).unwrap());
        assert!(bus.writes.borrow().is_empty());

        let empty = FakeEneBus::default();
        assert!(!apply(&empty, &m, &effect(false)).unwrap());
        assert!(scan(&empty, ENE_RAM_ADDRESSES, 0, "t").is_empty());
    }
}
