//! Fan safety net for crashes and forced kills.
//!
//! While OpenGear overrides a fan (Smart Cooling flat curves through ASUS WMI, or direct SuperIO
//! PWM), it keeps a marker file describing how to give control back to the BIOS:
//! `%APPDATA%\OpenGear\fan_override.json`. A clean exit restores the fans itself and deletes the
//! marker. If the process dies instead (crash, `taskkill /F`, installer update):
//! - a hidden watchdog process (`opengear --fan-watchdog <pid>`) waiting on the main process
//!   applies the marker immediately;
//! - and, as a last resort, the next GUI launch applies any marker left behind.

use opengear_asus::apply_asus_wmi_curve;
use opengear_core::cooling::FanChannelConfig;
use opengear_mainboard::{SuperIoSavedState, restore_superio_saved_states};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FanOverrideMarker {
    /// PID of the OpenGear process that owns the overrides.
    pub pid: u32,
    /// BIOS fan configurations to re-apply through ASUS WMI.
    #[serde(default)]
    pub wmi_restore: Vec<FanChannelConfig>,
    /// Original SuperIO channel states to write back.
    #[serde(default)]
    pub superio_restore: Vec<SuperIoSavedState>,
}

impl FanOverrideMarker {
    pub fn is_empty(&self) -> bool {
        self.wmi_restore.is_empty() && self.superio_restore.is_empty()
    }
}

pub fn marker_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenGear")
        .join("fan_override.json")
}

pub fn read_marker() -> Option<FanOverrideMarker> {
    let raw = std::fs::read_to_string(marker_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Write (or delete, when there is nothing to restore) the marker atomically.
pub fn write_marker(marker: &FanOverrideMarker) {
    let path = marker_path();
    if marker.is_empty() {
        clear_marker();
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(json) = serde_json::to_string_pretty(marker) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok()
        && let Err(e) = std::fs::rename(&tmp, &path)
    {
        error!("Marqueur ventilateurs non écrit ({}) : {e}", path.display());
    }
}

pub fn clear_marker() {
    let _ = std::fs::remove_file(marker_path());
}

/// Give fan control back to the BIOS as described by the marker. Returns `(logs, remaining)`
/// where `remaining` keeps only items whose restoration failed so callers can retry later.
pub(crate) fn restore_from_marker_with<F>(
    marker: &FanOverrideMarker,
    mut apply_wmi: F,
) -> (Vec<String>, Option<FanOverrideMarker>)
where
    F: FnMut(&FanChannelConfig) -> Result<(), String>,
{
    let mut log = Vec::new();
    let mut remaining_wmi = Vec::new();
    for cfg in &marker.wmi_restore {
        match apply_wmi(cfg) {
            Ok(()) => log.push(format!("Ventilateur {:?} rendu au BIOS", cfg.header)),
            Err(e) => {
                log.push(format!(
                    "Ventilateur {:?} : restauration échouée : {e}",
                    cfg.header
                ));
                remaining_wmi.push(cfg.clone());
            }
        }
    }
    let superio_logs = if marker.superio_restore.is_empty() {
        Vec::new()
    } else {
        restore_superio_saved_states(&marker.superio_restore)
    };
    let superio_failed = superio_logs.iter().any(|l| {
        let low = l.to_ascii_lowercase();
        low.contains("échou") || low.contains("erreur") || low.contains("impossible")
    });
    log.extend(superio_logs);
    let rem = FanOverrideMarker {
        pid: marker.pid,
        wmi_restore: remaining_wmi,
        superio_restore: if superio_failed {
            marker.superio_restore.clone()
        } else {
            Vec::new()
        },
    };
    let remaining = if rem.is_empty() { None } else { Some(rem) };
    (log, remaining)
}

/// Startup recovery: a marker left by a previous (dead) session is applied, then updated or removed.
/// Call only once the single-instance lock is held, so the owner cannot still be running.
pub fn recover_stale_marker() {
    let Some(marker) = read_marker() else {
        return;
    };
    if marker.pid == std::process::id() {
        return;
    }
    if !opengear_platform::is_elevated() {
        // WMI / SuperIO writes need admin: keep the marker for the next elevated launch.
        warn!(
            "Ventilateurs laissés en mode manuel par une session précédente : relance OpenGear en administrateur pour les rendre au BIOS."
        );
        return;
    }
    warn!("Ventilateurs laissés en mode manuel par une session précédente — restauration BIOS.");
    let (logs, remaining) = restore_from_marker_with(&marker, |cfg| {
        apply_asus_wmi_curve(cfg)
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    for line in logs {
        info!("{line}");
    }
    match remaining {
        Some(rem) => write_marker(&rem),
        None => clear_marker(),
    }
}

/// Spawn the hidden watchdog for the current process (once per session).
pub fn spawn_watchdog() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["--fan-watchdog", &std::process::id().to_string()]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map(|_| ())
}

/// Entry point of `opengear --fan-watchdog <pid>`: wait for `parent_pid` to exit, then restore
/// the fans if it left its marker behind (i.e. it did not shut down cleanly).
pub fn run_watchdog(parent_pid: u32) -> anyhow::Result<()> {
    wait_for_process_exit(parent_pid)?;
    // Give a clean shutdown a moment to delete its marker.
    std::thread::sleep(std::time::Duration::from_millis(500));
    if let Some(marker) = read_marker()
        && marker.pid == parent_pid
    {
        let (logs, remaining) = restore_from_marker_with(&marker, |cfg| {
            apply_asus_wmi_curve(cfg)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        for line in logs {
            info!("{line}");
        }
        match remaining {
            Some(rem) => write_marker(&rem),
            None => clear_marker(),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_process_exit(pid: u32) -> anyhow::Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{INFINITE, OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if h.is_null() {
            // Already gone (or inaccessible): fall through to the marker check.
            return Ok(());
        }
        WaitForSingleObject(h, INFINITE);
        CloseHandle(h);
    }
    Ok(())
}

#[cfg(not(windows))]
fn wait_for_process_exit(_pid: u32) -> anyhow::Result<()> {
    anyhow::bail!("Watchdog ventilateurs : Windows uniquement")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trips_through_json() {
        let m = FanOverrideMarker {
            pid: 42,
            wmi_restore: vec![FanChannelConfig::default_for(
                opengear_core::cooling::FanHeaderKind::AioPump,
            )],
            superio_restore: vec![SuperIoSavedState {
                index: 1,
                mode: 0x40,
                cmd: 128,
            }],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: FanOverrideMarker = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pid, 42);
        assert_eq!(back.superio_restore, m.superio_restore);
        assert_eq!(back.wmi_restore.len(), 1);
        assert!(!back.is_empty());
        assert!(FanOverrideMarker::default().is_empty());
    }

    #[test]
    fn partial_restore_retains_failed_entries_only() {
        use opengear_core::cooling::FanHeaderKind;
        let m = FanOverrideMarker {
            pid: 99,
            wmi_restore: vec![
                FanChannelConfig::default_for(FanHeaderKind::CpuFan),
                FanChannelConfig::default_for(FanHeaderKind::AioPump),
            ],
            superio_restore: Vec::new(),
        };
        let (logs, remaining) = restore_from_marker_with(&m, |cfg| {
            if cfg.header == FanHeaderKind::CpuFan {
                Ok(())
            } else {
                Err("WMI busy".into())
            }
        });
        assert_eq!(logs.len(), 2);
        let rem = remaining.expect("failed entry should be retained");
        assert_eq!(rem.wmi_restore.len(), 1);
        assert_eq!(rem.wmi_restore[0].header, FanHeaderKind::AioPump);

        let (_, none_left) = restore_from_marker_with(&m, |_| Ok(()));
        assert!(none_left.is_none());
    }
}
