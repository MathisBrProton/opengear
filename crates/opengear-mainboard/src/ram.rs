//! RAM inventory and RAM RGB through pluggable backends.
//!
//! A [`RamRgbBackend`] reaches the RGB controllers of the memory modules one way (PawnIO SMBus in
//! this crate, ASUS WMI in `opengear-asus`, the OpenRGB SDK in `opengear-openrgb`…). [`RamRgb`]
//! tries them in order and routes every later command to the backend that found each module.
//! Adding a transport = implementing the trait and registering it in the app.

pub use crate::ene::{EneRamMode, EneRamModule, RamEffect};
use anyhow::Result;
use opengear_platform::{PawnIoAvailability, is_elevated, pawnio::pawnio_availability};
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
pub struct RamRgbStatus {
    pub is_admin: bool,
    /// At least one backend could reach the bus (modules found or not).
    pub bus_available: bool,
    #[serde(default)]
    pub pawnio_status: String,
    pub physical_dimms: Vec<PhysicalDimmInfo>,
    pub ene_modules: Vec<EneRamModule>,
    pub message: String,
    pub last_mode: String,
    pub last_color: String,
    pub last_speed: u8,
}

/// Result of one backend probe.
#[derive(Debug, Default)]
pub struct RamProbe {
    /// The backend could reach its bus / service.
    pub available: bool,
    pub modules: Vec<EneRamModule>,
}

/// One way of reaching RAM RGB controllers.
pub trait RamRgbBackend: Send + Sync {
    /// Short name shown to the user (`"PawnIO"`, `"ASUS WMI"`…).
    fn label(&self) -> &'static str;
    /// Whether this backend drives modules tagged `backend` (see [`EneRamModule::backend`]).
    fn handles(&self, backend: &str) -> bool;
    /// Needs administrator rights (skipped otherwise).
    fn requires_admin(&self) -> bool;
    fn probe(&self) -> RamProbe;
    /// Apply an effect; returns a short status line.
    fn apply(&self, modules: &[&EneRamModule], effect: &RamEffect) -> Result<String>;
    /// Switch modules to raw ENE modes; returns how many were updated.
    fn set_modes(&self, modules: &[(&EneRamModule, u8)]) -> Result<usize>;
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
        let text = |row: &std::collections::BTreeMap<String, opengear_platform::wmi::WmiVal>,
                    key: &str| {
            row.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        rows.iter()
            .enumerate()
            .map(|(idx, row)| {
                let raw_slot = text(row, "DeviceLocator");
                let slot = if raw_slot.is_empty() || raw_slot == "DIMM 1" {
                    format!("DIMM #{}", idx + 1)
                } else {
                    raw_slot
                };
                let cap_bytes = row.get("Capacity").and_then(|v| v.as_u64()).unwrap_or(0);
                let memory_type = match row
                    .get("SMBIOSMemoryType")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                {
                    26 => "DDR4".to_string(),
                    34 => "DDR5".to_string(),
                    other => format!("Type-{other}"),
                };
                PhysicalDimmInfo {
                    slot,
                    manufacturer: text(row, "Manufacturer"),
                    part_number: text(row, "PartNumber"),
                    capacity_gb: ((cap_bytes as f64) / (1024.0 * 1024.0 * 1024.0)).round() as u32,
                    speed_mts: row
                        .get("ConfiguredClockSpeed")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    memory_type,
                }
            })
            .collect()
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// RAM RGB front-end over an ordered list of backends.
pub struct RamRgb {
    backends: Vec<Box<dyn RamRgbBackend>>,
}

fn describe(modules: &[EneRamModule]) -> String {
    modules
        .iter()
        .map(|m| {
            format!(
                "0x{:02X} {} [{} LEDs, {}]",
                m.address_7bit, m.firmware, m.led_count, m.backend
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl RamRgb {
    /// Backends are tried in this order; the first one that finds modules wins the probe.
    pub fn new(backends: Vec<Box<dyn RamRgbBackend>>) -> Self {
        Self { backends }
    }

    pub fn probe(&self) -> RamRgbStatus {
        let admin = is_elevated();
        let pio = pawnio_availability();
        let mut status = RamRgbStatus {
            is_admin: admin,
            bus_available: false,
            pawnio_status: match pio {
                PawnIoAvailability::Ready => "ready",
                PawnIoAvailability::NeedsAdmin => "needs_admin",
                PawnIoAvailability::NotInstalled => "not_installed",
            }
            .to_string(),
            physical_dimms: read_physical_dimms(),
            ene_modules: Vec::new(),
            message: String::new(),
            last_mode: "Static".into(),
            last_color: "#00D2FF".into(),
            last_speed: 2,
        };
        let mut unavailable = Vec::new();
        for backend in self
            .backends
            .iter()
            .filter(|b| admin || !b.requires_admin())
        {
            let probe = backend.probe();
            status.bus_available |= probe.available;
            if !probe.available {
                unavailable.push(backend.label());
            }
            if !probe.modules.is_empty() {
                status.message = format!(
                    "{} barrette(s) DDR RGB détectée(s) via {} ({}).",
                    probe.modules.len(),
                    backend.label(),
                    describe(&probe.modules)
                );
                status.ene_modules = probe.modules;
                return status;
            }
        }
        status.message = if !admin && pio == PawnIoAvailability::NeedsAdmin {
            "Pilote PawnIO détecté : relance OpenGear en administrateur pour piloter le RGB de ta RAM et lire la température CPU en natif.".into()
        } else if !admin {
            "Lance OpenGear en administrateur avec le pilote PawnIO pour piloter le RGB de la RAM, ou lance OpenRGB (serveur SDK, port 6742) sans droits admin.".into()
        } else if unavailable.is_empty() {
            "Aucun contrôleur RGB de RAM n'a répondu.".into()
        } else {
            format!(
                "Aucun contrôleur RGB de RAM n'a répondu (indisponible : {}).",
                unavailable.join(", ")
            )
        };
        status
    }

    fn backend_for(&self, tag: &str) -> Option<&dyn RamRgbBackend> {
        self.backends
            .iter()
            .find(|b| b.handles(tag))
            .map(|b| b.as_ref())
    }

    fn check_rights(&self, backend: &dyn RamRgbBackend) -> Result<()> {
        if backend.requires_admin() && !is_elevated() {
            anyhow::bail!(
                "Relance OpenGear en administrateur pour piloter la RAM via {}.",
                backend.label()
            );
        }
        Ok(())
    }

    /// Apply `effect` to every module (or only the one at `target_addr_7bit`).
    pub fn apply(
        &self,
        modules: &[EneRamModule],
        target_addr_7bit: Option<u8>,
        effect: &RamEffect,
    ) -> Result<String> {
        let targets: Vec<&EneRamModule> = modules
            .iter()
            .filter(|m| target_addr_7bit.is_none_or(|a| a == m.address_7bit))
            .collect();
        if targets.is_empty() {
            anyhow::bail!(
                "Aucune barrette RGB détectée à piloter (clique d'abord sur Scanner la RAM)."
            );
        }
        let mut results = Vec::new();
        for (backend, group) in self.group(&targets)? {
            self.check_rights(backend)?;
            results.push(backend.apply(&group, effect)?);
        }
        Ok(results.join(" + "))
    }

    /// Switch modules to raw modes (lights off / restore); returns how many were updated.
    pub fn set_modes(&self, modes: &[(&EneRamModule, u8)]) -> Result<usize> {
        let mut applied = 0;
        for backend in &self.backends {
            let mine: Vec<(&EneRamModule, u8)> = modes
                .iter()
                .filter(|(m, _)| backend.handles(&m.backend))
                .copied()
                .collect();
            if mine.is_empty() {
                continue;
            }
            self.check_rights(backend.as_ref())?;
            applied += backend.set_modes(&mine)?;
        }
        Ok(applied)
    }

    /// Group targets by owning backend, in backend order.
    fn group<'a>(
        &self,
        targets: &[&'a EneRamModule],
    ) -> Result<Vec<(&dyn RamRgbBackend, Vec<&'a EneRamModule>)>> {
        if let Some(orphan) = targets
            .iter()
            .find(|m| self.backend_for(&m.backend).is_none())
        {
            anyhow::bail!(
                "Aucun pilote pour la barrette gérée par « {} »",
                orphan.backend
            );
        }
        Ok(self
            .backends
            .iter()
            .filter_map(|b| {
                let mine: Vec<&EneRamModule> = targets
                    .iter()
                    .copied()
                    .filter(|m| b.handles(&m.backend))
                    .collect();
                (!mine.is_empty()).then_some((b.as_ref(), mine))
            })
            .collect())
    }
}

/// ENE modules on the motherboard SMBus through PawnIO (AMD PIIX4 / Intel I801).
pub struct PawnIoRamBackend;

#[cfg(windows)]
mod pawnio_backend {
    use super::*;
    use crate::ene;
    use crate::smbus::{PawnIoSmbus, SmbusController, lock_smbus};
    use anyhow::Context;

    pub(super) fn probe() -> RamProbe {
        if pawnio_availability() != PawnIoAvailability::Ready {
            return RamProbe::default();
        }
        let Some(_lock) = lock_smbus() else {
            return RamProbe::default();
        };
        let mut available = false;
        for controller in [SmbusController::Piix4, SmbusController::I801] {
            let Ok(bus) = PawnIoSmbus::open(controller) else {
                continue;
            };
            available = true;
            for &port in bus.ports() {
                if bus.select_port(port).is_err() {
                    continue;
                }
                ene::remap_from_0x77(&bus);
                let modules = ene::scan(&bus, ene::ENE_RAM_ADDRESSES, port, controller.tag());
                if !modules.is_empty() {
                    return RamProbe { available, modules };
                }
            }
        }
        RamProbe {
            available,
            modules: Vec::new(),
        }
    }

    /// Run `f` on each module's controller/port, under the SMBus lock.
    fn for_each_module<T>(
        modules: &[T],
        module_of: impl Fn(&T) -> &EneRamModule,
        mut f: impl FnMut(&PawnIoSmbus, &T) -> Result<bool>,
    ) -> Result<(usize, Vec<String>, Vec<SmbusController>)> {
        let _lock = lock_smbus().context("Bus SMBus occupé par un autre logiciel")?;
        let mut applied = 0;
        let mut skipped = Vec::new();
        let mut used = Vec::new();
        for controller in [SmbusController::Piix4, SmbusController::I801] {
            let mine: Vec<&T> = modules
                .iter()
                .filter(|t| SmbusController::from_tag(&module_of(t).backend) == Some(controller))
                .collect();
            if mine.is_empty() {
                continue;
            }
            let bus = PawnIoSmbus::open(controller)?;
            used.push(controller);
            for t in mine {
                let m = module_of(t);
                if bus.select_port(m.bus_arg).is_err() {
                    skipped.push(format!(
                        "0x{:02X} (port {} inaccessible)",
                        m.address_7bit, m.bus_arg
                    ));
                    continue;
                }
                if f(&bus, t)? {
                    applied += 1;
                } else {
                    skipped.push(format!("0x{:02X} (empreinte ENE absente)", m.address_7bit));
                }
            }
        }
        Ok((applied, skipped, used))
    }

    pub(super) fn apply(modules: &[&EneRamModule], effect: &RamEffect) -> Result<String> {
        let (applied, skipped, used) =
            for_each_module(modules, |m| *m, |bus, m| ene::apply(bus, m, effect))?;
        if applied == 0 {
            anyhow::bail!(
                "Aucune barrette ENE vérifiée — aucune écriture effectuée ({})",
                skipped.join(", ")
            );
        }
        Ok(format!(
            "OK PawnIO {} ({applied} DIMM{})",
            used.iter()
                .map(|c| c.label())
                .collect::<Vec<_>>()
                .join(" + "),
            if effect.save_eeprom {
                ", EEPROM sauvée"
            } else {
                ""
            }
        ))
    }

    pub(super) fn set_modes(modes: &[(&EneRamModule, u8)]) -> Result<usize> {
        let (applied, _, _) = for_each_module(
            modes,
            |t| t.0,
            |bus, (m, mode)| ene::set_mode(bus, m, *mode),
        )?;
        Ok(applied)
    }
}

impl RamRgbBackend for PawnIoRamBackend {
    fn label(&self) -> &'static str {
        "PawnIO"
    }

    fn handles(&self, backend: &str) -> bool {
        backend.starts_with("pawnio_")
    }

    fn requires_admin(&self) -> bool {
        true
    }

    fn probe(&self) -> RamProbe {
        #[cfg(windows)]
        {
            pawnio_backend::probe()
        }
        #[cfg(not(windows))]
        RamProbe::default()
    }

    fn apply(&self, modules: &[&EneRamModule], effect: &RamEffect) -> Result<String> {
        #[cfg(windows)]
        {
            pawnio_backend::apply(modules, effect)
        }
        #[cfg(not(windows))]
        {
            let _ = (modules, effect);
            anyhow::bail!("Non supporté hors Windows")
        }
    }

    fn set_modes(&self, modules: &[(&EneRamModule, u8)]) -> Result<usize> {
        #[cfg(windows)]
        {
            pawnio_backend::set_modes(modules)
        }
        #[cfg(not(windows))]
        {
            let _ = modules;
            anyhow::bail!("Non supporté hors Windows")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opengear_core::Rgb;
    use std::sync::Mutex;

    struct FakeBackend {
        tag: &'static str,
        admin: bool,
        modules: Vec<EneRamModule>,
        calls: Mutex<Vec<String>>,
    }

    fn module(addr: u8, backend: &str) -> EneRamModule {
        EneRamModule {
            address_7bit: addr,
            bus_arg: 0,
            firmware: "AUDA0".into(),
            is_v2: true,
            led_count: 8,
            current_mode: 1,
            backend: backend.into(),
        }
    }

    impl RamRgbBackend for FakeBackend {
        fn label(&self) -> &'static str {
            self.tag
        }
        fn handles(&self, backend: &str) -> bool {
            backend == self.tag
        }
        fn requires_admin(&self) -> bool {
            self.admin
        }
        fn probe(&self) -> RamProbe {
            RamProbe {
                available: true,
                modules: self.modules.clone(),
            }
        }
        fn apply(&self, modules: &[&EneRamModule], _: &RamEffect) -> Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("apply:{}", modules.len()));
            Ok(self.tag.to_string())
        }
        fn set_modes(&self, modules: &[(&EneRamModule, u8)]) -> Result<usize> {
            Ok(modules.len())
        }
    }

    fn fake(tag: &'static str, admin: bool, modules: Vec<EneRamModule>) -> Box<dyn RamRgbBackend> {
        Box::new(FakeBackend {
            tag,
            admin,
            modules,
            calls: Mutex::new(Vec::new()),
        })
    }

    #[test]
    fn commands_are_routed_to_the_backend_that_found_each_module() {
        let ram = RamRgb::new(vec![
            fake("a", false, vec![]),
            fake("b", false, vec![module(0x71, "b")]),
        ]);
        let effect = RamEffect {
            mode: EneRamMode::Static,
            color: Rgb::new(1, 2, 3),
            speed: 2,
            reverse: false,
            save_eeprom: false,
        };
        let modules = vec![module(0x70, "a"), module(0x71, "b"), module(0x72, "b")];
        assert_eq!(ram.apply(&modules, None, &effect).unwrap(), "a + b");
        assert_eq!(ram.apply(&modules, Some(0x72), &effect).unwrap(), "b");
        let pairs: Vec<(&EneRamModule, u8)> = modules.iter().map(|m| (m, 0)).collect();
        assert_eq!(ram.set_modes(&pairs).unwrap(), 3);
        assert!(ram.apply(&[module(0x70, "zzz")], None, &effect).is_err());
        assert!(ram.apply(&[], None, &effect).is_err());
    }

    #[test]
    fn first_backend_with_modules_wins_the_probe() {
        let ram = RamRgb::new(vec![
            fake("a", false, vec![]),
            fake("b", false, vec![module(0x71, "b")]),
            fake("c", false, vec![module(0x72, "c")]),
        ]);
        let st = ram.probe();
        assert_eq!(st.ene_modules.len(), 1);
        assert_eq!(st.ene_modules[0].backend, "b");
        assert!(st.bus_available);
        assert!(st.message.contains("via b"));
    }
}
