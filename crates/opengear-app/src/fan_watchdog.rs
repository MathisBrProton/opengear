//! Fan safety net for crashes and forced kills.
//!
//! While OpenGear overrides a fan (Smart Cooling flat curves through ASUS WMI, direct SuperIO
//! PWM, or a device's `Cooling` capability such as a laptop embedded controller), it keeps a
//! marker file describing how to give control back to the BIOS:
//! `%APPDATA%\OpenGear\fan_override.json`. A clean exit restores the fans itself and deletes the
//! marker. If the process dies instead (crash, `taskkill /F`, installer update):
//! - a hidden watchdog process (`opengear --fan-watchdog <pid>`) waiting on the main process
//!   applies the marker immediately;
//! - and, as a last resort, the next GUI launch applies any marker left behind.

use opengear_core::AppConfig;
use opengear_core::cooling::FanChannelConfig;
use opengear_core::driver::{Device, Driver};
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
    /// Ids of shared-model devices whose `Cooling` is overridden (manual speed, max fan…):
    /// rediscovered and given back to the firmware with `restore_firmware_control()`.
    #[serde(default)]
    pub cooling_devices: Vec<String>,
}

impl FanOverrideMarker {
    pub fn is_empty(&self) -> bool {
        self.wmi_restore.is_empty()
            && self.superio_restore.is_empty()
            && self.cooling_devices.is_empty()
    }
}

/// Result of restoring one device: `Ok` when given back, `Err` with the reason otherwise.
pub(crate) type CoolingRestore = (String, Result<(), String>);

/// Give firmware control back on each device of `devices` whose id is in `ids`. Ids without a
/// matching device (unplugged, driver disabled) are reported as not found.
pub(crate) fn restore_cooling_devices(
    devices: &mut [Box<dyn Device>],
    ids: &[String],
) -> Vec<CoolingRestore> {
    ids.iter()
        .map(|id| {
            let res = match devices.iter_mut().find(|d| &d.info().id == id) {
                None => Err("appareil introuvable".to_string()),
                Some(dev) => match dev.cooling() {
                    None => Err("ventilateurs indisponibles".to_string()),
                    Some(c) => c.restore_firmware_control().map_err(|e| e.to_string()),
                },
            };
            (id.clone(), res)
        })
        .collect()
}

/// Drivers that may own one of `ids` (a device id starts with `<driver name>:`), respecting the
/// user's experimental-driver opt-in.
pub(crate) fn drivers_for_ids<'a>(
    drivers: &'a [Box<dyn Driver>],
    enabled_experimental: &std::collections::BTreeSet<String>,
    ids: &[String],
) -> Vec<&'a dyn Driver> {
    drivers
        .iter()
        .filter(|d| !d.experimental() || enabled_experimental.contains(d.name()))
        .filter(|d| ids.iter().any(|id| id.split(':').next() == Some(d.name())))
        .map(|d| d.as_ref())
        .collect()
}

/// Rediscover the devices listed in the marker (only their drivers are probed) and restore them.
fn rediscover_and_restore(ids: &[String]) -> Vec<CoolingRestore> {
    if ids.is_empty() {
        return Vec::new();
    }
    let config = AppConfig::load(&AppConfig::default_config_path()).unwrap_or_default();
    let drivers = crate::drivers::device_drivers();
    let mut devices: Vec<Box<dyn Device>> =
        drivers_for_ids(&drivers, &config.experimental_drivers, ids)
            .into_iter()
            .flat_map(|d| d.discover())
            .collect();
    restore_cooling_devices(&mut devices, ids)
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
pub(crate) fn restore_from_marker_with<F, C>(
    marker: &FanOverrideMarker,
    mut apply_wmi: F,
    restore_cooling: C,
) -> (Vec<String>, Option<FanOverrideMarker>)
where
    F: FnMut(&FanChannelConfig) -> Result<(), String>,
    C: FnOnce(&[String]) -> Vec<CoolingRestore>,
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
    let mut remaining_cooling = Vec::new();
    if !marker.cooling_devices.is_empty() {
        for (id, res) in restore_cooling(&marker.cooling_devices) {
            match res {
                Ok(()) => log.push(format!("Ventilateurs de {id} rendus au firmware")),
                Err(e) => {
                    log.push(format!("Ventilateurs de {id} : restauration échouée : {e}"));
                    remaining_cooling.push(id);
                }
            }
        }
    }
    let rem = FanOverrideMarker {
        pid: marker.pid,
        wmi_restore: remaining_wmi,
        superio_restore: if superio_failed {
            marker.superio_restore.clone()
        } else {
            Vec::new()
        },
        cooling_devices: remaining_cooling,
    };
    let remaining = if rem.is_empty() { None } else { Some(rem) };
    (log, remaining)
}

fn restore_marker_via_drivers(
    marker: &FanOverrideMarker,
) -> (Vec<String>, Option<FanOverrideMarker>) {
    let mut curve_devices: Vec<Box<dyn Device>> = if marker.wmi_restore.is_empty() {
        Vec::new()
    } else {
        let config = AppConfig::load(&AppConfig::default_config_path()).unwrap_or_default();
        crate::drivers::device_drivers()
            .into_iter()
            .filter(|d| !d.experimental() || config.experimental_drivers.contains(d.name()))
            .flat_map(|d| d.discover())
            .filter_map(|mut dev| dev.fan_curves().is_some().then_some(dev))
            .collect()
    };
    restore_from_marker_with(
        marker,
        |cfg| match curve_devices.iter_mut().find_map(|d| d.fan_curves()) {
            Some(fc) => fc.apply_channel_curve(cfg).map_err(|e| e.to_string()),
            None => Err("Contrôleur de courbes carte mère introuvable".into()),
        },
        rediscover_and_restore,
    )
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
    let (logs, remaining) = restore_marker_via_drivers(&marker);
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
        let (logs, remaining) = restore_marker_via_drivers(&marker);
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
            cooling_devices: vec!["hp_omen:keyboard".into()],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: FanOverrideMarker = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pid, 42);
        assert_eq!(back.superio_restore, m.superio_restore);
        assert_eq!(back.wmi_restore.len(), 1);
        assert_eq!(back.cooling_devices, ["hp_omen:keyboard"]);
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
            cooling_devices: Vec::new(),
        };
        let no_cooling = |_: &[String]| -> Vec<CoolingRestore> { panic!("no cooling device") };
        let (logs, remaining) = restore_from_marker_with(
            &m,
            |cfg| {
                if cfg.header == FanHeaderKind::CpuFan {
                    Ok(())
                } else {
                    Err("WMI busy".into())
                }
            },
            no_cooling,
        );
        assert_eq!(logs.len(), 2);
        let rem = remaining.expect("failed entry should be retained");
        assert_eq!(rem.wmi_restore.len(), 1);
        assert_eq!(rem.wmi_restore[0].header, FanHeaderKind::AioPump);

        let (_, none_left) = restore_from_marker_with(&m, |_| Ok(()), no_cooling);
        assert!(none_left.is_none());
    }

    #[test]
    fn old_markers_without_cooling_devices_still_load() {
        let json =
            r#"{"pid":7,"wmi_restore":[],"superio_restore":[{"index":0,"mode":64,"cmd":100}]}"#;
        let m: FanOverrideMarker = serde_json::from_str(json).unwrap();
        assert_eq!(m.pid, 7);
        assert!(m.cooling_devices.is_empty());
        assert!(!m.is_empty());
        let only_cooling = FanOverrideMarker {
            pid: 1,
            cooling_devices: vec!["x:1".into()],
            ..Default::default()
        };
        assert!(
            !only_cooling.is_empty(),
            "a cooling override alone needs the marker"
        );
    }

    use opengear_core::driver::{Connection, DeviceInfo, DeviceKind};
    use opengear_core::fans::{Cooling, FanInfo, FanTarget};
    use std::sync::{Arc, Mutex};

    struct FakeFans {
        id: &'static str,
        restored: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl Cooling for FakeFans {
        fn fans(&self) -> Vec<FanInfo> {
            Vec::new()
        }
        fn set_fan(&mut self, _fan: Option<&str>, _target: FanTarget) -> anyhow::Result<()> {
            Ok(())
        }
        fn overridden(&self) -> bool {
            true
        }
        fn restore_firmware_control(&mut self) -> anyhow::Result<()> {
            anyhow::ensure!(!self.fail, "EC busy");
            self.restored.lock().unwrap().push(self.id.to_string());
            Ok(())
        }
    }

    impl Device for FakeFans {
        fn info(&self) -> DeviceInfo {
            DeviceInfo {
                id: self.id.into(),
                vendor: "Fake".into(),
                model: "Fake".into(),
                kind: DeviceKind::Laptop,
                connection: Connection::Internal,
                firmware: None,
            }
        }
        fn cooling(&mut self) -> Option<&mut dyn Cooling> {
            Some(self)
        }
    }

    struct NoFans;

    impl Device for NoFans {
        fn info(&self) -> DeviceInfo {
            DeviceInfo {
                id: "plain:1".into(),
                vendor: "Fake".into(),
                model: "Plain".into(),
                kind: DeviceKind::Mouse,
                connection: Connection::Usb,
                firmware: None,
            }
        }
    }

    #[test]
    fn only_the_listed_devices_are_restored() {
        let restored = Arc::new(Mutex::new(Vec::new()));
        let fake = |id: &'static str, fail: bool| -> Box<dyn Device> {
            Box::new(FakeFans {
                id,
                restored: restored.clone(),
                fail,
            })
        };
        let mut devices = vec![
            fake("a:1", false),
            fake("b:1", false),
            fake("c:1", true),
            Box::new(NoFans),
        ];
        let ids: Vec<String> = ["b:1", "c:1", "gone:1", "plain:1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let res = restore_cooling_devices(&mut devices, &ids);
        assert_eq!(*restored.lock().unwrap(), ["b:1"], "a:1 was not listed");
        assert!(res[0].1.is_ok());
        assert!(res[1].1.is_err());
        assert!(res[2].1.is_err());
        assert!(res[3].1.is_err());

        // Failed restorations stay in the marker for a later retry.
        let marker = FanOverrideMarker {
            pid: 3,
            cooling_devices: ids.clone(),
            ..Default::default()
        };
        let (logs, rem) = restore_from_marker_with(
            &marker,
            |_| Ok(()),
            |ids| restore_cooling_devices(&mut devices, ids),
        );
        assert_eq!(logs.len(), 4);
        assert_eq!(rem.unwrap().cooling_devices, ["c:1", "gone:1", "plain:1"]);
    }

    struct NamedDriver(&'static str, bool);

    impl Driver for NamedDriver {
        fn name(&self) -> &'static str {
            self.0
        }
        fn experimental(&self) -> bool {
            self.1
        }
        fn discover(&self) -> Vec<Box<dyn Device>> {
            Vec::new()
        }
    }

    #[test]
    fn only_drivers_owning_a_listed_id_are_probed() {
        let drivers: Vec<Box<dyn Driver>> = vec![
            Box::new(NamedDriver("corsair", false)),
            Box::new(NamedDriver("hp_omen", false)),
            Box::new(NamedDriver("razer", true)),
        ];
        let ids = vec![
            "hp_omen:keyboard".to_string(),
            "razer:1532:0001".to_string(),
        ];
        let mut enabled = std::collections::BTreeSet::new();
        let names = |v: Vec<&dyn Driver>| v.iter().map(|d| d.name()).collect::<Vec<_>>();
        assert_eq!(
            names(drivers_for_ids(&drivers, &enabled, &ids)),
            ["hp_omen"]
        );
        enabled.insert("razer".to_string());
        assert_eq!(
            names(drivers_for_ids(&drivers, &enabled, &ids)),
            ["hp_omen", "razer"]
        );
    }
}
