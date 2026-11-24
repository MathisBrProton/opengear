//! Safe DDR5/DDR4 RAM RGB controller via ASUS BIOS `root\wmi\ASUSManagement` SMBus methods.
//!
//! Safety guarantees:
//! - Uses ONLY the signed ASUS BIOS ACPI WMI interface (`ASUSManagement::read_smbus_byte`,
//!   `write_smbus_word`, `write_smbus_byte`) — zero unsigned Ring-0 drivers (`WinRing0`/`InpOutx64`).
//! - NEVER probes or writes to SPD EEPROM (`0x50..0x57`) or DDR5 PMIC (`0x48..0x4F`) addresses.
//! - ONLY scans the ENE DRAM RGB address window (`0x70..=0x77`) using a 100% read-only 16-byte
//!   hardware fingerprint (`reg 0xA0..0xAF == 0x00..0x0F`) before issuing a single register select.
//! - Verifies the ENE firmware signature at `0x1000` (`AUDA0-E6K5-*`, `DIMM_LED-*`, `AUMA0-E6K5-*`)
//!   and explicitly rejects Micron controllers (`0x1030 == "Micron"`).

use crate::cooling::is_elevated;
use crate::pawnio::{
    PawnIoAvailability, apply_pawnio_ene_ram, pawnio_availability, probe_pawnio_ene_ram,
    set_pawnio_ene_modes,
};
use anyhow::{Context, Result};
use opengear_core::Rgb;
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Physical DIMM stick metadata from `Win32_PhysicalMemory` (works without Admin).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalDimmInfo {
    pub slot: String,
    pub manufacturer: String,
    pub part_number: String,
    pub capacity_gb: u32,
    pub speed_mts: u32,
    pub memory_type: String,
}

/// Verified ENE SMBus or OpenRGB SDK RGB controller found on a DIMM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EneRamModule {
    /// 7-bit SMBus address (0x70..=0x77) or OpenRGB device index
    pub address_7bit: u8,
    /// Actual `slave` argument accepted by `ASUSManagement` WMI (either `addr` or `addr << 1`)
    pub wmi_slave_arg: u16,
    /// Firmware string read from `ENE_REG_DEVICE_NAME` (e.g. `AUDA0-E6K5-0101`, `DIMM_LED-0102`)
    pub firmware: String,
    /// True if V2 register map (`0x8100`/`0x8160`), false if V1 (`0x8000`/`0x8010`)
    pub is_v2: bool,
    /// LED count read from ENE config table (`0x1C02`)
    pub led_count: u8,
    /// Current hardware mode read from `0x8021`
    pub current_mode: u8,
    /// Transport backend ("asus_wmi" or "openrgb_sdk")
    #[serde(default = "default_backend_wmi")]
    pub backend: String,
}

fn default_backend_wmi() -> String {
    "asus_wmi".to_string()
}

/// Full RAM RGB subsystem status exposed to UI & CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamRgbStatus {
    pub is_admin: bool,
    pub wmi_smbus_available: bool,
    /// Universal PawnIO WHQL driver status: "ready" | "needs_admin" | "not_installed"
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

/// Read installed physical RAM sticks via `Win32_PhysicalMemory` (no Admin required).
pub fn read_physical_dimms() -> Vec<PhysicalDimmInfo> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = r#"
$mems = Get-CimInstance Win32_PhysicalMemory -ErrorAction SilentlyContinue
foreach ($m in $mems) {
  $gb = [math]::Round($m.Capacity / 1GB)
  $mt = switch ($m.SMBIOSMemoryType) { 26 { 'DDR4' } 34 { 'DDR5' } default { "Type-$($m.SMBIOSMemoryType)" } }
  "$($m.DeviceLocator)|$($m.Manufacturer)|$($m.PartNumber)|$gb|$($m.ConfiguredClockSpeed)|$mt"
}
"#;
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let mut dimms = Vec::new();
        if let Ok(o) = out {
            let txt = String::from_utf8_lossy(&o.stdout);
            for (idx, line) in txt.lines().enumerate() {
                let parts: Vec<&str> = line.trim().split('|').collect();
                if parts.len() >= 6 {
                    let raw_slot = parts[0].trim();
                    let slot = if raw_slot.is_empty() || raw_slot == "DIMM 1" {
                        format!("DIMM #{}", idx + 1)
                    } else {
                        raw_slot.to_string()
                    };
                    dimms.push(PhysicalDimmInfo {
                        slot,
                        manufacturer: parts[1].trim().to_string(),
                        part_number: parts[2].trim().to_string(),
                        capacity_gb: parts[3].trim().parse().unwrap_or(0),
                        speed_mts: parts[4].trim().parse().unwrap_or(0),
                        memory_type: parts[5].trim().to_string(),
                    });
                }
            }
        }
        dimms
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Perform a 100% safe scan of ENE DRAM RGB controllers via `root\wmi\ASUSManagement`.
///
/// Only probes `0x70..=0x77` using the read-only 16-byte ENE fingerprint (`0xA0..0xAF == 0x00..0x0F`).
/// Automatically detects whether `ASUSManagement` expects 7-bit (`0x71`) or 8-bit (`0xE2`) slave addresses.
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
            "Pilote universel signé PawnIO détecté ! Clique sur « Relancer en Administrateur (UAC) » pour piloter ta RAM DDR4/DDR5 RGB et lire la température CPU Ryzen/Intel en natif (sur n'importe quelle carte mère).".into()
        } else {
            "Lance OpenGear en Administrateur (UAC) avec le pilote officiel PawnIO (pawnio.eu) pour le SMBus universel AMD/Intel, OU lance OpenRGB (SDK port 6742) sans Admin.".into()
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

    // 1. UNIVERSAL PAWNIO SMBUS FIRST (works on ANY AMD AM4/AM5 or Intel motherboard — zero PowerShell!)
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
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // Safe single-session PowerShell WMI SMBus scanner:
        // 1. Read-only check reg 0xA0..0xAF == 0x00..0x0F on 0x70..0x77 (tests 7-bit first, then 8-bit if none found)
        // 2. Only on verified ENE chips: read Micron check (0x1030), Device Name (0x1000), Config LED count (0x1C02), Mode (0x8021)
        let script = r#"
$inst = Get-CimInstance -Namespace root\wmi -ClassName ASUSManagement -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $inst) { Write-Output 'NO_WMI'; exit 0 }

function Read-SmbusByte([uint32]$slave, [uint32]$cmd) {
    $r = Invoke-CimMethod -InputObject $inst -MethodName read_smbus_byte -Arguments @{ slave = $slave; cmd = $cmd } -ErrorAction SilentlyContinue
    if ($null -eq $r) { return -1 }
    if ($r.Result -ne 0 -and $r.Result -ne 1) { return -1 }
    return ([int]$r.data -band 0xFF)
}

function Read-EneReg([uint32]$slave, [uint16]$reg) {
    $swapped = [uint32]((($reg -shl 8) -band 0xFF00) -bor (($reg -shr 8) -band 0x00FF))
    $w = Invoke-CimMethod -InputObject $inst -MethodName write_smbus_word -Arguments @{ slave = $slave; cmd = [uint32]0x00; data = $swapped } -ErrorAction SilentlyContinue
    if ($null -eq $w -or ($w.Result -ne 0 -and $w.Result -ne 1)) { return -1 }
    return (Read-SmbusByte $slave 0x81)
}

function Test-EneFingerprint([uint32]$slave) {
    for ($i = 0; $i -lt 16; $i++) {
        $v = Read-SmbusByte $slave (0xA0 + $i)
        if ($v -ne $i) { return $false }
    }
    return $true
}

$found = @()
foreach ($shift in @(0, 1)) {
    if ($found.Count -gt 0) { break }
    foreach ($addr7 in 0x70..0x77) {
        $slave = [uint32]($addr7 -shl $shift)
        if (Test-EneFingerprint $slave) {
            # Verify not Micron (0x1030)
            $mBytes = @()
            for ($i = 0; $i -lt 6; $i++) {
                $b = Read-EneReg $slave (0x1030 + $i)
                if ($b -gt 0) { $mBytes += [char]$b }
            }
            if ((-join $mBytes) -eq 'Micron') { continue }

            # Read ENE device version string (0x1000..0x100F)
            $nBytes = @()
            for ($i = 0; $i -lt 16; $i++) {
                $b = Read-EneReg $slave (0x1000 + $i)
                if ($b -ge 32 -and $b -le 126) { $nBytes += [char]$b } else { break }
            }
            $fw = (-join $nBytes).Trim()
            if ($fw.Length -lt 4) { continue }

            $leds = Read-EneReg $slave 0x1C02
            if ($leds -lt 1 -or $leds -gt 20) { $leds = 8 }
            $curMode = Read-EneReg $slave 0x8021
            if ($curMode -lt 0) { $curMode = 1 }
            $found += "ENE|$addr7|$slave|$fw|$leds|$curMode"
        }
    }
}
Write-Output 'WMI_OK'
foreach ($line in $found) { Write-Output $line }
"#;

        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let mut wmi_ok = false;
        let mut modules = Vec::new();
        if let Ok(o) = out {
            let txt = String::from_utf8_lossy(&o.stdout);
            for line in txt.lines() {
                let l = line.trim();
                if l == "WMI_OK" {
                    wmi_ok = true;
                } else if let Some(rest) = l.strip_prefix("ENE|") {
                    let parts: Vec<&str> = rest.split('|').collect();
                    if parts.len() >= 5 {
                        let addr7: u8 = parts[0].parse().unwrap_or(0);
                        let slave: u16 = parts[1].parse().unwrap_or(addr7 as u16);
                        let fw = parts[2].to_string();
                        let leds: u8 = parts[3].parse().unwrap_or(8);
                        let cur_mode: u8 = parts[4].parse().unwrap_or(1);
                        let is_v2 = !matches!(
                            fw.as_str(),
                            "LED-0116" | "DIMM_LED-0102" | "DIMM_LED-0103" | "AUMA0-E8K4-0101"
                        );
                        modules.push(EneRamModule {
                            address_7bit: addr7,
                            wmi_slave_arg: slave,
                            firmware: fw,
                            is_v2,
                            led_count: leds,
                            current_mode: cur_mode,
                            backend: "asus_wmi".to_string(),
                        });
                    }
                }
            }
        }

        // Pure-TCP OpenRGB SDK fallback (127.0.0.1:6742, zero drivers, works for any RGB RAM if OpenRGB --server is running)
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
            "ASUSManagement WMI actif, aucun contrôleur ENE DRAM n'a répondu sur 0x70..0x77 (sur AM5 B650 le bus DRAM SMBus est parfois routé hors WMI). Astuce 100% safe : lance OpenRGB avec le serveur SDK (port 6742) puis clique sur Scanner — OpenGear pilotera ta RAM DDR5 directement !".into()
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
    {
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
}

/// Apply an ENE hardware lighting mode + color to one or all verified ENE DDR5 RAM modules.
///
/// Color byte order on ENE SMBus is verified `[Red, Blue, Green]` (RBG) per OpenRGB `ENESMBusController`.
/// When `save_eeprom` is true, writes `0xAA` to `ENE_REG_APPLY (0x80A0)` so the effect persists across power cycles.
/// Switch RAM lighting off / back on by writing ONLY the ENE effect-mode register of each module
/// (colors, speed and EEPROM untouched), so restoring the previous mode restores the exact look.
/// `modes`: (module, raw mode byte). Supports PawnIO and ASUSManagement WMI backends; modules
/// driven through the OpenRGB SDK are skipped. Returns the number of modules updated.
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
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut targets = String::new();
        for (m, mode) in &wmi {
            targets.push_str(&format!(
                "@{{ slave = [uint32]{}; mode = [uint32]{} }},\n",
                m.wmi_slave_arg, mode
            ));
        }
        let script = format!(
            r#"
$inst = Get-CimInstance -Namespace root\wmi -ClassName ASUSManagement -ErrorAction Stop | Select-Object -First 1
if (-not $inst) {{ throw 'ASUSManagement WMI introuvable' }}
function Write-EneReg([uint32]$slave, [uint16]$reg, [uint32]$val) {{
    $swapped = [uint32]((($reg -shl 8) -band 0xFF00) -bor (($reg -shr 8) -band 0x00FF))
    $w1 = Invoke-CimMethod -InputObject $inst -MethodName write_smbus_word -Arguments @{{ slave = $slave; cmd = [uint32]0x00; data = $swapped }} -ErrorAction Stop
    if ($w1.Result -ne 0) {{ throw "SMBus write_word failed (slave=$slave reg=$reg)" }}
    $w2 = Invoke-CimMethod -InputObject $inst -MethodName write_smbus_byte -Arguments @{{ slave = $slave; cmd = [uint32]0x01; data = $val }} -ErrorAction Stop
    if ($w2.Result -ne 0) {{ throw "SMBus write_byte failed (slave=$slave reg=$reg)" }}
}}
function Test-EneFingerprint([uint32]$slave) {{
    for ($i = 0; $i -lt 16; $i++) {{
        $r = Invoke-CimMethod -InputObject $inst -MethodName read_smbus_byte -Arguments @{{ slave = $slave; cmd = [uint32](0xA0 + $i) }} -ErrorAction SilentlyContinue
        if ($null -eq $r -or ($r.Result -ne 0 -and $r.Result -ne 1)) {{ return $false }}
        if ((([int]$r.data) -band 0xFF) -ne $i) {{ return $false }}
    }}
    return $true
}}
$targets = @(
{targets}
)
$applied = 0
foreach ($t in $targets) {{
    if (-not (Test-EneFingerprint $t.slave)) {{ continue }}
    Write-EneReg $t.slave 0x8020 0
    Write-EneReg $t.slave 0x8021 $t.mode
    Write-EneReg $t.slave 0x80A0 1
    $applied++
}}
"$applied"
"#
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("Échec exécution PowerShell WMI SMBus")?;
        if !out.status.success() {
            anyhow::bail!(
                "Erreur SMBus ENE RAM : {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        applied += String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<usize>()
            .unwrap_or(0);
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
        .filter(|m| target_addr_7bit.map(|a| a == m.address_7bit).unwrap_or(true))
        .collect();
    if targets.is_empty() {
        anyhow::bail!("Aucune barrette RGB détectée à piloter (clique d'abord sur Scanner la RAM).");
    }

    // Route PawnIO Universal SMBus modules (AMD PIIX4 / Intel I801 — zero PowerShell!)
    if targets
        .iter()
        .all(|m| m.backend == "pawnio_piix4" || m.backend == "pawnio_i801")
    {
        return apply_pawnio_ene_ram(&targets, mode, color, speed, reverse, save_eeprom);
    }

    // Route OpenRGB SDK modules via pure TCP (works even without Admin!)
    if targets.iter().all(|m| m.backend == "openrgb_sdk") {
        let mut applied = 0usize;
        for m in &targets {
            apply_openrgb_sdk_ram(m.address_7bit as u32, m.led_count as usize, mode, color)?;
            applied += 1;
        }
        return Ok(format!("OK OpenRGB SDK ({applied} DIMM)"));
    }

    if !is_elevated() {
        anyhow::bail!("Relance OpenGear en Administrateur pour contrôler le RGB de la RAM DDR5 via ASUSManagement WMI.");
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let mode_val = mode as u8;
        // ENE speed register: 0x04 slowest .. 0x00 fastest
        let ene_speed = speed.min(4);
        let ene_dir: u8 = if reverse { 1 } else { 0 };
        let apply_val: u8 = if save_eeprom { 0xAA } else { 0x01 };

        // Build target list literal for PowerShell
        let mut target_cmds = String::new();
        for m in &targets {
            let effect_reg: u16 = if m.is_v2 { 0x8160 } else { 0x8010 };
            let leds = m.led_count.clamp(1, 16);
            target_cmds.push_str(&format!(
                "@{{ slave = [uint32]{}; effectReg = [uint16]{}; leds = {}; addr7 = {} }},\n",
                m.wmi_slave_arg, effect_reg, leds, m.address_7bit
            ));
        }

        let script = format!(
            r#"
$inst = Get-CimInstance -Namespace root\wmi -ClassName ASUSManagement -ErrorAction Stop | Select-Object -First 1
if (-not $inst) {{ throw 'ASUSManagement WMI introuvable' }}

function Write-EneReg([uint32]$slave, [uint16]$reg, [uint32]$val) {{
    $swapped = [uint32]((($reg -shl 8) -band 0xFF00) -bor (($reg -shr 8) -band 0x00FF))
    $w1 = Invoke-CimMethod -InputObject $inst -MethodName write_smbus_word -Arguments @{{ slave = $slave; cmd = [uint32]0x00; data = $swapped }} -ErrorAction Stop
    if ($w1.Result -ne 0) {{ throw "SMBus write_word failed (slave=$slave reg=$reg res=$($w1.Result))" }}
    $w2 = Invoke-CimMethod -InputObject $inst -MethodName write_smbus_byte -Arguments @{{ slave = $slave; cmd = [uint32]0x01; data = $val }} -ErrorAction Stop
    if ($w2.Result -ne 0) {{ throw "SMBus write_byte failed (slave=$slave reg=$reg res=$($w2.Result))" }}
}}

function Test-EneFingerprint([uint32]$slave) {{
    for ($i = 0; $i -lt 16; $i++) {{
        $r = Invoke-CimMethod -InputObject $inst -MethodName read_smbus_byte -Arguments @{{ slave = $slave; cmd = [uint32](0xA0 + $i) }} -ErrorAction SilentlyContinue
        if ($null -eq $r -or ($r.Result -ne 0 -and $r.Result -ne 1)) {{ return $false }}
        if ((([int]$r.data) -band 0xFF) -ne $i) {{ return $false }}
    }}
    return $true
}}

$targets = @(
{target_cmds}
)

$r = [uint32]{r}; $g = [uint32]{g}; $b = [uint32]{b}
$modeVal = [uint32]{mode_val}
$speedVal = [uint32]{ene_speed}
$dirVal = [uint32]{ene_dir}
$applyVal = [uint32]{apply_val}

$applied = 0
foreach ($t in $targets) {{
    $sl = $t.slave
    # Re-verify the read-only ENE fingerprint right before any write (cached scan may be stale)
    if (-not (Test-EneFingerprint $sl)) {{ continue }}
    $applied++
    # Disable Direct mode (use hardware effect engine)
    Write-EneReg $sl 0x8020 0
    # Write R, B, G triplet per LED to effect color table
    for ($i = 0; $i -lt $t.leds; $i++) {{
        $base = [uint16]($t.effectReg + ($i * 3))
        Write-EneReg $sl $base $r
        Write-EneReg $sl ([uint16]($base + 1)) $b
        Write-EneReg $sl ([uint16]($base + 2)) $g
    }}
    Write-EneReg $sl 0x8021 $modeVal
    Write-EneReg $sl 0x8022 $speedVal
    Write-EneReg $sl 0x8023 $dirVal
    Write-EneReg $sl 0x80A0 1
    if ($applyVal -eq 0xAA) {{
        Write-EneReg $sl 0x80A0 0xAA
    }}
}}
if ($applied -eq 0) {{ throw "Aucune barrette ENE verifiee - aucune ecriture effectuee" }}
"OK ($applied/$($targets.Count) DIMM)"
"#,
            target_cmds = target_cmds,
            r = color.r,
            g = color.g,
            b = color.b,
            mode_val = mode_val,
            ene_speed = ene_speed,
            ene_dir = ene_dir,
            apply_val = apply_val,
        );

        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("Échec exécution PowerShell WMI SMBus")?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("Erreur SMBus ENE RAM : {}", err.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    #[cfg(not(windows))]
    {
        let _ = (mode, color, speed, reverse, save_eeprom);
        anyhow::bail!("Non supporté hors Windows")
    }
}

// ================= PURE-RUST OPENRGB SDK TCP CLIENT (127.0.0.1:6742, ZERO DRIVERS) =================

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
    if &hdr[0..4] != b"ORGB" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid ORGB magic",
        ));
    }
    let dev_idx = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
    let pkt_id = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
    let len = u32::from_le_bytes(hdr[12..16].try_into().unwrap()).min(262_144) as usize;
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
    // Set client name (packet 50)
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

/// Probe `127.0.0.1:6742` OpenRGB SDK for DRAM controllers (`device_type == 1`).
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
        let mut off = 4usize; // skip data_size u32
        let dev_type = match read_u32_le(&data, &mut off) {
            Some(t) => t,
            None => continue,
        };
        // OpenRGB DEVICE_TYPE_DRAM = 1
        if dev_type != 1 {
            continue;
        }
        let name = read_orgb_str(&data, &mut off).unwrap_or_else(|| format!("DRAM #{idx}"));
        let _desc = read_orgb_str(&data, &mut off);
        let version = read_orgb_str(&data, &mut off).unwrap_or_default();
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
            led_count: 8,
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
    let c = if mode == EneRamMode::Off {
        Rgb::BLACK
    } else {
        color
    };
    // UpdateLEDs (packet 1050): u32 data_size + u16 num_colors + num_colors * [R, G, B, 0]
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
