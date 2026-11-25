//! Safe DDR5/DDR4 RAM RGB controller via PawnIO SMBus, ASUS `ASUSManagement` COM WMI, or OpenRGB SDK.

use crate::pawnio::{
    PawnIoAvailability, apply_pawnio_ene_ram, pawnio_availability, probe_pawnio_ene_ram,
    set_pawnio_ene_modes,
};
use anyhow::{Context, Result};
use opengear_core::Rgb;
use opengear_platform::is_elevated;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalDimmInfo {
    pub slot: String,
    pub manufacturer: String,
    pub part_number: String,
    pub capacity_gb: u32,
    pub speed_mts: u32,
    pub memory_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EneRamModule {
    pub address_7bit: u8,
    pub wmi_slave_arg: u16,
    pub firmware: String,
    pub is_v2: bool,
    pub led_count: u8,
    pub current_mode: u8,
    #[serde(default = "default_backend_wmi")]
    pub backend: String,
}

fn default_backend_wmi() -> String {
    "asus_wmi".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamRgbStatus {
    pub is_admin: bool,
    pub wmi_smbus_available: bool,
    #[serde(default)]
    pub pawnio_status: String,
    pub physical_dimms: Vec<PhysicalDimmInfo>,
    pub ene_modules: Vec<EneRamModule>,
    pub message: String,
    pub last_mode: String,
    pub last_color: String,
    pub last_speed: u8,
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

/// Read installed physical RAM sticks via native COM `Win32_PhysicalMemory` (no Admin required).
pub fn read_physical_dimms() -> Vec<PhysicalDimmInfo> {
    #[cfg(windows)]
    {
        let Ok(conn) = opengear_platform::wmi::WmiConn::connect(r"root\cimv2") else {
            return Vec::new();
        };
        let Ok(rows) = conn.query(
            "SELECT DeviceLocator, Manufacturer, PartNumber, Capacity, ConfiguredClockSpeed, SMBIOSMemoryType FROM Win32_PhysicalMemory",
        ) else {
            return Vec::new();
        };
        let mut dimms = Vec::new();
        for (idx, row) in rows.iter().enumerate() {
            let raw_slot = row
                .get("DeviceLocator")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let slot = if raw_slot.is_empty() || raw_slot == "DIMM 1" {
                format!("DIMM #{}", idx + 1)
            } else {
                raw_slot.to_string()
            };
            let cap_bytes = row.get("Capacity").and_then(|v| v.as_u64()).unwrap_or(0);
            let capacity_gb = ((cap_bytes as f64) / (1024.0 * 1024.0 * 1024.0)).round() as u32;
            let smbios_mt = row
                .get("SMBIOSMemoryType")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let memory_type = match smbios_mt {
                26 => "DDR4".to_string(),
                34 => "DDR5".to_string(),
                other => format!("Type-{other}"),
            };
            dimms.push(PhysicalDimmInfo {
                slot,
                manufacturer: row
                    .get("Manufacturer")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                part_number: row
                    .get("PartNumber")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                capacity_gb,
                speed_mts: row
                    .get("ConfiguredClockSpeed")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                memory_type,
            });
        }
        dimms
    }
    #[cfg(not(windows))]
    Vec::new()
}

#[cfg(windows)]
mod wmi_smbus {
    use super::*;
    use crate::pawnio::NamedMutexGuard;
    use opengear_platform::wmi::{WmiConn, WmiVal};

    fn read_smbus_byte(conn: &WmiConn, obj_path: &str, slave: u32, cmd: u32) -> Option<u8> {
        let res = conn
            .exec_method(
                obj_path,
                "ASUSManagement",
                "read_smbus_byte",
                &[("slave", WmiVal::U32(slave)), ("cmd", WmiVal::U32(cmd))],
            )
            .ok()?;
        let code = res.get("Result").and_then(|v| v.as_u64())?;
        if code != 0 && code != 1 {
            return None;
        }
        res.get("data")
            .and_then(|v| v.as_u64())
            .map(|d| (d & 0xFF) as u8)
    }

    fn write_smbus_byte(
        conn: &WmiConn,
        obj_path: &str,
        slave: u32,
        cmd: u32,
        data: u32,
    ) -> Result<()> {
        let res = conn.exec_method(
            obj_path,
            "ASUSManagement",
            "write_smbus_byte",
            &[
                ("slave", WmiVal::U32(slave)),
                ("cmd", WmiVal::U32(cmd)),
                ("data", WmiVal::U32(data)),
            ],
        )?;
        let code = res.get("Result").and_then(|v| v.as_u64()).unwrap_or(1);
        if code != 0 {
            anyhow::bail!("ASUSManagement write_smbus_byte failed (Result={code})");
        }
        Ok(())
    }

    fn write_smbus_word(
        conn: &WmiConn,
        obj_path: &str,
        slave: u32,
        cmd: u32,
        data: u32,
    ) -> Result<()> {
        let res = conn.exec_method(
            obj_path,
            "ASUSManagement",
            "write_smbus_word",
            &[
                ("slave", WmiVal::U32(slave)),
                ("cmd", WmiVal::U32(cmd)),
                ("data", WmiVal::U32(data)),
            ],
        )?;
        let code = res.get("Result").and_then(|v| v.as_u64()).unwrap_or(1);
        if code != 0 {
            anyhow::bail!("ASUSManagement write_smbus_word failed (Result={code})");
        }
        Ok(())
    }

    fn read_ene_reg(conn: &WmiConn, obj_path: &str, slave: u32, reg: u16) -> Option<u8> {
        write_smbus_word(conn, obj_path, slave, 0x00, reg.swap_bytes() as u32).ok()?;
        read_smbus_byte(conn, obj_path, slave, 0x81)
    }

    fn write_ene_reg(conn: &WmiConn, obj_path: &str, slave: u32, reg: u16, val: u8) -> Result<()> {
        write_smbus_word(conn, obj_path, slave, 0x00, reg.swap_bytes() as u32)?;
        write_smbus_byte(conn, obj_path, slave, 0x01, val as u32)?;
        Ok(())
    }

    fn test_ene_fingerprint(conn: &WmiConn, obj_path: &str, slave: u32) -> bool {
        for i in 0..16u32 {
            match read_smbus_byte(conn, obj_path, slave, 0xA0 + i) {
                Some(v) if v as u32 == i => {}
                _ => return false,
            }
        }
        true
    }

    pub(super) fn probe_wmi_ene() -> (bool, Vec<EneRamModule>) {
        let Some(_lock) = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500) else {
            return (false, Vec::new());
        };
        let Ok(conn) = WmiConn::connect(r"root\wmi") else {
            return (false, Vec::new());
        };
        let Ok(Some(obj_path)) = conn.first_instance_path("ASUSManagement") else {
            return (false, Vec::new());
        };

        let mut modules = Vec::new();
        for shift in [0u32, 1u32] {
            if !modules.is_empty() {
                break;
            }
            for addr7 in 0x70u8..=0x77u8 {
                let slave = (addr7 as u32) << shift;
                if !test_ene_fingerprint(&conn, &obj_path, slave) {
                    continue;
                }
                let mut mbytes = [0u8; 6];
                for (i, b) in mbytes.iter_mut().enumerate() {
                    *b = read_ene_reg(&conn, &obj_path, slave, 0x1030 + i as u16).unwrap_or(0);
                }
                if &mbytes == b"Micron" {
                    continue;
                }
                let mut nbytes = Vec::with_capacity(16);
                for i in 0..16u16 {
                    match read_ene_reg(&conn, &obj_path, slave, 0x1000 + i) {
                        Some(b) if (32..=126).contains(&b) => nbytes.push(b),
                        _ => break,
                    }
                }
                let fw = String::from_utf8_lossy(&nbytes).trim().to_string();
                if fw.len() < 4 {
                    continue;
                }
                let leds = read_ene_reg(&conn, &obj_path, slave, 0x1C02)
                    .filter(|&n| (1..=20).contains(&n))
                    .unwrap_or(8);
                let cur_mode = read_ene_reg(&conn, &obj_path, slave, 0x8021).unwrap_or(1);
                let is_v2 = !matches!(
                    fw.as_str(),
                    "LED-0116" | "DIMM_LED-0102" | "DIMM_LED-0103" | "AUMA0-E8K4-0101"
                );
                modules.push(EneRamModule {
                    address_7bit: addr7,
                    wmi_slave_arg: slave as u16,
                    firmware: fw,
                    is_v2,
                    led_count: leds,
                    current_mode: cur_mode,
                    backend: "asus_wmi".to_string(),
                });
            }
        }
        (true, modules)
    }

    pub(super) fn set_wmi_modes_only(wmi: &[(&EneRamModule, u8)]) -> Result<usize> {
        let _lock = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500)
            .context("Bus SMBus occupé par un autre logiciel")?;
        let conn = WmiConn::connect(r"root\wmi")?;
        let obj_path = conn
            .first_instance_path("ASUSManagement")?
            .context("ASUSManagement WMI introuvable")?;

        let mut applied = 0usize;
        for (m, mode) in wmi {
            let slave = m.wmi_slave_arg as u32;
            if !test_ene_fingerprint(&conn, &obj_path, slave) {
                continue;
            }
            write_ene_reg(&conn, &obj_path, slave, 0x8020, 0)?;
            write_ene_reg(&conn, &obj_path, slave, 0x8021, *mode)?;
            write_ene_reg(&conn, &obj_path, slave, 0x80A0, 1)?;
            applied += 1;
        }
        Ok(applied)
    }

    pub(super) fn apply_wmi_ram_rgb(
        targets: &[&EneRamModule],
        mode: EneRamMode,
        color: Rgb,
        speed: u8,
        reverse: bool,
        save_eeprom: bool,
    ) -> Result<String> {
        let _lock = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500)
            .context("Bus SMBus occupé par un autre logiciel")?;
        let conn = WmiConn::connect(r"root\wmi")?;
        let obj_path = conn
            .first_instance_path("ASUSManagement")?
            .context("ASUSManagement WMI introuvable")?;

        let ene_speed = speed.min(4);
        let ene_dir = if reverse { 1 } else { 0 };
        let mut applied = 0usize;
        for m in targets {
            let sl = m.wmi_slave_arg as u32;
            if !test_ene_fingerprint(&conn, &obj_path, sl) {
                continue;
            }
            applied += 1;
            let effect_reg: u16 = if m.is_v2 { 0x8160 } else { 0x8010 };
            let leds = m.led_count.clamp(1, 16) as u16;
            write_ene_reg(&conn, &obj_path, sl, 0x8020, 0)?;
            for i in 0..leds {
                let base = effect_reg + i * 3;
                write_ene_reg(&conn, &obj_path, sl, base, color.r)?;
                write_ene_reg(&conn, &obj_path, sl, base + 1, color.b)?;
                write_ene_reg(&conn, &obj_path, sl, base + 2, color.g)?;
            }
            write_ene_reg(&conn, &obj_path, sl, 0x8021, mode as u8)?;
            write_ene_reg(&conn, &obj_path, sl, 0x8022, ene_speed)?;
            write_ene_reg(&conn, &obj_path, sl, 0x8023, ene_dir)?;
            write_ene_reg(&conn, &obj_path, sl, 0x80A0, 1)?;
            if save_eeprom {
                write_ene_reg(&conn, &obj_path, sl, 0x80A0, 0xAA)?;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
        if applied == 0 {
            anyhow::bail!("Aucune barrette ENE vérifiée — aucune écriture effectuée");
        }
        Ok(format!("OK ({applied}/{} DIMM)", targets.len()))
    }
}

pub fn probe_ram_rgb() -> RamRgbStatus {
    let physical_dimms = read_physical_dimms();
    let admin = is_elevated();
    let pio = pawnio_availability();
    let pawnio_status = match pio {
        PawnIoAvailability::Ready => "ready",
        PawnIoAvailability::NeedsAdmin => "needs_admin",
        PawnIoAvailability::NotInstalled => "not_installed",
    }
    .to_string();

    if !admin {
        let openrgb_mods = probe_openrgb_sdk_ram();
        if !openrgb_mods.is_empty() {
            let msg = format!(
                "{} barrette(s) DDR RGB détectée(s) via OpenRGB SDK (127.0.0.1:6742, sans Admin !).",
                openrgb_mods.len()
            );
            return RamRgbStatus {
                is_admin: false,
                wmi_smbus_available: false,
                pawnio_status,
                physical_dimms,
                ene_modules: openrgb_mods,
                message: msg,
                last_mode: "Static".into(),
                last_color: "#00D2FF".into(),
                last_speed: 2,
            };
        }
        let msg = if pio == PawnIoAvailability::NeedsAdmin {
            "Pilote universel signé PawnIO détecté ! Relance en Administrateur (UAC) pour piloter ta RAM DDR4/DDR5 RGB et lire la température CPU Ryzen/Intel en natif.".into()
        } else {
            "Lance OpenGear en Administrateur (UAC) avec le pilote officiel PawnIO pour le SMBus universel AMD/Intel, OU lance OpenRGB (SDK port 6742) sans Admin.".into()
        };
        return RamRgbStatus {
            is_admin: false,
            wmi_smbus_available: false,
            pawnio_status,
            physical_dimms,
            ene_modules: Vec::new(),
            message: msg,
            last_mode: "Static".into(),
            last_color: "#00D2FF".into(),
            last_speed: 2,
        };
    }

    if pio == PawnIoAvailability::Ready {
        let pio_mods = probe_pawnio_ene_ram();
        if !pio_mods.is_empty() {
            let msg = format!(
                "{} barrette(s) DDR RGB détectée(s) en natif via PawnIO Universel ({}).",
                pio_mods.len(),
                pio_mods
                    .iter()
                    .map(|m| format!(
                        "0x{:02X} {} [{} LEDs, {}]",
                        m.address_7bit, m.firmware, m.led_count, m.backend
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return RamRgbStatus {
                is_admin: true,
                wmi_smbus_available: true,
                pawnio_status,
                physical_dimms,
                ene_modules: pio_mods,
                message: msg,
                last_mode: "Static".into(),
                last_color: "#00D2FF".into(),
                last_speed: 2,
            };
        }
    }

    #[cfg(windows)]
    {
        let (wmi_ok, mut modules) = wmi_smbus::probe_wmi_ene();
        if modules.is_empty() {
            modules.extend(probe_openrgb_sdk_ram());
        }
        let message = if !modules.is_empty() {
            format!(
                "{} barrette(s) DDR5 RGB détectée(s) ({}).",
                modules.len(),
                modules
                    .iter()
                    .map(|m| format!(
                        "0x{:02X} {} [{} LEDs, {}]",
                        m.address_7bit, m.firmware, m.led_count, m.backend
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if !wmi_ok {
            "Admin actif, mais ASUSManagement WMI est indisponible sur cette carte mère.".into()
        } else {
            "ASUSManagement WMI actif, aucun contrôleur ENE DRAM n'a répondu sur 0x70..0x77. Tu peux aussi utiliser OpenRGB avec le serveur SDK (port 6742).".into()
        };

        RamRgbStatus {
            is_admin: true,
            wmi_smbus_available: wmi_ok,
            pawnio_status,
            physical_dimms,
            ene_modules: modules,
            message,
            last_mode: "Static".into(),
            last_color: "#00D2FF".into(),
            last_speed: 2,
        }
    }

    #[cfg(not(windows))]
    RamRgbStatus {
        is_admin: false,
        wmi_smbus_available: false,
        pawnio_status,
        physical_dimms,
        ene_modules: Vec::new(),
        message: "Non supporté hors Windows.".into(),
        last_mode: "Static".into(),
        last_color: "#00D2FF".into(),
        last_speed: 2,
    }
}

pub(crate) fn partition_ram_targets<'a>(
    targets: &[&'a EneRamModule],
) -> (
    Vec<&'a EneRamModule>,
    Vec<&'a EneRamModule>,
    Vec<&'a EneRamModule>,
) {
    let mut pawnio = Vec::new();
    let mut openrgb = Vec::new();
    let mut wmi = Vec::new();
    for &m in targets {
        if m.backend == "pawnio_piix4" || m.backend == "pawnio_i801" {
            pawnio.push(m);
        } else if m.backend == "openrgb_sdk" {
            openrgb.push(m);
        } else {
            wmi.push(m);
        }
    }
    (pawnio, openrgb, wmi)
}

pub fn set_ram_modes_only(modes: &[(&EneRamModule, u8)]) -> Result<usize> {
    let pawnio: Vec<(&EneRamModule, u8)> = modes
        .iter()
        .filter(|(m, _)| m.backend == "pawnio_piix4" || m.backend == "pawnio_i801")
        .copied()
        .collect();
    let wmi: Vec<(&EneRamModule, u8)> = modes
        .iter()
        .filter(|(m, _)| !m.backend.starts_with("pawnio") && m.backend != "openrgb_sdk")
        .copied()
        .collect();
    let mut applied = 0usize;
    if !pawnio.is_empty() {
        applied += set_pawnio_ene_modes(&pawnio)?;
    }
    if wmi.is_empty() {
        return Ok(applied);
    }
    if !is_elevated() {
        anyhow::bail!("Droits administrateur requis pour piloter la RAM via ASUSManagement WMI");
    }
    #[cfg(windows)]
    {
        applied += wmi_smbus::set_wmi_modes_only(&wmi)?;
    }
    Ok(applied)
}

pub fn apply_ram_rgb(
    modules: &[EneRamModule],
    target_addr_7bit: Option<u8>,
    mode: EneRamMode,
    color: Rgb,
    speed: u8,
    reverse: bool,
    save_eeprom: bool,
) -> Result<String> {
    let targets: Vec<&EneRamModule> = modules
        .iter()
        .filter(|m| {
            target_addr_7bit
                .map(|a| a == m.address_7bit)
                .unwrap_or(true)
        })
        .collect();
    if targets.is_empty() {
        anyhow::bail!(
            "Aucune barrette RGB détectée à piloter (clique d'abord sur Scanner la RAM)."
        );
    }

    let (pawnio, openrgb, wmi) = partition_ram_targets(&targets);
    let mut results = Vec::new();

    if !pawnio.is_empty() {
        results.push(apply_pawnio_ene_ram(
            &pawnio,
            mode,
            color,
            speed,
            reverse,
            save_eeprom,
        )?);
    }

    if !openrgb.is_empty() {
        let mut applied = 0usize;
        for m in &openrgb {
            apply_openrgb_sdk_ram(m.address_7bit as u32, m.led_count as usize, mode, color)?;
            applied += 1;
        }
        results.push(format!("OK OpenRGB SDK ({applied} DIMM)"));
    }

    if !wmi.is_empty() {
        if !is_elevated() {
            anyhow::bail!(
                "Relance OpenGear en Administrateur pour contrôler le RGB de la RAM DDR5 via ASUSManagement WMI."
            );
        }
        #[cfg(windows)]
        {
            results.push(wmi_smbus::apply_wmi_ram_rgb(
                &wmi,
                mode,
                color,
                speed,
                reverse,
                save_eeprom,
            )?);
        }
        #[cfg(not(windows))]
        anyhow::bail!("Non supporté hors Windows");
    }

    Ok(results.join(" + "))
}

// ================= PURE-RUST OPENRGB SDK TCP CLIENT (127.0.0.1:6742, ZERO DRIVERS) =================

const MAX_OPENRGB_PACKET_LEN: u32 = 262_144;

pub(crate) fn parse_openrgb_header(hdr: &[u8; 16]) -> std::io::Result<(u32, u32, usize)> {
    if &hdr[0..4] != b"ORGB" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid ORGB magic",
        ));
    }
    let dev_idx = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
    let pkt_id = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
    let len = u32::from_le_bytes(hdr[12..16].try_into().unwrap());
    if len > MAX_OPENRGB_PACKET_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Oversized OpenRGB packet ({len} bytes)"),
        ));
    }
    Ok((dev_idx, pkt_id, len as usize))
}

fn openrgb_send_packet(
    stream: &mut std::net::TcpStream,
    dev_idx: u32,
    pkt_id: u32,
    payload: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    let mut hdr = [0u8; 16];
    hdr[0..4].copy_from_slice(b"ORGB");
    hdr[4..8].copy_from_slice(&dev_idx.to_le_bytes());
    hdr[8..12].copy_from_slice(&pkt_id.to_le_bytes());
    hdr[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    stream.write_all(&hdr)?;
    if !payload.is_empty() {
        stream.write_all(payload)?;
    }
    Ok(())
}

fn openrgb_recv_packet(stream: &mut std::net::TcpStream) -> std::io::Result<(u32, u32, Vec<u8>)> {
    use std::io::Read;
    let mut hdr = [0u8; 16];
    stream.read_exact(&mut hdr)?;
    let (dev_idx, pkt_id, len) = parse_openrgb_header(&hdr)?;
    let mut buf = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut buf)?;
    }
    Ok((dev_idx, pkt_id, buf))
}

fn openrgb_connect() -> Option<std::net::TcpStream> {
    let addr: std::net::SocketAddr = "127.0.0.1:6742".parse().ok()?;
    let mut stream =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(120)).ok()?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(250)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(250)));
    let _ = openrgb_send_packet(&mut stream, 0, 50, b"OpenGear\0");
    Some(stream)
}

fn read_u16_le(buf: &[u8], off: &mut usize) -> Option<u16> {
    let v = u16::from_le_bytes(buf.get(*off..*off + 2)?.try_into().ok()?);
    *off += 2;
    Some(v)
}

fn read_u32_le(buf: &[u8], off: &mut usize) -> Option<u32> {
    let v = u32::from_le_bytes(buf.get(*off..*off + 4)?.try_into().ok()?);
    *off += 4;
    Some(v)
}

fn read_orgb_str(buf: &[u8], off: &mut usize) -> Option<String> {
    let len = read_u16_le(buf, off)? as usize;
    let s = buf.get(*off..*off + len)?;
    *off += len;
    Some(
        String::from_utf8_lossy(s)
            .trim_end_matches('\0')
            .to_string(),
    )
}

fn parse_openrgb_dram_leds(data: &[u8], off: &mut usize) -> Option<u8> {
    // Skip num_modes (u16) + active_mode (i32) + modes...
    let num_modes = read_u16_le(data, off)? as usize;
    let _active_mode = read_u32_le(data, off)?;
    for _ in 0..num_modes.min(64) {
        let _name = read_orgb_str(data, off)?;
        let _val = read_u32_le(data, off)?;
        let _flags = read_u32_le(data, off)?;
        let _speed_min = read_u32_le(data, off)?;
        let _speed_max = read_u32_le(data, off)?;
        let _b_min = read_u32_le(data, off)?;
        let _b_max = read_u32_le(data, off)?;
        let _colors_min = read_u32_le(data, off)?;
        let _colors_max = read_u32_le(data, off)?;
        let _speed = read_u32_le(data, off)?;
        let _brightness = read_u32_le(data, off)?;
        let _dir = read_u32_le(data, off)?;
        let _cmode = read_u32_le(data, off)?;
        let num_colors = read_u16_le(data, off)? as usize;
        *off = off.checked_add(num_colors * 4)?;
        if *off > data.len() {
            return None;
        }
    }
    let num_zones = read_u16_le(data, off)? as usize;
    for _ in 0..num_zones.min(32) {
        let _zname = read_orgb_str(data, off)?;
        let _ztype = read_u32_le(data, off)?;
        let _zmin = read_u32_le(data, off)?;
        let _zmax = read_u32_le(data, off)?;
        let _zcount = read_u32_le(data, off)?;
        let mat_len = read_u16_le(data, off)? as usize;
        *off = off.checked_add(mat_len)?;
        if *off > data.len() {
            return None;
        }
    }
    let num_leds = read_u16_le(data, off)? as usize;
    Some(num_leds.clamp(1, 32) as u8)
}

fn probe_openrgb_sdk_ram() -> Vec<EneRamModule> {
    let mut stream = match openrgb_connect() {
        Some(s) => s,
        None => return Vec::new(),
    };
    if openrgb_send_packet(&mut stream, 0, 0, &[]).is_err() {
        return Vec::new();
    }
    let (_, _, cnt_buf) = match openrgb_recv_packet(&mut stream) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    if cnt_buf.len() < 4 {
        return Vec::new();
    }
    let count = u32::from_le_bytes(cnt_buf[0..4].try_into().unwrap()).min(32);
    let mut out = Vec::new();
    for idx in 0..count {
        if openrgb_send_packet(&mut stream, idx, 1, &[]).is_err() {
            continue;
        }
        let (_, _, data) = match openrgb_recv_packet(&mut stream) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mut off = 4usize;
        let Some(dev_type) = read_u32_le(&data, &mut off) else {
            continue;
        };
        if dev_type != 1 {
            continue;
        }
        let name = read_orgb_str(&data, &mut off).unwrap_or_else(|| format!("DRAM #{idx}"));
        let _desc = read_orgb_str(&data, &mut off);
        let version = read_orgb_str(&data, &mut off).unwrap_or_default();
        let _serial = read_orgb_str(&data, &mut off);
        let _location = read_orgb_str(&data, &mut off);
        let led_count = parse_openrgb_dram_leds(&data, &mut off).unwrap_or(8);
        let fw = if !version.is_empty() {
            format!("{name} ({version})")
        } else {
            name
        };
        out.push(EneRamModule {
            address_7bit: idx as u8,
            wmi_slave_arg: idx as u16,
            firmware: fw,
            is_v2: true,
            led_count,
            current_mode: 1,
            backend: "openrgb_sdk".to_string(),
        });
    }
    out
}

fn apply_openrgb_sdk_ram(
    dev_idx: u32,
    led_count: usize,
    mode: EneRamMode,
    color: Rgb,
) -> Result<()> {
    let mut stream =
        openrgb_connect().context("Serveur OpenRGB SDK (127.0.0.1:6742) indisponible")?;
    // Switch controller to Direct / Custom mode (packet 1100) before sending UpdateLEDs (1050).
    let _ = openrgb_send_packet(&mut stream, dev_idx, 1100, &[]);

    let c = if mode == EneRamMode::Off {
        Rgb::BLACK
    } else {
        color
    };
    let n = led_count.clamp(1, 32);
    let mut payload = Vec::with_capacity(6 + n * 4);
    let data_size = (6 + n * 4) as u32;
    payload.extend_from_slice(&data_size.to_le_bytes());
    payload.extend_from_slice(&(n as u16).to_le_bytes());
    for _ in 0..n {
        payload.push(c.r);
        payload.push(c.g);
        payload.push(c.b);
        payload.push(0);
    }
    openrgb_send_packet(&mut stream, dev_idx, 1050, &payload)
        .context("Échec envoi couleurs OpenRGB SDK")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrgb_header_rejects_oversized_packet() {
        let mut hdr = [0u8; 16];
        hdr[0..4].copy_from_slice(b"ORGB");
        hdr[12..16].copy_from_slice(&(MAX_OPENRGB_PACKET_LEN + 1).to_le_bytes());
        assert!(parse_openrgb_header(&hdr).is_err());

        hdr[12..16].copy_from_slice(&128u32.to_le_bytes());
        assert_eq!(parse_openrgb_header(&hdr).unwrap(), (0, 0, 128));
    }

    #[test]
    fn mixed_ram_backends_are_partitioned_cleanly() {
        let m1 = EneRamModule {
            address_7bit: 0x71,
            wmi_slave_arg: 0,
            firmware: "AUDA0".into(),
            is_v2: true,
            led_count: 8,
            current_mode: 1,
            backend: "pawnio_piix4".into(),
        };
        let m2 = EneRamModule {
            address_7bit: 0,
            wmi_slave_arg: 0,
            firmware: "DRAM".into(),
            is_v2: true,
            led_count: 8,
            current_mode: 1,
            backend: "openrgb_sdk".into(),
        };
        let (p, o, w) = partition_ram_targets(&[&m1, &m2]);
        assert_eq!(p.len(), 1);
        assert_eq!(o.len(), 1);
        assert!(w.is_empty());
    }
}
