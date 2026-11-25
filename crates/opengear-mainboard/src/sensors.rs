use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanRpmReading {
    pub name: String,
    pub rpm: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareSensorsSnapshot {
    /// Source of sensors (e.g. "PawnIO AMD Ryzen Tctl/Tdie + Nuvoton NCT6798D", or LHM WMI)
    pub source: String,
    pub available: bool,
    pub cpu_temp_c: Option<f32>,
    pub motherboard_temp_c: Option<f32>,
    pub aio_pump_rpm: Option<u32>,
    pub cpu_fan_rpm: Option<u32>,
    pub fans: Vec<FanRpmReading>,
}

/// Classify SuperIO fan channels without mislabeling air-cooled CPU fans as an AIO pump.
///
/// Only RPMs >= 1800 are treated as an AIO pump fallback; slower primary channels map to `cpu_fan_rpm`.
pub(crate) fn classify_superio_rpms(rpms: &[u32]) -> (Option<u32>, Option<u32>) {
    let pump = rpms.iter().copied().filter(|&r| r >= 1800).max();
    let cpu_fan = rpms.iter().copied().find(|&r| r > 0 && Some(r) != pump);
    (pump, cpu_fan)
}

/// Universal hardware sensor reader:
/// 1. Native WHQL-signed **PawnIO** (`AMDFamily17` Ryzen Tctl/Tdie or `IntelMSR` Intel Package Temp) — <0.2 ms.
/// 2. Native WHQL-signed **PawnIO** (`LpcIO` SuperIO Nuvoton fan RPMs & PWM).
/// 3. Native COM WMI `LibreHardwareMonitor` / `OpenHardwareMonitor` fallback (no PowerShell subprocess!).
pub fn read_hardware_sensors() -> HardwareSensorsSnapshot {
    let mut snap = HardwareSensorsSnapshot::default();

    // 1. Instant native CPU temperature via WHQL-signed PawnIO
    if let Some((cpu_t, src)) = crate::cpu_temp::read_native_cpu_temp() {
        snap.cpu_temp_c = Some((cpu_t * 10.0).round() / 10.0);
        snap.available = true;
        snap.source = src.to_string();
    }

    // 1b. Instant native SuperIO Fan RPMs via WHQL-signed PawnIO LpcIO
    let sio = crate::superio::read_superio_snapshot();
    if sio.detected {
        snap.available = true;
        if snap.source.is_empty() {
            snap.source = format!("PawnIO {}", sio.chip_name);
        } else {
            snap.source = format!("{} + {}", snap.source, sio.chip_name);
        }
        let mut active_rpms = Vec::new();
        for f in &sio.fans {
            if f.rpm > 0 {
                active_rpms.push(f.rpm);
                snap.fans.push(FanRpmReading {
                    name: format!("{} ({}%)", f.label, f.pwm_pct),
                    rpm: f.rpm,
                });
            }
        }
        let (pump, cpu_fan) = classify_superio_rpms(&active_rpms);
        if snap.aio_pump_rpm.is_none() {
            snap.aio_pump_rpm = pump;
        }
        if snap.cpu_fan_rpm.is_none() {
            snap.cpu_fan_rpm = cpu_fan;
        }
    }

    // 2. Native COM WMI LibreHardwareMonitor / OpenHardwareMonitor (zero child processes)
    #[cfg(windows)]
    if snap.cpu_temp_c.is_none() || snap.fans.is_empty() || snap.motherboard_temp_c.is_none() {
        for (ns, wmi_src) in [
            (r"root\LibreHardwareMonitor", "LibreHardwareMonitor WMI"),
            (r"root\OpenHardwareMonitor", "OpenHardwareMonitor WMI"),
        ] {
            let Ok(conn) = opengear_platform::wmi::WmiConn::connect(ns) else {
                continue;
            };
            let Ok(rows) = conn.query(
                "SELECT SensorType, Name, Value FROM Sensor WHERE SensorType='Temperature' OR SensorType='Fan'",
            ) else {
                continue;
            };
            if rows.is_empty() {
                continue;
            }
            if snap.source.is_empty() {
                snap.source = wmi_src.into();
            } else {
                snap.source = format!("{} + {}", snap.source, wmi_src);
            }
            snap.available = true;

            for row in &rows {
                let stype = row.get("SensorType").and_then(|v| v.as_str()).unwrap_or("");
                let name = row
                    .get("Name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                let Some(val) = row.get("Value").and_then(|v| v.as_f64()).map(|v| v as f32) else {
                    continue;
                };
                let lower = name.to_ascii_lowercase();
                if stype == "Temperature" {
                    let rounded = (val * 10.0).round() / 10.0;
                    if snap.cpu_temp_c.is_none()
                        && (lower.contains("tdie")
                            || lower.contains("tctl")
                            || lower.contains("cpu")
                            || lower == "core (tctl/tdie)")
                    {
                        snap.cpu_temp_c = Some(rounded);
                    } else if snap.motherboard_temp_c.is_none()
                        && (lower.contains("motherboard")
                            || lower.contains("chipset")
                            || lower.contains("vrm"))
                    {
                        snap.motherboard_temp_c = Some(rounded);
                    }
                } else if stype == "Fan" {
                    let rpm = val.max(0.0).round() as u32;
                    if lower.contains("pump") || lower.contains("aio") {
                        snap.aio_pump_rpm = Some(rpm);
                    } else if lower.contains("cpu") {
                        snap.cpu_fan_rpm = Some(rpm);
                    }
                    if !sio.detected && (rpm > 0 || lower.contains("fan") || lower.contains("pump"))
                    {
                        snap.fans.push(FanRpmReading {
                            name: name.to_string(),
                            rpm,
                        });
                    }
                }
            }
            break;
        }
    }

    if snap.available {
        return snap;
    }

    let hint = match opengear_platform::pawnio::pawnio_availability() {
        opengear_platform::PawnIoAvailability::NeedsAdmin => {
            "PawnIO détecté — relance en Administrateur (UAC) pour la température CPU AMD/Intel native (zéro LHM requis)"
        }
        opengear_platform::PawnIoAvailability::Ready => {
            "PawnIO actif (plateforme CPU non reconnue) — LHM optionnel pour RPM"
        }
        opengear_platform::PawnIoAvailability::NotInstalled => {
            "Installe le pilote signé PawnIO pour la temp CPU AMD/Intel native (ou LibreHardwareMonitor pour RPM)"
        }
    };

    HardwareSensorsSnapshot {
        source: hint.into(),
        available: false,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::classify_superio_rpms;

    #[test]
    fn air_cooler_is_not_mislabeled_as_aio_pump() {
        assert_eq!(classify_superio_rpms(&[1100, 850]), (None, Some(1100)));
        assert_eq!(
            classify_superio_rpms(&[2850, 1200]),
            (Some(2850), Some(1200))
        );
        assert_eq!(classify_superio_rpms(&[]), (None, None));
    }
}
