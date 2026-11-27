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
use tracing::{debug, info, warn};

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
const COLORS_DIRECT_V1: u16 = 0x8000;
const COLORS_V1: u16 = 0x8010;
const COLORS_DIRECT_V2: u16 = 0x8100;
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
    for attempt in 0..2 {
        if bus.write_word_data(addr, 0x00, reg.swap_bytes()).is_ok()
            && let Some(val) = bus.read_byte_data(addr, 0x81)
        {
            return Some(val);
        }
        if attempt == 0 {
            #[cfg(not(test))]
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    None
}

fn write_reg(bus: &(impl SmbusBus + ?Sized), addr: u8, reg: u16, val: u8) -> Result<()> {
    bus.write_word_data(addr, 0x00, reg.swap_bytes())?;
    bus.write_byte_data(addr, 0x01, val)
}

/// Read-only ENE fingerprint check (retries a single transient read miss per byte).
pub fn fingerprint_ok(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    (0u8..16).all(|i| {
        bus.read_byte_data(addr, 0xA0 + i)
            .or_else(|| bus.read_byte_data(addr, 0xA0 + i))
            == Some(i)
    })
}

fn is_micron(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    let mut vendor = [0u8; 6];
    for (i, b) in vendor.iter_mut().enumerate() {
        *b = read_reg(bus, addr, REG_VENDOR + i as u16).unwrap_or(0);
    }
    &vendor == b"Micron"
}

/// A controller that is safe to write to.
/// Retries briefly outside tests in case the shared SMBus is momentarily busy after a prior transfer.
fn is_writable_ene(bus: &(impl SmbusBus + ?Sized), addr: u8) -> bool {
    let fp = fingerprint_ok(bus, addr);
    if fp {
        if !is_micron(bus, addr) {
            return true;
        }
        debug!(
            addr = format_args!("0x{addr:02X}"),
            "Contrôleur ENE ignoré : constructeur Micron"
        );
        return false;
    }
    #[cfg(not(test))]
    for attempt in 1..=3 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        if fingerprint_ok(bus, addr) {
            if !is_micron(bus, addr) {
                debug!(
                    addr = format_args!("0x{addr:02X}"),
                    attempt, "Empreinte ENE récupérée après réessai SMBus"
                );
                return true;
            }
            return false;
        }
    }
    false
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
        if !address_free(bus, addr) {
            debug!(
                addr = format_args!("0x{addr:02X}"),
                bus_arg, backend, "Adresse SMBus non libre mais empreinte ENE absente ou Micron"
            );
        }
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
        warn!(
            addr = format_args!("0x{addr:02X}"),
            bus_arg,
            backend,
            raw_len = name.len(),
            firmware = %firmware,
            "Empreinte ENE valide mais nom de firmware trop court"
        );
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
    let found: Vec<EneRamModule> = addresses
        .iter()
        .filter_map(|&a| read_module(bus, a, bus_arg, backend))
        .collect();
    debug!(
        backend,
        bus_arg,
        count = found.len(),
        addrs = ?found
            .iter()
            .map(|m| format!("0x{:02X}({})", m.address_7bit, m.firmware))
            .collect::<Vec<_>>(),
        "Scan ENE terminé"
    );
    found
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
            warn!(slot, "Aucune adresse ENE libre pour remapper 0x77");
            break;
        };
        next += 1;
        info!(
            slot,
            target = format_args!("0x{target:02X}"),
            "Remappage d'un contrôleur ENE depuis 0x77"
        );
        if let Err(err) = write_reg(bus, 0x77, REG_SLOT_INDEX, slot) {
            warn!(slot, ?err, "Échec écriture REG_SLOT_INDEX sur 0x77");
        }
        if let Err(err) = write_reg(bus, 0x77, REG_I2C_ADDRESS, target << 1) {
            warn!(
                slot,
                target = format_args!("0x{target:02X}"),
                ?err,
                "Échec écriture REG_I2C_ADDRESS sur 0x77"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn write_effect_registers(
    bus: &(impl SmbusBus + ?Sized),
    module: &EneRamModule,
    effect: &RamEffect,
) -> Result<bool> {
    let addr = module.address_7bit;
    if !is_writable_ene(bus, addr) {
        warn!(
            addr = format_args!("0x{addr:02X}"),
            bus_arg = module.bus_arg,
            backend = %module.backend,
            "Écriture ENE annulée : le contrôleur ne répond plus à l'empreinte de sécurité"
        );
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
    Ok(true)
}

/// Apply `effect` to one module. Returns `Ok(false)` (nothing written) when the controller no
/// longer passes the safety checks.
pub fn apply(
    bus: &(impl SmbusBus + ?Sized),
    module: &EneRamModule,
    effect: &RamEffect,
) -> Result<bool> {
    if !write_effect_registers(bus, module, effect)? {
        return Ok(false);
    }
    if effect.save_eeprom {
        write_reg(bus, module.address_7bit, REG_APPLY, APPLY_SAVE)?;
        #[cfg(not(test))]
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(true)
}

/// Apply `effect` to every module on a shared bus segment cleanly:
/// Pass 1 programs and activates (`APPLY_NOW`) all DIMMs first so an EEPROM burn on DIMM #1
/// cannot interfere with programming DIMM #2; Pass 2 commits EEPROM (`0xAA`) per DIMM with a
/// full 100 ms burn window.
pub fn apply_all(
    bus: &(impl SmbusBus + ?Sized),
    modules: &[&EneRamModule],
    effect: &RamEffect,
) -> Result<usize> {
    let mut updated_addrs = Vec::with_capacity(modules.len());
    for module in modules {
        if write_effect_registers(bus, module, effect)? {
            debug!(
                addr = format_args!("0x{:02X}", module.address_7bit),
                mode = ?effect.mode,
                color = %effect.color.to_hex(),
                speed = effect.speed,
                reverse = effect.reverse,
                "Registres ENE appliqués (APPLY_NOW)"
            );
            updated_addrs.push(module.address_7bit);
        }
    }
    if effect.save_eeprom && !updated_addrs.is_empty() {
        #[cfg(not(test))]
        std::thread::sleep(std::time::Duration::from_millis(10));
        for &addr in &updated_addrs {
            write_reg(bus, addr, REG_APPLY, APPLY_SAVE)?;
            #[cfg(not(test))]
            std::thread::sleep(std::time::Duration::from_millis(100));
            let alive_mode = read_reg(bus, addr, REG_MODE);
            info!(
                addr = format_args!("0x{addr:02X}"),
                alive_mode = ?alive_mode,
                "Commit EEPROM ENE (0xAA) terminé"
            );
            if alive_mode.is_none() {
                warn!(
                    addr = format_args!("0x{addr:02X}"),
                    "Le contrôleur ENE ne répond plus immédiatement après le commit EEPROM 0xAA"
                );
            }
        }
    }
    Ok(updated_addrs.len())
}

/// Switch one module to a raw ENE mode (used to turn the lights off and restore them).
/// Returns `Ok(false)` when the controller does not pass the safety checks.
pub fn set_mode(bus: &(impl SmbusBus + ?Sized), module: &EneRamModule, mode: u8) -> Result<bool> {
    let addr = module.address_7bit;
    if !is_writable_ene(bus, addr) {
        warn!(
            addr = format_args!("0x{addr:02X}"),
            mode, "Changement mode ENE ignoré : empreinte absente"
        );
        return Ok(false);
    }
    write_reg(bus, addr, REG_DIRECT, 0)?;
    write_reg(bus, addr, REG_MODE, mode)?;
    write_reg(bus, addr, REG_APPLY, APPLY_NOW)?;
    Ok(true)
}

/// Write individual per-LED direct RGB values (`0x8000` on V1, `0x8100` on V2) onto one ENE
/// module. When `enter_direct` is true, enables `REG_DIRECT = 1` + `APPLY_NOW` before streaming.
/// Returns `Ok(false)` if the ENE fingerprint / non-Micron guard rejects the address.
pub fn apply_direct(
    bus: &(impl SmbusBus + ?Sized),
    module: &EneRamModule,
    colors: &[Rgb],
    enter_direct: bool,
) -> Result<bool> {
    let addr = module.address_7bit;
    if !is_writable_ene(bus, addr) {
        warn!(
            addr = format_args!("0x{addr:02X}"),
            bus_arg = module.bus_arg,
            backend = %module.backend,
            "Trame Direct ENE annulée : empreinte absente ou Micron"
        );
        return Ok(false);
    }
    if enter_direct {
        write_reg(bus, addr, REG_DIRECT, 1)?;
        write_reg(bus, addr, REG_APPLY, APPLY_NOW)?;
    }
    let base_reg = if module.is_v2 {
        COLORS_DIRECT_V2
    } else {
        COLORS_DIRECT_V1
    };
    let count = module.led_count.clamp(1, 16) as usize;
    let fallback = colors.last().copied().unwrap_or(Rgb::BLACK);
    for idx in 0..count {
        let c = colors.get(idx).copied().unwrap_or(fallback);
        let reg = base_reg + (idx as u16) * 3;
        write_reg(bus, addr, reg, c.r)?;
        write_reg(bus, addr, reg + 1, c.b)?;
        write_reg(bus, addr, reg + 2, c.g)?;
    }
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

    #[test]
    fn apply_all_programs_every_dimm_before_committing_eeprom() {
        let mut bus = FakeEneBus::with_module(0x70, "AUDA0-E6K5-0101", 1);
        bus.present.push(0x71);
        bus.load(0x71, "AUDA0-E6K5-0101", 1);
        let m0 = read_module(&bus, 0x70, 0, "t").unwrap();
        let m1 = read_module(&bus, 0x71, 0, "t").unwrap();

        let count = apply_all(&bus, &[&m0, &m1], &effect(true)).unwrap();
        assert_eq!(count, 2);

        let writes = bus.writes.borrow();
        let save0 = writes
            .iter()
            .position(|&w| w == (0x70, 0x80A0, APPLY_SAVE))
            .expect("DIMM 0 EEPROM commit");
        let save1 = writes
            .iter()
            .position(|&w| w == (0x71, 0x80A0, APPLY_SAVE))
            .expect("DIMM 1 EEPROM commit");
        let now1 = writes
            .iter()
            .position(|&w| w == (0x71, 0x80A0, APPLY_NOW))
            .expect("DIMM 1 live apply");
        assert!(now1 < save0 && save0 < save1);
    }

    #[test]
    fn fingerprint_ok_recovers_from_single_transient_read_drop() {
        struct FlakyBus {
            dropped_once: RefCell<bool>,
        }
        impl SmbusBus for FlakyBus {
            fn read_byte_data(&self, _addr7: u8, cmd: u8) -> Option<u8> {
                if cmd == 0xA7 && !*self.dropped_once.borrow() {
                    *self.dropped_once.borrow_mut() = true;
                    return None;
                }
                (0xA0..=0xAF).contains(&cmd).then_some(cmd - 0xA0)
            }
            fn write_byte_data(&self, _: u8, _: u8, _: u8) -> Result<()> {
                Ok(())
            }
            fn write_word_data(&self, _: u8, _: u8, _: u16) -> Result<()> {
                Ok(())
            }
        }
        let bus = FlakyBus {
            dropped_once: RefCell::new(false),
        };
        assert!(fingerprint_ok(&bus, 0x70));
        assert!(*bus.dropped_once.borrow());
    }

    #[test]
    fn apply_direct_writes_per_led_rbg_registers_on_v1_and_v2() {
        let mut bus = FakeEneBus::with_module(0x70, "DIMM_LED-0102", 2);
        bus.load(0x71, "AUDA0-E6K5-0101", 2);
        bus.present.push(0x71);
        let m_v1 = read_module(&bus, 0x70, 0, "fake").unwrap();
        let m_v2 = read_module(&bus, 0x71, 0, "fake").unwrap();

        let leds = [Rgb::new(0xAA, 0xBB, 0xCC), Rgb::new(0x10, 0x20, 0x30)];
        assert!(apply_direct(&bus, &m_v1, &leds, true).unwrap());
        assert!(apply_direct(&bus, &m_v2, &leds, false).unwrap());

        let w = bus.writes.borrow();
        assert!(w.contains(&(0x70, REG_DIRECT, 1)));
        assert!(w.contains(&(0x70, REG_APPLY, APPLY_NOW)));
        // V1 direct colors at 0x8000 (R, B, G order)
        assert!(w.contains(&(0x70, 0x8000, 0xAA)));
        assert!(w.contains(&(0x70, 0x8001, 0xCC)));
        assert!(w.contains(&(0x70, 0x8002, 0xBB)));
        assert!(w.contains(&(0x70, 0x8003, 0x10)));
        assert!(w.contains(&(0x70, 0x8004, 0x30)));
        assert!(w.contains(&(0x70, 0x8005, 0x20)));
        // V2 direct colors at 0x8100 without re-sending REG_DIRECT
        assert!(!w.iter().any(|&(a, r, _)| a == 0x71 && r == REG_DIRECT));
        assert!(w.contains(&(0x71, 0x8100, 0xAA)));
        assert!(w.contains(&(0x71, 0x8101, 0xCC)));
        assert!(w.contains(&(0x71, 0x8102, 0xBB)));
    }
}
