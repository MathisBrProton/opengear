//! Native Windows laptop telemetry and display controls:
//! - Chassis detection (`Win32_Battery` / `Win32_SystemEnclosure`)
//! - Battery health, cycle count, voltage, real-time charge/discharge Watts (`root\wmi`)
//! - Internal panel brightness (`WmiMonitorBrightness` / `WmiMonitorBrightnessMethods`)
//! - Refresh-rate enumeration & switching (`EnumDisplaySettingsW` / `ChangeDisplaySettingsExW`)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LaptopBatteryInfo {
    pub present: bool,
    pub ac_online: bool,
    pub charging: bool,
    pub discharging: bool,
    pub percent: Option<u8>,
    pub design_capacity_mwh: Option<u32>,
    pub full_charged_capacity_mwh: Option<u32>,
    pub remaining_capacity_mwh: Option<u32>,
    pub health_pct: Option<f32>,
    pub cycle_count: Option<u32>,
    pub voltage_v: Option<f32>,
    /// Positive when charging, negative when discharging.
    pub power_watts: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub brightness_pct: Option<u8>,
    pub current_hz: Option<u32>,
    pub supported_hz: Vec<u32>,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn compute_battery_health(design: Option<u32>, full: Option<u32>) -> Option<f32> {
    match (design, full) {
        (Some(d), Some(f)) if d > 0 && f > 0 => {
            Some(((f as f32 / d as f32) * 100.0).clamp(0.0, 120.0))
        }
        _ => None,
    }
}

pub(crate) fn compute_battery_watts(
    charging: bool,
    discharging: bool,
    charge_rate_mw: Option<u32>,
    discharge_rate_mw: Option<u32>,
) -> Option<f32> {
    let valid_mw = |r: Option<u32>| r.filter(|&v| v > 0 && v < 0x8000_0000);
    if charging {
        valid_mw(charge_rate_mw).map(|mw| mw as f32 / 1000.0)
    } else if discharging {
        valid_mw(discharge_rate_mw).map(|mw| -(mw as f32) / 1000.0)
    } else {
        None
    }
}

pub(crate) fn normalize_refresh_rates(rates: Vec<u32>) -> Vec<u32> {
    let mut out: Vec<u32> = rates
        .into_iter()
        .filter(|&hz| (24..=500).contains(&hz))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Detect whether the host machine is a laptop/notebook/portable chassis.
pub fn is_laptop_chassis() -> bool {
    #[cfg(windows)]
    {
        if let Ok(conn) = crate::wmi::WmiConn::connect(r"root\cimv2") {
            if let Ok(rows) = conn.query("SELECT BatteryStatus FROM Win32_Battery")
                && !rows.is_empty()
            {
                return true;
            }
            if let Ok(rows) = conn.query("SELECT ChassisTypes FROM Win32_SystemEnclosure") {
                for row in rows {
                    if let Some(types) = row.get("ChassisTypes").and_then(|v| v.as_ints()) {
                        // SMBIOS portable types: Portable(8), Laptop(9), Notebook(10),
                        // SubNotebook(14), Tablet(30), Convertible(31), Detachable(32).
                        if types
                            .iter()
                            .any(|&t| matches!(t, 8 | 9 | 10 | 14 | 30 | 31 | 32))
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
    #[cfg(not(windows))]
    false
}

/// Read live battery health, cycle count, charge percentage, and charge/discharge power.
pub fn read_laptop_battery() -> LaptopBatteryInfo {
    let mut info = LaptopBatteryInfo::default();
    #[cfg(windows)]
    {
        if let Ok(conn) = crate::wmi::WmiConn::connect(r"root\wmi") {
            if let Ok(rows) = conn.query("SELECT DesignedCapacity FROM BatteryStaticData") {
                info.design_capacity_mwh = rows
                    .first()
                    .and_then(|r| r.get("DesignedCapacity"))
                    .and_then(|v| v.as_u64())
                    .filter(|&v| v > 0)
                    .map(|v| v as u32);
            }
            if let Ok(rows) =
                conn.query("SELECT FullChargedCapacity FROM BatteryFullChargedCapacity")
            {
                info.full_charged_capacity_mwh = rows
                    .first()
                    .and_then(|r| r.get("FullChargedCapacity"))
                    .and_then(|v| v.as_u64())
                    .filter(|&v| v > 0)
                    .map(|v| v as u32);
            }
            if let Ok(rows) = conn.query("SELECT CycleCount FROM BatteryCycleCount") {
                info.cycle_count = rows
                    .first()
                    .and_then(|r| r.get("CycleCount"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
            }
            if let Ok(rows) = conn.query(
                "SELECT PowerOnline, Charging, Discharging, RemainingCapacity, ChargeRate, DischargeRate, Voltage FROM BatteryStatus",
            )
                && let Some(row) = rows.first() {
                    info.present = true;
                    info.ac_online = row
                        .get("PowerOnline")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    info.charging = row
                        .get("Charging")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    info.discharging = row
                        .get("Discharging")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    info.remaining_capacity_mwh = row
                        .get("RemainingCapacity")
                        .and_then(|v| v.as_u64())
                        .filter(|&v| v < 0x8000_0000)
                        .map(|v| v as u32);
                    info.voltage_v = row
                        .get("Voltage")
                        .and_then(|v| v.as_u64())
                        .filter(|&v| v > 0 && v < 100_000)
                        .map(|mv| mv as f32 / 1000.0);
                    let charge_rate = row
                        .get("ChargeRate")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    let discharge_rate = row
                        .get("DischargeRate")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    info.power_watts = compute_battery_watts(
                        info.charging,
                        info.discharging,
                        charge_rate,
                        discharge_rate,
                    );
                }
        }

        // Cross-check / fallback via Win32_Battery for percent & AC detection
        if let Ok(cim) = crate::wmi::WmiConn::connect(r"root\cimv2")
            && let Ok(rows) = cim.query(
                "SELECT EstimatedChargeRemaining, BatteryStatus, DesignVoltage FROM Win32_Battery",
            )
            && let Some(row) = rows.first()
        {
            info.present = true;
            info.percent = row
                .get("EstimatedChargeRemaining")
                .and_then(|v| v.as_u64())
                .map(|v| v.min(100) as u8);
            if info.voltage_v.is_none() {
                info.voltage_v = row
                    .get("DesignVoltage")
                    .and_then(|v| v.as_u64())
                    .filter(|&v| v > 0)
                    .map(|mv| mv as f32 / 1000.0);
            }
            if let Some(st) = row.get("BatteryStatus").and_then(|v| v.as_u64())
                && !info.ac_online
                && matches!(st, 2 | 3 | 6 | 7 | 8 | 9)
            {
                info.ac_online = true;
            }
        }

        if info.percent.is_none()
            && let (Some(rem), Some(full)) =
                (info.remaining_capacity_mwh, info.full_charged_capacity_mwh)
            && full > 0
        {
            info.percent = Some(
                ((rem as f32 / full as f32) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8,
            );
        }
        info.health_pct =
            compute_battery_health(info.design_capacity_mwh, info.full_charged_capacity_mwh);
    }
    info
}

/// Read internal panel brightness (via `root\wmi`) and active/supported refresh rates (Win32 GDI).
pub fn read_display_info() -> DisplayInfo {
    let mut info = DisplayInfo::default();
    #[cfg(windows)]
    {
        if let Ok(conn) = crate::wmi::WmiConn::connect(r"root\wmi")
            && let Ok(rows) = conn.query("SELECT CurrentBrightness FROM WmiMonitorBrightness")
        {
            info.brightness_pct = rows
                .first()
                .and_then(|r| r.get("CurrentBrightness"))
                .and_then(|v| v.as_u64())
                .map(|v| v.min(100) as u8);
        }

        use windows_sys::Win32::Graphics::Gdi::{
            DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW,
        };
        unsafe {
            let mut cur: DEVMODEW = std::mem::zeroed();
            cur.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            if EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut cur) != 0 {
                info.width = cur.dmPelsWidth;
                info.height = cur.dmPelsHeight;
                if cur.dmDisplayFrequency > 0 {
                    info.current_hz = Some(cur.dmDisplayFrequency);
                }
                let bpp = cur.dmBitsPerPel;
                let mut rates = Vec::new();
                let mut mode_num = 0u32;
                loop {
                    let mut dm: DEVMODEW = std::mem::zeroed();
                    dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
                    if EnumDisplaySettingsW(std::ptr::null(), mode_num, &mut dm) == 0 {
                        break;
                    }
                    if dm.dmPelsWidth == cur.dmPelsWidth
                        && dm.dmPelsHeight == cur.dmPelsHeight
                        && dm.dmBitsPerPel == bpp
                    {
                        rates.push(dm.dmDisplayFrequency);
                    }
                    mode_num += 1;
                }
                info.supported_hz = normalize_refresh_rates(rates);
            }
        }
    }
    info
}

/// Set the internal laptop display brightness (`0..=100`) via `WmiMonitorBrightnessMethods`.
pub fn set_display_brightness(pct: u8) -> Result<()> {
    #[cfg(windows)]
    {
        use crate::wmi::{WmiConn, WmiVal};
        let pct = pct.min(100);
        let conn = WmiConn::connect(r"root\wmi")?;
        let path = conn
            .first_active_instance_path("WmiMonitorBrightnessMethods")?
            .context("Contrôleur de luminosité WMI absent (écran externe ?)")?;
        let _ = conn.exec_method(
            &path,
            "WmiMonitorBrightnessMethods",
            "WmiSetBrightness",
            &[("Timeout", WmiVal::U32(1)), ("Brightness", WmiVal::U8(pct))],
        )?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = pct;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Switch the primary display refresh rate while preserving current resolution and color depth.
pub fn set_display_refresh_rate(target_hz: u32) -> Result<u32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::{
            CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE_SUCCESSFUL,
            DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW,
        };
        let cur_info = read_display_info();
        anyhow::ensure!(
            cur_info.supported_hz.contains(&target_hz),
            "Fréquence {} Hz non supportée sur cet écran (disponibles : {:?})",
            target_hz,
            cur_info.supported_hz
        );
        unsafe {
            let mut dm: DEVMODEW = std::mem::zeroed();
            dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            anyhow::ensure!(
                EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut dm) != 0,
                "Impossible de lire le mode d'affichage actuel"
            );
            dm.dmDisplayFrequency = target_hz;
            dm.dmFields = DM_DISPLAYFREQUENCY;
            let rc = ChangeDisplaySettingsExW(
                std::ptr::null(),
                &dm,
                std::ptr::null_mut(),
                CDS_UPDATEREGISTRY,
                std::ptr::null(),
            );
            anyhow::ensure!(
                rc == DISP_CHANGE_SUCCESSFUL,
                "ChangeDisplaySettingsExW({target_hz} Hz) a échoué (code {rc})"
            );
        }
        Ok(target_hz)
    }
    #[cfg(not(windows))]
    {
        let _ = target_hz;
        anyhow::bail!("Non supporté hors Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_health_and_watts_calculations() {
        assert_eq!(
            compute_battery_health(Some(70000), Some(64680)).map(|v| (v * 10.0).round() / 10.0),
            Some(92.4)
        );
        assert_eq!(compute_battery_health(Some(0), Some(50000)), None);
        assert_eq!(
            compute_battery_watts(true, false, Some(35400), None),
            Some(35.4)
        );
        assert_eq!(
            compute_battery_watts(false, true, None, Some(18250)),
            Some(-18.25)
        );
        assert_eq!(
            compute_battery_watts(false, false, Some(0), Some(0xFFFFFFFF)),
            None
        );
    }

    #[test]
    fn refresh_rates_are_sorted_and_filtered() {
        assert_eq!(
            normalize_refresh_rates(vec![144, 60, 144, 0, 1000, 60]),
            vec![60, 144]
        );
    }

    #[test]
    #[ignore]
    fn live_wmi_brightness_roundtrip() {
        let info = read_display_info();
        let cur = info
            .brightness_pct
            .expect("WmiMonitorBrightness indisponible");
        println!("Current panel brightness: {cur}%");
        set_display_brightness(cur)
            .unwrap_or_else(|e| panic!("set_display_brightness({cur}) failed: {e:#}"));
    }
}
