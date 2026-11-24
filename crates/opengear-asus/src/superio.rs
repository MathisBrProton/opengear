//! Universal Nuvoton SuperIO (`NCT6791D`..`NCT6799D`, `NCT6701D`, `NCT5585D`) live Fan RPM & PWM
//! controller via Microsoft WHQL-signed **PawnIO** (`LpcIO.bin`).
//!
//! Works on ANY modern ASUS / MSI / ASRock / Gigabyte motherboard equipped with a Nuvoton SuperIO
//! chip — reads live RPMs & PWM % without LibreHardwareMonitor, and applies verified live PWM duty!

use crate::cooling::is_elevated;
use crate::pawnio::{NamedMutexGuard, PawnIoSession, pawnio_availability, PawnIoAvailability};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

const MOD_LPC_IO: &[u8] = include_bytes!("../pawnio_modules/LpcIO.bin");

// Standard Nuvoton NCT679xD hardware monitor registers (verified against LibreHardwareMonitor Nct677X.cs)
const BANK_SELECT_REG: u8 = 0x4E;
const VENDOR_ID_HIGH_REG: u16 = 0x804F;
const VENDOR_ID_LOW_REG: u16 = 0x004F;
const NUVOTON_VENDOR_ID: u16 = 0x5CA3;

const FAN_RPM_REGS: [u16; 7] = [0x4B0, 0x4B2, 0x4B4, 0x4B6, 0x4B8, 0x4BA, 0x4CC];
const FAN_PWM_OUT_REGS_98: [u16; 7] = [0x001, 0x003, 0x011, 0x013, 0x015, 0xA09, 0xB09];
const FAN_PWM_OUT_REGS_95: [u16; 7] = [0x001, 0x003, 0x011, 0x013, 0x015, 0x017, 0x029];
const FAN_PWM_CMD_REGS: [u16; 7] = [0x109, 0x209, 0x309, 0x809, 0x909, 0xA09, 0xB09];
const FAN_CTRL_MODE_REGS: [u16; 7] = [0x102, 0x202, 0x302, 0x802, 0x902, 0xA02, 0xB02];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperIoFanChannel {
    /// 0-based SuperIO channel index (0..7)
    pub index: usize,
    pub label: String,
    pub rpm: u32,
    pub pwm_pct: u8,
    /// Raw control mode byte (0 = Manual, non-zero = SmartFan / BIOS auto)
    pub mode_raw: u8,
    pub is_manual: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuperIoSnapshot {
    pub detected: bool,
    pub chip_name: String,
    pub chip_id_hex: String,
    pub hwm_base_hex: String,
    pub fans: Vec<SuperIoFanChannel>,
    pub diagnostic: String,
}

#[derive(Debug, Clone, Copy)]
struct DetectedChip {
    slot: i64,
    hwm_base: u16,
    chip_id: u16,
    chip_name: &'static str,
    fan_count: usize,
    use_98_pwm_out: bool,
}

/// Minimum manual PWM duty OpenGear will ever write to a SuperIO header.
///
/// The header→fan mapping (CPU_FAN, AIO_PUMP, CHA_FAN…) is board specific and unknown here, so a
/// universal floor prevents stopping a CPU fan or an AIO pump by mistake. Fan-stop (0 %) remains
/// available through the BIOS SmartFan curves (restore to BIOS Auto).
pub const SUPERIO_MIN_PWM_PCT: u8 = 30;

/// Clamp a requested manual duty into the safe range `SUPERIO_MIN_PWM_PCT..=100`.
pub fn safe_superio_duty(duty_pct: u8) -> u8 {
    duty_pct.clamp(SUPERIO_MIN_PWM_PCT, 100)
}

/// Original BIOS state of a channel (control mode + PWM command), saved on first manual write
/// so it can be restored byte-for-byte (same approach as LibreHardwareMonitor `Nct677X`).
#[derive(Debug, Clone, Copy)]
struct SavedFanState {
    mode: u8,
    cmd: u8,
}

/// Serializable copy of a channel's original BIOS state, so another process (the fan watchdog)
/// can restore it if OpenGear dies while the channel is in manual mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperIoSavedState {
    pub index: usize,
    pub mode: u8,
    pub cmd: u8,
}

/// Original BIOS states of every channel currently overridden by OpenGear.
pub fn saved_superio_states() -> Vec<SuperIoSavedState> {
    saved_states()
        .lock()
        .map(|g| {
            (0..7)
                .filter_map(|i| {
                    g[i].map(|s| SuperIoSavedState {
                        index: i,
                        mode: s.mode,
                        cmd: s.cmd,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

static SAVED_INITIAL_STATES: OnceLock<Mutex<[Option<SavedFanState>; 7]>> = OnceLock::new();

fn saved_states() -> &'static Mutex<[Option<SavedFanState>; 7]> {
    SAVED_INITIAL_STATES.get_or_init(|| Mutex::new([None; 7]))
}

fn identify_nuvoton(id: u16) -> Option<(&'static str, usize, bool)> {
    match id & 0xFFF0 {
        0xC800 => Some(("Nuvoton NCT6791D", 6, false)),
        0xC910 => Some(("Nuvoton NCT6792D", 6, false)),
        0xD120 => Some(("Nuvoton NCT6793D/NCT6795D", 6, false)),
        0xD350 => Some(("Nuvoton NCT6796D", 6, false)),
        0xD420 => {
            if id == 0xD42B || id == 0xD428 {
                Some(("Nuvoton NCT6798D", 7, true))
            } else {
                Some(("Nuvoton NCT6796D-R/S", 7, true))
            }
        }
        0xD450 => Some(("Nuvoton NCT6797D", 7, true)),
        0xD800 => Some(("Nuvoton NCT6799D", 7, true)),
        0xD590 => Some(("Nuvoton NCT6796D-S", 7, true)),
        _ => None,
    }
}

#[cfg(windows)]
mod win_sio {
    use super::*;

    fn pio_outb(s: &PawnIoSession, port: u16, val: u8) -> Result<()> {
        s.execute("ioctl_pio_outb", &[port as i64, val as i64], 0)?;
        Ok(())
    }

    fn pio_inb(s: &PawnIoSession, port: u16) -> Result<u8> {
        let out = s.execute("ioctl_pio_inb", &[port as i64], 1)?;
        Ok((out.first().copied().unwrap_or(0) & 0xFF) as u8)
    }

    fn sio_inb(s: &PawnIoSession, reg: u8) -> Result<u8> {
        let out = s.execute("ioctl_superio_inb", &[reg as i64], 1)?;
        Ok((out.first().copied().unwrap_or(0) & 0xFF) as u8)
    }

    fn sio_inw(s: &PawnIoSession, reg: u8) -> Result<u16> {
        let out = s.execute("ioctl_superio_inw", &[reg as i64], 1)?;
        Ok((out.first().copied().unwrap_or(0) & 0xFFFF) as u16)
    }

    fn sio_outb(s: &PawnIoSession, reg: u8, val: u8) -> Result<()> {
        s.execute("ioctl_superio_outb", &[reg as i64, val as i64], 0)?;
        Ok(())
    }

    fn enter_nuvoton(s: &PawnIoSession, reg_port: u16) -> Result<()> {
        pio_outb(s, reg_port, 0x87)?;
        pio_outb(s, reg_port, 0x87)?;
        Ok(())
    }

    fn exit_nuvoton(s: &PawnIoSession, reg_port: u16) {
        let _ = pio_outb(s, reg_port, 0xAA);
    }

    fn hwm_read(s: &PawnIoSession, base: u16, addr: u16) -> Result<u8> {
        let bank = (addr >> 8) as u8;
        let reg = (addr & 0xFF) as u8;
        pio_outb(s, base + 5, BANK_SELECT_REG)?;
        pio_outb(s, base + 6, bank)?;
        pio_outb(s, base + 5, reg)?;
        pio_inb(s, base + 6)
    }

    fn hwm_write(s: &PawnIoSession, base: u16, addr: u16, val: u8) -> Result<()> {
        let bank = (addr >> 8) as u8;
        let reg = (addr & 0xFF) as u8;
        pio_outb(s, base + 5, BANK_SELECT_REG)?;
        pio_outb(s, base + 6, bank)?;
        pio_outb(s, base + 5, reg)?;
        pio_outb(s, base + 6, val)
    }

    fn verify_nuvoton_vendor(s: &PawnIoSession, base: u16) -> bool {
        let hi = hwm_read(s, base, VENDOR_ID_HIGH_REG).unwrap_or(0);
        let lo = hwm_read(s, base, VENDOR_ID_LOW_REG).unwrap_or(0);
        (((hi as u16) << 8) | (lo as u16)) == NUVOTON_VENDOR_ID
    }

    fn detect_and_prepare() -> Result<(PawnIoSession, DetectedChip)> {
        let sess = PawnIoSession::load(MOD_LPC_IO)?;
        for &(slot, reg_port) in &[(0i64, 0x2Eu16), (1i64, 0x4Eu16)] {
            if sess.execute("ioctl_select_slot", &[slot], 0).is_err() {
                continue;
            }
            if enter_nuvoton(&sess, reg_port).is_err() {
                continue;
            }
            let chip_id = sio_inw(&sess, 0x20).unwrap_or(0xFFFF);
            if let Some((chip_name, fan_count, use_98_pwm_out)) = identify_nuvoton(chip_id) {
                // Unlock Hardware Monitor BARs inside PawnIO LpcIO whitelist
                let _ = sess.execute("ioctl_find_bars", &[], 0);
                // Select LDN 0x0B (Hardware Monitor) and read BAR at 0x60/0x61
                let _ = sio_outb(&sess, 0x07, 0x0B);
                let hwm_base = sio_inw(&sess, 0x60).unwrap_or(0);
                exit_nuvoton(&sess, reg_port);

                if hwm_base >= 0x100 && hwm_base != 0xFFFF {
                    // Check Vendor ID; if IO space lock is set on ASUS/Nuvoton, clear bit 4 of reg 0x28
                    if !verify_nuvoton_vendor(&sess, hwm_base) {
                        if enter_nuvoton(&sess, reg_port).is_ok() {
                            let _ = sio_outb(&sess, 0x07, 0x0B);
                            if let Ok(opt) = sio_inb(&sess, 0x28) {
                                if (opt & 0x10) != 0 {
                                    let _ = sio_outb(&sess, 0x28, opt & !0x10);
                                }
                            }
                            exit_nuvoton(&sess, reg_port);
                        }
                    }

                    if verify_nuvoton_vendor(&sess, hwm_base) {
                        return Ok((
                            sess,
                            DetectedChip {
                                slot,
                                hwm_base,
                                chip_id,
                                chip_name,
                                fan_count,
                                use_98_pwm_out,
                            },
                        ));
                    }
                }
            } else {
                exit_nuvoton(&sess, reg_port);
            }
        }
        anyhow::bail!("Aucune puce SuperIO Nuvoton NCT679xD détectée sur 0x2E/0x4E")
    }

    pub fn read_snapshot() -> SuperIoSnapshot {
        if !is_elevated() {
            return SuperIoSnapshot {
                diagnostic: "Admin UAC requis pour lire le SuperIO via PawnIO LpcIO.".into(),
                ..Default::default()
            };
        }
        if pawnio_availability() != PawnIoAvailability::Ready {
            return SuperIoSnapshot {
                diagnostic: "Pilote signé PawnIO non disponible.".into(),
                ..Default::default()
            };
        }

        let _lock = NamedMutexGuard::acquire(r"Global\Access_ISABUS.HTP.Method", 1000);
        let (sess, chip) = match detect_and_prepare() {
            Ok(v) => v,
            Err(e) => {
                return SuperIoSnapshot {
                    diagnostic: e.to_string(),
                    ..Default::default()
                };
            }
        };

        let pwm_out_regs = if chip.use_98_pwm_out {
            &FAN_PWM_OUT_REGS_98
        } else {
            &FAN_PWM_OUT_REGS_95
        };

        let mut fans = Vec::new();
        for i in 0..chip.fan_count {
            let hi = hwm_read(&sess, chip.hwm_base, FAN_RPM_REGS[i]).unwrap_or(0) as u32;
            let lo = hwm_read(&sess, chip.hwm_base, FAN_RPM_REGS[i] + 1).unwrap_or(0) as u32;
            let raw_rpm = (hi << 8) | lo;
            let rpm = if raw_rpm >= 100 && raw_rpm < 15000 {
                raw_rpm
            } else {
                0
            };

            let pwm_raw = hwm_read(&sess, chip.hwm_base, pwm_out_regs[i]).unwrap_or(0);
            let pwm_pct = ((pwm_raw as u32 * 100 + 127) / 255).min(100) as u8;
            let mode_raw = hwm_read(&sess, chip.hwm_base, FAN_CTRL_MODE_REGS[i]).unwrap_or(0xFF);

            fans.push(SuperIoFanChannel {
                index: i,
                label: format!("Fan Header #{}", i + 1),
                rpm,
                pwm_pct,
                mode_raw,
                // Fan control mode lives in bits 7:4 (0 = Manual / PWM command)
                is_manual: (mode_raw >> 4) == 0,
            });
        }

        SuperIoSnapshot {
            detected: true,
            chip_name: chip.chip_name.to_string(),
            chip_id_hex: format!("0x{:04X}", chip.chip_id),
            hwm_base_hex: format!("0x{:04X} (slot {})", chip.hwm_base, chip.slot),
            diagnostic: format!(
                "{} (0x{:04X}) @ 0x{:04X} — {} canaux PWM/RPM actifs (Vendor 0x5CA3 vérifié)",
                chip.chip_name, chip.chip_id, chip.hwm_base, chip.fan_count
            ),
            fans,
        }
    }

    /// Directly set live manual PWM duty (`SUPERIO_MIN_PWM_PCT..=100`%) on a Nuvoton SuperIO fan
    /// channel AND verify via register read-back. Requests below the floor are raised to it.
    pub fn set_fan_pwm(channel_idx: usize, duty_pct: u8) -> Result<String> {
        let duty_pct = safe_superio_duty(duty_pct);
        if channel_idx >= 7 {
            anyhow::bail!("Canal SuperIO invalide ({channel_idx})");
        }
        let _lock = NamedMutexGuard::acquire(r"Global\Access_ISABUS.HTP.Method", 1500)
            .context("Bus SuperIO ISA occupé")?;
        let (sess, chip) = detect_and_prepare()?;
        if channel_idx >= chip.fan_count {
            anyhow::bail!("Canal #{channel_idx} hors plage pour {}", chip.chip_name);
        }

        let mode_reg = FAN_CTRL_MODE_REGS[channel_idx];
        let cmd_reg = FAN_PWM_CMD_REGS[channel_idx];
        let pwm_out_reg = if chip.use_98_pwm_out {
            FAN_PWM_OUT_REGS_98[channel_idx]
        } else {
            FAN_PWM_OUT_REGS_95[channel_idx]
        };

        // Save the original BIOS mode + PWM command once so restore puts back exactly what was there
        let cur_mode = hwm_read(&sess, chip.hwm_base, mode_reg)?;
        let cur_cmd = hwm_read(&sess, chip.hwm_base, cmd_reg)?;
        if let Ok(mut guard) = saved_states().lock() {
            if guard[channel_idx].is_none() {
                guard[channel_idx] = Some(SavedFanState {
                    mode: cur_mode,
                    cmd: cur_cmd,
                });
            }
        }

        let target_raw = ((duty_pct.min(100) as u32 * 255 + 50) / 100).min(255) as u8;
        // Switch channel to Manual Mode (0x00) and write PWM command byte
        hwm_write(&sess, chip.hwm_base, mode_reg, 0x00)?;
        hwm_write(&sess, chip.hwm_base, cmd_reg, target_raw)?;

        // Immediate hardware read-back verification!
        std::thread::sleep(std::time::Duration::from_millis(25));
        let rb_mode = hwm_read(&sess, chip.hwm_base, mode_reg)?;
        let rb_cmd = hwm_read(&sess, chip.hwm_base, cmd_reg)?;
        let rb_out = hwm_read(&sess, chip.hwm_base, pwm_out_reg)?;
        let rb_pct = ((rb_out as u32 * 100 + 127) / 255).min(100) as u8;

        if rb_mode != 0 || rb_cmd.abs_diff(target_raw) > 2 {
            anyhow::bail!(
                "Échec vérification SuperIO ch#{}: écrit mode=0/cmd={target_raw}, relu mode=0x{rb_mode:02X}/cmd={rb_cmd} (sortie={rb_pct}%)",
                channel_idx + 1
            );
        }

        Ok(format!(
            "SuperIO {} ch#{} -> {}% vérifié (cmd={}, sortie={}%, mode=Manuel)",
            chip.chip_name,
            channel_idx + 1,
            duty_pct.min(100),
            rb_cmd,
            rb_pct
        ))
    }

    /// Restore a SuperIO fan channel back to its original BIOS SmartFan IV mode.
    pub fn restore_fan_auto(channel_idx: usize) -> Result<String> {
        if channel_idx >= 7 {
            anyhow::bail!("Canal SuperIO invalide ({channel_idx})");
        }
        let _lock = NamedMutexGuard::acquire(r"Global\Access_ISABUS.HTP.Method", 1500)
            .context("Bus SuperIO ISA occupé")?;
        let saved = saved_states().lock().ok().and_then(|g| g[channel_idx]);
        let Some(saved) = saved else {
            // OpenGear never switched this channel to manual in this session: nothing to undo,
            // and guessing a mode byte could leave the fan in the wrong mode.
            return Ok(format!(
                "SuperIO ch#{} jamais modifié par OpenGear — réglage BIOS inchangé",
                channel_idx + 1
            ));
        };
        let (sess, chip) = detect_and_prepare()?;
        hwm_write(&sess, chip.hwm_base, FAN_PWM_CMD_REGS[channel_idx], saved.cmd)?;
        hwm_write(&sess, chip.hwm_base, FAN_CTRL_MODE_REGS[channel_idx], saved.mode)?;
        if let Ok(mut guard) = saved_states().lock() {
            guard[channel_idx] = None;
        }
        Ok(format!(
            "SuperIO {} ch#{} restauré en mode BIOS d'origine (mode=0x{:02X}, cmd={})",
            chip.chip_name,
            channel_idx + 1,
            saved.mode,
            saved.cmd
        ))
    }

    /// Write explicit saved BIOS states back (used by the fan watchdog / crash recovery).
    pub fn restore_saved_states(states: &[SuperIoSavedState]) -> Vec<String> {
        if states.is_empty() {
            return Vec::new();
        }
        let Some(_lock) = NamedMutexGuard::acquire(r"Global\Access_ISABUS.HTP.Method", 3000) else {
            return vec!["SuperIO : bus ISA occupé, restauration impossible".into()];
        };
        let (sess, chip) = match detect_and_prepare() {
            Ok(v) => v,
            Err(e) => return vec![format!("SuperIO : {e}")],
        };
        states
            .iter()
            .filter(|s| s.index < chip.fan_count)
            .map(|s| {
                let r = hwm_write(&sess, chip.hwm_base, FAN_PWM_CMD_REGS[s.index], s.cmd).and_then(
                    |_| hwm_write(&sess, chip.hwm_base, FAN_CTRL_MODE_REGS[s.index], s.mode),
                );
                match r {
                    Ok(()) => format!("SuperIO ch#{} restauré (mode=0x{:02X})", s.index + 1, s.mode),
                    Err(e) => format!("SuperIO ch#{} : restauration échouée : {e}", s.index + 1),
                }
            })
            .collect()
    }

    /// Restore every channel OpenGear switched to manual back to its original BIOS state.
    pub fn restore_all_fans_auto() -> Vec<String> {
        let touched: Vec<usize> = saved_states()
            .lock()
            .map(|g| (0..7).filter(|&i| g[i].is_some()).collect())
            .unwrap_or_default();
        touched
            .into_iter()
            .map(|i| match restore_fan_auto(i) {
                Ok(m) => m,
                Err(e) => format!("SuperIO ch#{} : restauration échouée : {e}", i + 1),
            })
            .collect()
    }
}

pub fn read_superio_snapshot() -> SuperIoSnapshot {
    #[cfg(windows)]
    {
        win_sio::read_snapshot()
    }
    #[cfg(not(windows))]
    SuperIoSnapshot::default()
}

pub fn set_superio_fan_pwm(channel_idx: usize, duty_pct: u8) -> Result<String> {
    #[cfg(windows)]
    {
        win_sio::set_fan_pwm(channel_idx, duty_pct)
    }
    #[cfg(not(windows))]
    {
        let _ = (channel_idx, duty_pct);
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Restore explicit saved states (fan watchdog / crash recovery in another process).
pub fn restore_superio_saved_states(states: &[SuperIoSavedState]) -> Vec<String> {
    #[cfg(windows)]
    {
        win_sio::restore_saved_states(states)
    }
    #[cfg(not(windows))]
    {
        let _ = states;
        Vec::new()
    }
}

/// Restore every SuperIO channel OpenGear put in manual mode (call on application exit).
pub fn restore_all_superio_fans_auto() -> Vec<String> {
    #[cfg(windows)]
    {
        win_sio::restore_all_fans_auto()
    }
    #[cfg(not(windows))]
    Vec::new()
}

pub fn restore_superio_fan_auto(channel_idx: usize) -> Result<String> {
    #[cfg(windows)]
    {
        win_sio::restore_fan_auto(channel_idx)
    }
    #[cfg(not(windows))]
    {
        let _ = channel_idx;
        anyhow::bail!("Non supporté hors Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_duty_is_floored_and_capped() {
        assert_eq!(safe_superio_duty(0), SUPERIO_MIN_PWM_PCT);
        assert_eq!(safe_superio_duty(SUPERIO_MIN_PWM_PCT - 1), SUPERIO_MIN_PWM_PCT);
        assert_eq!(safe_superio_duty(55), 55);
        assert_eq!(safe_superio_duty(250), 100);
    }
}
