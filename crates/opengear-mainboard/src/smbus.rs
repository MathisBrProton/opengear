//! SMBus access: a transport-neutral [`SmbusBus`] trait, the PawnIO implementation (AMD PIIX4 /
//! Intel I801 host controllers) and the read-only bus diagnostic.
//!
//! Safety rules shared by every implementation:
//! - always hold [`lock_smbus`] (the cross-application SMBus mutex) while talking to the bus;
//! - never touch the SPD / PMIC / thermal address ranges ([`is_diag_excluded_addr`]).

use anyhow::Result;

/// One SMBus segment, whatever carries the transactions (PawnIO, a vendor WMI interface…).
/// Addresses are 7-bit.
pub trait SmbusBus {
    /// SMBus "read byte data".
    fn read_byte_data(&self, addr7: u8, cmd: u8) -> Option<u8>;
    /// SMBus "write byte data".
    fn write_byte_data(&self, addr7: u8, cmd: u8, val: u8) -> Result<()>;
    /// SMBus "write word data" (little-endian on the wire).
    fn write_word_data(&self, addr7: u8, cmd: u8, word: u16) -> Result<()>;
    /// SMBus "receive byte" (no command), used for presence checks. `None` when unsupported.
    fn receive_byte(&self, _addr7: u8) -> Option<u8> {
        None
    }
}

/// Sensitive SMBus 7-bit address ranges excluded even from read-only diagnostic scans:
/// - `0x18..=0x1F`: SMBus thermal sensors
/// - `0x30..=0x37`: DDR4 SPD page-select / write-protect commands
/// - `0x48..=0x4F`: DDR5 PMIC power controllers
/// - `0x50..=0x57`: SPD EEPROM / DDR5 SPD hub
pub fn is_diag_excluded_addr(addr7: u8) -> bool {
    (0x18..=0x1F).contains(&addr7)
        || (0x30..=0x37).contains(&addr7)
        || (0x48..=0x4F).contains(&addr7)
        || (0x50..=0x57).contains(&addr7)
}

/// Hold the cross-application SMBus mutex (HWiNFO / LHM / OpenRGB use the same one).
#[cfg(windows)]
pub fn lock_smbus() -> Option<opengear_platform::named_mutex::NamedMutexGuard> {
    use opengear_platform::named_mutex::{NamedMutexGuard, SMBUS_MUTEX};
    NamedMutexGuard::acquire(SMBUS_MUTEX, 1500)
}

#[cfg(windows)]
mod pawnio_bus {
    use super::SmbusBus;
    use anyhow::Result;
    use opengear_platform::pawnio::PawnIoSession;

    const I2C_SMBUS_WRITE: i64 = 0;
    const I2C_SMBUS_READ: i64 = 1;
    const I2C_SMBUS_BYTE: i64 = 1;
    const I2C_SMBUS_BYTE_DATA: i64 = 2;
    const I2C_SMBUS_WORD_DATA: i64 = 3;

    /// SMBus host controller driven by a PawnIO module.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SmbusController {
        /// AMD FCH (AM4 / AM5): several ports behind one controller.
        Piix4,
        /// Intel ICH / PCH.
        I801,
    }

    impl SmbusController {
        fn module(self) -> &'static str {
            match self {
                Self::Piix4 => "SmbusPIIX4.bin",
                Self::I801 => "SmbusI801.bin",
            }
        }

        /// Tag stored on modules found behind this controller.
        pub fn tag(self) -> &'static str {
            match self {
                Self::Piix4 => "pawnio_piix4",
                Self::I801 => "pawnio_i801",
            }
        }

        pub fn from_tag(tag: &str) -> Option<Self> {
            match tag {
                "pawnio_piix4" => Some(Self::Piix4),
                "pawnio_i801" => Some(Self::I801),
                _ => None,
            }
        }

        pub fn label(self) -> &'static str {
            match self {
                Self::Piix4 => "AMD PIIX4",
                Self::I801 => "Intel I801",
            }
        }
    }

    /// PawnIO SMBus session. On PIIX4 the previously selected port is restored on drop, so other
    /// tools sharing the controller find it as they left it.
    pub struct PawnIoSmbus {
        sess: PawnIoSession,
        controller: SmbusController,
        prev_port: Option<i64>,
    }

    impl PawnIoSmbus {
        pub fn open(controller: SmbusController) -> Result<Self> {
            let sess = PawnIoSession::load_file(controller.module())?;
            let prev_port = (controller == SmbusController::Piix4).then(|| {
                sess.execute("ioctl_piix4_port_sel", &[-1], 1)
                    .ok()
                    .and_then(|v| v.first().copied())
                    .unwrap_or(0)
            });
            Ok(Self {
                sess,
                controller,
                prev_port,
            })
        }

        pub fn controller(&self) -> SmbusController {
            self.controller
        }

        /// Ports to scan on this controller.
        pub fn ports(&self) -> &'static [u16] {
            match self.controller {
                SmbusController::Piix4 => &[0, 1, 2],
                SmbusController::I801 => &[0],
            }
        }

        /// Select a PIIX4 port (no-op on single-port controllers).
        pub fn select_port(&self, port: u16) -> Result<()> {
            if self.controller == SmbusController::Piix4 {
                self.sess
                    .execute("ioctl_piix4_port_sel", &[port as i64], 1)?;
            }
            Ok(())
        }

        /// `(io_base, pci_address)` reported by the module, for diagnostics.
        pub fn identity(&self) -> Option<(i64, i64)> {
            let id = self.sess.execute("ioctl_identity", &[], 3).ok()?;
            Some((
                id.get(1).copied().unwrap_or(0),
                id.get(2).copied().unwrap_or(0),
            ))
        }

        fn xfer(&self, args: &[i64], out_len: usize) -> Result<Vec<i64>> {
            self.sess.execute("ioctl_smbus_xfer", args, out_len)
        }
    }

    impl Drop for PawnIoSmbus {
        fn drop(&mut self) {
            if let Some(port) = self.prev_port {
                let _ = self.sess.execute("ioctl_piix4_port_sel", &[port], 1);
            }
        }
    }

    impl SmbusBus for PawnIoSmbus {
        fn read_byte_data(&self, addr7: u8, cmd: u8) -> Option<u8> {
            let out = self
                .xfer(
                    &[
                        addr7 as i64,
                        I2C_SMBUS_READ,
                        cmd as i64,
                        I2C_SMBUS_BYTE_DATA,
                    ],
                    1,
                )
                .ok()?;
            out.first().map(|&v| (v & 0xFF) as u8)
        }

        fn write_byte_data(&self, addr7: u8, cmd: u8, val: u8) -> Result<()> {
            self.xfer(
                &[
                    addr7 as i64,
                    I2C_SMBUS_WRITE,
                    cmd as i64,
                    I2C_SMBUS_BYTE_DATA,
                    val as i64,
                ],
                0,
            )?;
            Ok(())
        }

        fn write_word_data(&self, addr7: u8, cmd: u8, word: u16) -> Result<()> {
            self.xfer(
                &[
                    addr7 as i64,
                    I2C_SMBUS_WRITE,
                    cmd as i64,
                    I2C_SMBUS_WORD_DATA,
                    word as i64,
                ],
                0,
            )?;
            Ok(())
        }

        fn receive_byte(&self, addr7: u8) -> Option<u8> {
            let out = self
                .xfer(&[addr7 as i64, I2C_SMBUS_READ, 0, I2C_SMBUS_BYTE], 1)
                .ok()?;
            out.first().map(|&v| (v & 0xFF) as u8)
        }
    }
}

#[cfg(windows)]
pub use pawnio_bus::{PawnIoSmbus, SmbusController};

/// Read-only presence scan of every SMBus port reachable through PawnIO, skipping the sensitive
/// ranges. Each line is human-readable diagnostic text.
pub fn diagnose_smbus_readonly() -> Vec<String> {
    #[cfg(windows)]
    {
        use opengear_platform::PawnIoAvailability;
        use opengear_platform::pawnio::pawnio_availability;

        let mut lines = Vec::new();
        match pawnio_availability() {
            PawnIoAvailability::NeedsAdmin => {
                lines.push("Admin UAC requis pour diagnostiquer le bus SMBus PawnIO.".into());
                return lines;
            }
            PawnIoAvailability::NotInstalled => {
                lines.push("Service PawnIO non installé.".into());
                return lines;
            }
            PawnIoAvailability::Ready => {}
        }
        let Some(_lock) = lock_smbus() else {
            lines.push(
                "Bus SMBus occupé par un autre logiciel (HWiNFO, OpenRGB, LHM…) — diagnostic annulé."
                    .into(),
            );
            return lines;
        };
        lines.push(
            "Plages sensibles exclues : 0x18..0x1F (capteurs), 0x30..0x37 (SPD WP), 0x48..0x4F (PMIC DDR5), 0x50..0x57 (SPD)."
                .into(),
        );
        for controller in [SmbusController::Piix4, SmbusController::I801] {
            let bus = match PawnIoSmbus::open(controller) {
                Ok(b) => b,
                Err(e) => {
                    lines.push(format!("{} non chargé : {e}", controller.label()));
                    continue;
                }
            };
            if let Some((io, pci)) = bus.identity() {
                lines.push(format!(
                    "{} chargé : io_base=0x{io:X}, pci=0x{pci:X}",
                    controller.label()
                ));
            }
            for &port in bus.ports() {
                if bus.select_port(port).is_err() {
                    continue;
                }
                let found: Vec<String> = (0x10u8..=0x7F)
                    .filter(|&a| !is_diag_excluded_addr(a))
                    .filter(|&a| {
                        bus.read_byte_data(a, 0x00).is_some() || bus.receive_byte(a).is_some()
                    })
                    .map(|a| {
                        if crate::ene::looks_like_ene(&bus, a) {
                            format!("0x{a:02X}[ENE!]")
                        } else {
                            format!("0x{a:02X}")
                        }
                    })
                    .collect();
                lines.push(format!(
                    "  {} port {port} -> {} adresse(s): [{}]",
                    controller.label(),
                    found.len(),
                    found.join(", ")
                ));
            }
        }
        lines
    }
    #[cfg(not(windows))]
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::is_diag_excluded_addr;

    #[test]
    fn spd_pmic_and_thermal_ranges_are_never_probed() {
        for a in 0x18u8..=0x1F {
            assert!(is_diag_excluded_addr(a), "thermal 0x{a:02X}");
        }
        for a in 0x30u8..=0x37 {
            assert!(is_diag_excluded_addr(a), "spd-wp 0x{a:02X}");
        }
        for a in 0x48u8..=0x4F {
            assert!(is_diag_excluded_addr(a), "pmic 0x{a:02X}");
        }
        for a in 0x50u8..=0x57 {
            assert!(is_diag_excluded_addr(a), "spd 0x{a:02X}");
        }
        for a in [0x2Fu8, 0x38, 0x39, 0x58, 0x70, 0x77] {
            assert!(!is_diag_excluded_addr(a), "0x{a:02X}");
        }
    }
}
