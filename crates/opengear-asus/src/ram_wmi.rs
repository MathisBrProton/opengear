//! RAM RGB through the ASUS `ASUSManagement` WMI SMBus methods (ASUS motherboards, admin).
//!
//! Only the transport is ASUS-specific: the ENE register protocol comes from
//! `opengear_mainboard::ene`.

use anyhow::Result;
use opengear_mainboard::ram::{EneRamModule, RamEffect, RamProbe, RamRgbBackend};
use tracing::{debug, info, warn};

const TAG: &str = "asus_wmi";

/// RAM RGB backend for ASUS boards exposing SMBus access in `root\wmi\ASUSManagement`.
pub struct AsusWmiRamBackend;

#[cfg(windows)]
mod imp {
    use super::*;
    use anyhow::Context;
    use opengear_mainboard::ene;
    use opengear_mainboard::smbus::{SmbusBus, lock_smbus};
    use opengear_platform::wmi::{WmiConn, WmiVal};

    /// ASUS WMI SMBus. Depending on the board the `slave` argument is the 7-bit address or the
    /// 8-bit (shifted) one; `shift` records which one this board expects.
    pub(super) struct AsusWmiSmbus {
        conn: WmiConn,
        obj_path: String,
        shift: u32,
    }

    impl AsusWmiSmbus {
        pub(super) fn open(shift: u32) -> Result<Self> {
            let conn = WmiConn::connect(r"root\wmi")?;
            let obj_path = conn
                .first_instance_path("ASUSManagement")?
                .context("ASUSManagement WMI introuvable")?;
            Ok(Self {
                conn,
                obj_path,
                shift,
            })
        }

        fn call(&self, method: &str, addr7: u8, cmd: u8, data: Option<u32>) -> Result<u64> {
            let slave = (addr7 as u32) << self.shift;
            let mut args = vec![
                ("slave", WmiVal::U32(slave)),
                ("cmd", WmiVal::U32(cmd as u32)),
            ];
            if let Some(d) = data {
                args.push(("data", WmiVal::U32(d)));
            }
            let res = self
                .conn
                .exec_method(&self.obj_path, "ASUSManagement", method, &args)?;
            let code = res.get("Result").and_then(|v| v.as_u64()).unwrap_or(1);
            if method == "read_smbus_byte" {
                // 0 and 1 both mean "data valid" for reads.
                anyhow::ensure!(code <= 1, "ASUSManagement {method} failed (Result={code})");
                return res
                    .get("data")
                    .and_then(|v| v.as_u64())
                    .context("ASUSManagement read without data");
            }
            anyhow::ensure!(code == 0, "ASUSManagement {method} failed (Result={code})");
            Ok(0)
        }
    }

    impl SmbusBus for AsusWmiSmbus {
        fn read_byte_data(&self, addr7: u8, cmd: u8) -> Option<u8> {
            self.call("read_smbus_byte", addr7, cmd, None)
                .ok()
                .map(|d| (d & 0xFF) as u8)
        }

        fn write_byte_data(&self, addr7: u8, cmd: u8, val: u8) -> Result<()> {
            self.call("write_smbus_byte", addr7, cmd, Some(val as u32))
                .map(|_| ())
        }

        fn write_word_data(&self, addr7: u8, cmd: u8, word: u16) -> Result<()> {
            self.call("write_smbus_word", addr7, cmd, Some(word as u32))
                .map(|_| ())
        }
    }

    /// Addresses ENE controllers use on ASUS boards (no remapping through this interface).
    const ADDRESSES: [u8; 8] = [0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77];

    pub(super) fn probe() -> RamProbe {
        let Some(_lock) = lock_smbus() else {
            warn!("Mutex SMBus occupé : sonde ASUS WMI RAM ignorée");
            return RamProbe::default();
        };
        let mut probe = RamProbe::default();
        for shift in [0u32, 1] {
            let bus = match AsusWmiSmbus::open(shift) {
                Ok(b) => b,
                Err(err) => {
                    debug!(shift, ?err, "Interface ASUSManagement WMI SMBus absente");
                    return probe;
                }
            };
            probe.available = true;
            let modules = ene::scan(&bus, &ADDRESSES, shift as u16, TAG);
            info!(shift, found = modules.len(), "Scan RAM via ASUS WMI SMBus");
            if !modules.is_empty() {
                probe.modules = modules;
                break;
            }
        }
        probe
    }

    /// Run `f` on every module grouped by bus variant (shift).
    pub(super) fn for_each<T>(
        items: &[T],
        module_of: impl Fn(&T) -> &EneRamModule,
        mut f: impl FnMut(&AsusWmiSmbus, &T) -> Result<bool>,
    ) -> Result<usize> {
        let _lock = lock_smbus().context("Bus SMBus occupé par un autre logiciel")?;
        let mut applied = 0;
        for shift in [0u32, 1] {
            let mine: Vec<&T> = items
                .iter()
                .filter(|t| module_of(t).bus_arg as u32 == shift)
                .collect();
            if mine.is_empty() {
                continue;
            }
            let bus = AsusWmiSmbus::open(shift)?;
            for t in mine {
                if f(&bus, t)? {
                    applied += 1;
                }
            }
        }
        Ok(applied)
    }

    pub(super) fn apply_all(modules: &[&EneRamModule], effect: &RamEffect) -> Result<usize> {
        let _lock = lock_smbus().context("Bus SMBus occupé par un autre logiciel")?;
        let mut applied = 0;
        for shift in [0u32, 1] {
            let mine: Vec<&EneRamModule> = modules
                .iter()
                .copied()
                .filter(|m| m.bus_arg as u32 == shift)
                .collect();
            if mine.is_empty() {
                continue;
            }
            let bus = AsusWmiSmbus::open(shift)?;
            applied += ene::apply_all(&bus, &mine, effect)?;
        }
        Ok(applied)
    }
}

impl RamRgbBackend for AsusWmiRamBackend {
    fn label(&self) -> &'static str {
        "ASUS WMI"
    }

    fn handles(&self, backend: &str) -> bool {
        backend == TAG
    }

    fn requires_admin(&self) -> bool {
        true
    }

    fn probe(&self) -> RamProbe {
        #[cfg(windows)]
        {
            imp::probe()
        }
        #[cfg(not(windows))]
        RamProbe::default()
    }

    fn apply(&self, modules: &[&EneRamModule], effect: &RamEffect) -> Result<String> {
        #[cfg(windows)]
        {
            let applied = imp::apply_all(modules, effect)?;
            anyhow::ensure!(
                applied > 0 && applied == modules.len(),
                "Écriture ASUS WMI RAM RGB incomplète : {applied}/{} barrette(s) vérifiée(s)",
                modules.len()
            );
            Ok(format!("OK ({applied}/{} DIMM)", modules.len()))
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
            imp::for_each(
                modules,
                |t| t.0,
                |bus, (m, mode)| opengear_mainboard::ene::set_mode(bus, m, *mode),
            )
        }
        #[cfg(not(windows))]
        {
            let _ = modules;
            anyhow::bail!("Non supporté hors Windows")
        }
    }

    fn apply_direct(
        &self,
        frames: &[(&EneRamModule, &[opengear_core::Rgb], bool)],
    ) -> Result<Option<usize>> {
        #[cfg(windows)]
        {
            let n = imp::for_each(
                frames,
                |t| t.0,
                |bus, (m, cols, enter)| opengear_mainboard::ene::apply_direct(bus, m, cols, *enter),
            )?;
            Ok(Some(n))
        }
        #[cfg(not(windows))]
        {
            let _ = frames;
            anyhow::bail!("Non supporté hors Windows")
        }
    }
}
