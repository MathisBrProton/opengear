use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanRpmReading {
    pub name: String,
    pub rpm: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareSensorsSnapshot {
    /// Source of sensors (e.g. "LibreHardwareMonitor WMI", "OpenHardwareMonitor WMI", or "Aucun démon capteur (ADL2 GPU seul)")
    pub source: String,
    pub available: bool,
    pub cpu_temp_c: Option<f32>,
    pub motherboard_temp_c: Option<f32>,
    pub aio_pump_rpm: Option<u32>,
    pub cpu_fan_rpm: Option<u32>,
    pub fans: Vec<FanRpmReading>,
}

/// Universal hardware sensor reader:
/// 1. Native WHQL-signed **PawnIO** (`AMDFamily17` Ryzen Tctl/Tdie or `IntelMSR` Intel Package Temp) — <0.2 ms, zero PowerShell, works on ANY AMD/Intel PC!
/// 2. Optionally merges `LibreHardwareMonitor` / `OpenHardwareMonitor` WMI if running (for SuperIO fan/AIO RPMs & motherboard temp).
pub fn read_hardware_sensors() -> HardwareSensorsSnapshot {
    let mut snap = HardwareSensorsSnapshot::default();

    // 1. Instant native CPU temperature via WHQL-signed PawnIO (works on ANY AMD Ryzen or Intel Core!)
    if let Some((cpu_t, src)) = crate::pawnio::read_native_cpu_temp() {
        snap.cpu_temp_c = Some((cpu_t * 10.0).round() / 10.0);
        snap.available = true;
        snap.source = src.to_string();
    }

    // 1b. Instant native SuperIO Fan RPMs via WHQL-signed PawnIO LpcIO (Nuvoton NCT679xD — ZERO LHM needed!)
    let sio = crate::superio::read_superio_snapshot();
    if sio.detected {
        snap.available = true;
        if snap.source.is_empty() {
            snap.source = format!("PawnIO {}", sio.chip_name);
        } else {
            snap.source = format!("{} + {}", snap.source, sio.chip_name);
        }
        for f in &sio.fans {
            if f.rpm > 0 {
                snap.fans.push(FanRpmReading {
                    name: format!("{} ({}%)", f.label, f.pwm_pct),
                    rpm: f.rpm,
                });
            }
        }
        // If any fan spins > 1800 RPM (typical Silent Loop 3 pump 1800-3500 RPM), expose as aio_pump_rpm fallback
        if snap.aio_pump_rpm.is_none() {
            snap.aio_pump_rpm = sio
                .fans
                .iter()
                .map(|f| f.rpm)
                .filter(|&r| r >= 1500)
                .max()
                .or_else(|| sio.fans.first().map(|f| f.rpm).filter(|&r| r > 0));
        }
        // Skip spawning PowerShell WMI when PawnIO already gave us BOTH CPU temp & live fan RPMs!
        if snap.cpu_temp_c.is_some() && !snap.fans.is_empty() {
            return snap;
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let script = r#"
$ns = $null
foreach ($cand in @('root\LibreHardwareMonitor', 'root\OpenHardwareMonitor')) {
  if (Get-CimClass -Namespace $cand -ClassName Sensor -ErrorAction SilentlyContinue) {
    $ns = $cand; break
  }
}
if (-not $ns) {
  Write-Output "NONE"
  exit 0
}
Write-Output "SRC|$ns"
$sensors = Get-CimInstance -Namespace $ns -ClassName Sensor -ErrorAction SilentlyContinue | Where-Object { $_.SensorType -eq 'Temperature' -or $_.SensorType -eq 'Fan' }
foreach ($s in $sensors) {
  if ($null -ne $s.Value) {
    Write-Output ("S|{0}|{1}|{2}" -f $s.SensorType, $s.Name, ([math]::Round([double]$s.Value, 1)))
  }
}
"#;
        if let Ok(out) = Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            let txt = String::from_utf8_lossy(&out.stdout);
            let mut lhm_found = false;
            for line in txt.lines() {
                let l = line.trim();
                if l == "NONE" {
                    break;
                }
                if let Some(ns) = l.strip_prefix("SRC|") {
                    lhm_found = true;
                    let wmi_src = if ns.contains("Libre") {
                        "LibreHardwareMonitor WMI"
                    } else {
                        "OpenHardwareMonitor WMI"
                    };
                    if snap.source.is_empty() {
                        snap.source = wmi_src.into();
                    } else {
                        snap.source = format!("{} + {}", snap.source, wmi_src);
                    }
                    snap.available = true;
                } else if let Some(rest) = l.strip_prefix("S|") {
                    let parts: Vec<&str> = rest.split('|').collect();
                    if parts.len() >= 3 {
                        let stype = parts[0];
                        let name = parts[1].trim();
                        let val: f32 = parts[2].replace(',', ".").parse().unwrap_or(0.0);
                        let lower = name.to_ascii_lowercase();
                        if stype == "Temperature" {
                            if snap.cpu_temp_c.is_none()
                                && (lower.contains("tdie")
                                    || lower.contains("tctl")
                                    || lower.contains("cpu package")
                                    || lower == "core (tctl/tdie)")
                            {
                                snap.cpu_temp_c = Some(val);
                            } else if snap.cpu_temp_c.is_none() && lower.contains("cpu") {
                                snap.cpu_temp_c = Some(val);
                            } else if lower.contains("motherboard")
                                || lower.contains("chipset")
                                || lower.contains("vrm")
                            {
                                snap.motherboard_temp_c = Some(val);
                            }
                        } else if stype == "Fan" {
                            let rpm = val.max(0.0).round() as u32;
                            if lower.contains("pump") || lower.contains("aio") {
                                snap.aio_pump_rpm = Some(rpm);
                            } else if lower.contains("cpu") {
                                snap.cpu_fan_rpm = Some(rpm);
                            }
                            if rpm > 0 || lower.contains("fan") || lower.contains("pump") {
                                snap.fans.push(FanRpmReading {
                                    name: name.to_string(),
                                    rpm,
                                });
                            }
                        }
                    }
                }
            }
            if lhm_found || snap.available {
                return snap;
            }
        }
    }

    if snap.available {
        return snap;
    }

    let hint = match crate::pawnio::pawnio_availability() {
        crate::pawnio::PawnIoAvailability::NeedsAdmin => {
            "PawnIO détecté — relance en Administrateur (UAC) pour la température CPU AMD/Intel native (zéro LHM requis)"
        }
        crate::pawnio::PawnIoAvailability::Ready => {
            "PawnIO actif (plateforme CPU non reconnue) — LHM optionnel pour RPM"
        }
        crate::pawnio::PawnIoAvailability::NotInstalled => {
            "Installe le pilote signé PawnIO (pawnio.eu) pour la temp CPU AMD/Intel native (ou LibreHardwareMonitor pour RPM)"
        }
    };

    HardwareSensorsSnapshot {
        source: hint.into(),
        available: false,
        ..Default::default()
    }
}
