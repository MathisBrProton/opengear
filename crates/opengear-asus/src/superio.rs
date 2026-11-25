//! Universal Nuvoton SuperIO (`NCT6791D`..`NCT6799D`) live Fan RPM & PWM
//! controller via Microsoft WHQL-signed **PawnIO** (`LpcIO.bin`).

use crate::cooling::is_elevated;
use crate::pawnio::{
    NamedMutexGuard, PawnIoAvailability, PawnIoSession, load_pawnio_module, pawnio_availability,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

const BANK_SELECT_REG: u8 = 0x4E;
const VENDOR_ID_HIGH_REG: u16 = 0x804F;
const VENDOR_ID_LOW_REG: u16 = 0x004F;
const NUVOTON_VENDOR_ID: u16 = 0x5CA3;

const FAN_RPM_REGS_95: [u16; 7] = [0x4C0, 0x4C2, 0x4C4, 0x4C6, 0x4C8, 0x4CA, 0x4CE];
const FAN_RPM_REGS_96: [u16; 7] = [0x4B0, 0x4B2, 0x4B4, 0x4B6, 0x4B8, 0x4BA, 0x4CC];
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
    /// Raw control mode byte (bits 7:4 == 0 -> Manual, non-zero -> SmartFan / BIOS auto)
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
pub(crate) struct DetectedChip {
    slot: i64,
    hwm_base: u16,
    chip_id: u16,
    chip_name: &'static str,
    fan_count: usize,
    use_98_pwm_out: bool,
    use_96_rpm_regs: bool,
}

/// Minimum manual PWM duty OpenGear will ever write to a generic SuperIO header.
pub const SUPERIO_MIN_PWM_PCT: u8 = 30;

/// Clamp a requested manual duty into `floor_pct..=100` (never below `SUPERIO_MIN_PWM_PCT` unless explicit higher floor).
pub fn safe_superio_duty_with_floor(duty_pct: u8, floor_pct: u8) -> u8 {
    let floor = floor_pct.clamp(SUPERIO_MIN_PWM_PCT, 100);
    duty_pct.clamp(floor, 100)
}

/// Clamp a requested manual duty into the safe range `SUPERIO_MIN_PWM_PCT..=100`.
pub fn safe_superio_duty(duty_pct: u8) -> u8 {
    safe_superio_duty_with_floor(duty_pct, SUPERIO_MIN_PWM_PCT)
}

/// Preserve lower bits `3:0` when switching `FAN_CTRL_MODE_REGS` to manual mode (`bits 7:4 == 0`).
pub(crate) fn manual_mode_byte(cur_mode: u8) -> u8 {
    cur_mode & 0x0F
}

#[derive(Debug, Clone, Copy)]
struct SavedFanState {
    mode: u8,
    cmd: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperIoSavedState {
    pub index: usize,
    pub mode: u8,
    pub cmd: u8,
}

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

/// Return `(name, fan_count, use_98_pwm_out, use_96_rpm_regs)`.
pub(crate) fn identify_nuvoton(id: u16) -> Option<(&'static str, usize, bool, bool)> {
    match id & 0xFFF0 {
        0xC800 => Some(("Nuvoton NCT6791D", 6, false, false)),
        0xC910 => Some(("Nuvoton NCT6792D", 6, false, false)),
        0xD120 => Some(("Nuvoton NCT6793D/NCT6795D", 6, false, false)),
        0xD350 => Some(("Nuvoton NCT6796D", 6, false, true)),
        0xD420 => {
            if id == 0xD42B || id == 0xD428 {
                Some(("Nuvoton NCT6798D", 7, true, true))
            } else {
                Some(("Nuvoton NCT6796D-R/S", 7, true, true))
            }
        }
        0xD450 => Some(("Nuvoton NCT6797D", 7, true, true)),
        0xD800 => Some(("Nuvoton NCT6799D", 7, true, true)),
        0xD590 => Some(("Nuvoton NCT6796D-S", 7, true, true)),
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
        let mod_bytes = load_pawnio_module("LpcIO.bin")?;
        let sess = PawnIoSession::load(&mod_bytes)?;
        for &(slot, reg_port) in &[(0i64, 0x2Eu16), (1i64, 0x4Eu16)] {
            if sess.execute("ioctl_select_slot", &[slot], 0).is_err() {
                continue;
            }
            if enter_nuvoton(&sess, reg_port).is_err() {
                continue;
            }
            let chip_id = sio_inw(&sess, 0x20).unwrap_or(0xFFFF);
            if let Some((chip_name, fan_count, use_98_pwm_out, use_96_rpm_regs)) =
                identify_nuvoton(chip_id)
            {
                let _ = sess.execute("ioctl_find_bars", &[], 0);
                let _ = sio_outb(&sess, 0x07, 0x0B);
                let hwm_base = sio_inw(&sess, 0x60).unwrap_or(0);
                exit_nuvoton(&sess, reg_port);

                if hwm_base >= 0x100 && hwm_base != 0xFFFF {
                    if !verify_nuvoton_vendor(&sess, hwm_base) {
                        if let Some(_ec_lock) = NamedMutexGuard::acquire(r"Global\Access_EC", 500) {
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
                                use_96_rpm_regs,
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

    static SNAPSHOT_SESSION: OnceLock<Mutex<Option<(PawnIoSession, DetectedChip)>>> =
        OnceLock::new();

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

        let Some(_lock) = NamedMutexGuard::acquire(r"Global\Access_ISABUS.HTP.Method", 1000) else {
            return SuperIoSnapshot {
                diagnostic: "Bus SuperIO ISA occupé par un autre outil — lecture annulée.".into(),
                ..Default::default()
            };
        };

        let cache = SNAPSHOT_SESSION.get_or_init(|| Mutex::new(None));
        let Ok(mut guard) = cache.lock() else {
            return SuperIoSnapshot {
                diagnostic: "Verrou interne SuperIO indisponible.".into(),
                ..Default::default()
            };
        };

        if guard.is_none() {
            match detect_and_prepare() {
                Ok(v) => *guard = Some(v),
                Err(e) => {
                    return SuperIoSnapshot {
                        diagnostic: e.to_string(),
                        ..Default::default()
                    };
                }
            }
        }

        let Some((ref sess, chip)) = *guard else {
            return SuperIoSnapshot::default();
        };

        if !verify_nuvoton_vendor(sess, chip.hwm_base) {
            *guard = None;
            return SuperIoSnapshot {
                diagnostic: "Réponse SuperIO invalide — session réinitialisée.".into(),
                ..Default::default()
            };
        }

        let rpm_regs = if chip.use_96_rpm_regs {
            &FAN_RPM_REGS_96
        } else {
            &FAN_RPM_REGS_95
        };
        let pwm_out_regs = if chip.use_98_pwm_out {
            &FAN_PWM_OUT_REGS_98
        } else {
            &FAN_PWM_OUT_REGS_95
        };

        let mut fans = Vec::new();
        for i in 0..chip.fan_count {
            let Ok(hi) = hwm_read(sess, chip.hwm_base, rpm_regs[i]) else {
                *guard = None;
                return SuperIoSnapshot {
                    diagnostic: "Erreur lecture RPM SuperIO.".into(),
                    ..Default::default()
                };
            };
            let Ok(lo) = hwm_read(sess, chip.hwm_base, rpm_regs[i] + 1) else {
                *guard = None;
                return SuperIoSnapshot {
                    diagnostic: "Erreur lecture RPM SuperIO.".into(),
                    ..Default::default()
                };
            };
            let raw_rpm = ((hi as u32) << 8) | (lo as u32);
            let rpm = if (100..15000).contains(&raw_rpm) {
                raw_rpm
            } else {
                0
            };

            let pwm_raw = hwm_read(sess, chip.hwm_base, pwm_out_regs[i]).unwrap_or(0);
            let pwm_pct = ((pwm_raw as u32 * 100 + 127) / 255).min(100) as u8;
            let mode_raw = hwm_read(sess, chip.hwm_base, FAN_CTRL_MODE_REGS[i]).unwrap_or(0xFF);

            fans.push(SuperIoFanChannel {
                index: i,
                label: format!("Fan Header #{}", i + 1),
                rpm,
                pwm_pct,
                mode_raw,
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

    pub fn set_fan_pwm(channel_idx: usize, duty_pct: u8, min_floor_pct: u8) -> Result<String> {
        let duty_pct = safe_superio_duty_with_floor(duty_pct, min_floor_pct);
        if channel_idx >= 7 {
            anyhow::bail!("Canal SuperIO invalide ({channel_idx})");
        }

        let (sess, chip, cur_mode, cur_cmd, target_raw, mode_reg, cmd_reg, pwm_out_reg) = {
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
            let new_mode = manual_mode_byte(cur_mode);
            hwm_write(&sess, chip.hwm_base, mode_reg, new_mode)?;
            if let Err(e) = hwm_write(&sess, chip.hwm_base, cmd_reg, target_raw) {
                let _ = hwm_write(&sess, chip.hwm_base, mode_reg, cur_mode);
                return Err(e);
            }
            (
                sess,
                chip,
                cur_mode,
                cur_cmd,
                target_raw,
                mode_reg,
                cmd_reg,
                pwm_out_reg,
            )
        };

        // Release ISA mutex while waiting for the SuperIO PWM output to settle.
        std::thread::sleep(std::time::Duration::from_millis(25));

        let _lock = NamedMutexGuard::acquire(r"Global\Access_ISABUS.HTP.Method", 1500)
            .context("Bus SuperIO ISA occupé lors de la vérification")?;
        let rb_mode = hwm_read(&sess, chip.hwm_base, mode_reg)?;
        let rb_cmd = hwm_read(&sess, chip.hwm_base, cmd_reg)?;
        let rb_out = hwm_read(&sess, chip.hwm_base, pwm_out_reg)?;
        let rb_pct = ((rb_out as u32 * 100 + 127) / 255).min(100) as u8;

        if (rb_mode >> 4) != 0 || rb_cmd.abs_diff(target_raw) > 2 {
            let _ = hwm_write(&sess, chip.hwm_base, cmd_reg, cur_cmd);
            let _ = hwm_write(&sess, chip.hwm_base, mode_reg, cur_mode);
            anyhow::bail!(
                "Échec vérification SuperIO ch#{} (rollback appliqué): écrit mode=0x{:02X}/cmd={target_raw}, relu mode=0x{rb_mode:02X}/cmd={rb_cmd} (sortie={rb_pct}%)",
                channel_idx + 1,
                manual_mode_byte(cur_mode)
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

    pub fn restore_fan_auto(channel_idx: usize) -> Result<String> {
        if channel_idx >= 7 {
            anyhow::bail!("Canal SuperIO invalide ({channel_idx})");
        }
        let _lock = NamedMutexGuard::acquire(r"Global\Access_ISABUS.HTP.Method", 1500)
            .context("Bus SuperIO ISA occupé")?;
        let saved = saved_states().lock().ok().and_then(|g| g[channel_idx]);
        let Some(saved) = saved else {
            return Ok(format!(
                "SuperIO ch#{} jamais modifié par OpenGear — réglage BIOS inchangé",
                channel_idx + 1
            ));
        };
        let (sess, chip) = detect_and_prepare()?;
        hwm_write(&sess, chip.hwm_base, FAN_PWM_CMD_REGS[channel_idx], saved.cmd)?;
        hwm_write(
            &sess,
            chip.hwm_base,
            FAN_CTRL_MODE_REGS[channel_idx],
            saved.mode,
        )?;
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
                let r = hwm_write(&sess, chip.hwm_base, FAN_PWM_CMD_REGS[s.index], s.cmd)
                    .and_then(|_| {
                        hwm_write(&sess, chip.hwm_base, FAN_CTRL_MODE_REGS[s.index], s.mode)
                    });
                match r {
                    Ok(()) => {
                        format!("SuperIO ch#{} restauré (mode=0x{:02X})", s.index + 1, s.mode)
                    }
                    Err(e) => format!("SuperIO ch#{} : restauration échouée : {e}", s.index + 1),
                }
            })
            .collect()
    }

    pub fn restore_all_fans_auto() -> Vec<String> {
        let states = saved_superio_states();
        if states.is_empty() {
            return Vec::new();
        }
        let out = restore_saved_states(&states);
        if let Ok(mut guard) = saved_states().lock() {
            for s in &states {
                if s.index < guard.len() {
                    guard[s.index] = None;
                }
            }
        }
        out
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
    set_superio_fan_pwm_with_floor(channel_idx, duty_pct, SUPERIO_MIN_PWM_PCT)
}

pub fn set_superio_fan_pwm_with_floor(
    channel_idx: usize,
    duty_pct: u8,
    min_floor_pct: u8,
) -> Result<String> {
    #[cfg(windows)]
    {
        win_sio::set_fan_pwm(channel_idx, duty_pct, min_floor_pct)
    }
    #[cfg(not(windows))]
    {
        let _ = (channel_idx, duty_pct, min_floor_pct);
        anyhow::bail!("Non supporté hors Windows")
    }
}

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
        assert_eq!(
            safe_superio_duty(SUPERIO_MIN_PWM_PCT - 1),
            SUPERIO_MIN_PWM_PCT
        );
        assert_eq!(safe_superio_duty(55), 55);
        assert_eq!(safe_superio_duty(250), 100);
        assert_eq!(safe_superio_duty_with_floor(10, 45), 45);
    }

    #[test]
    fn mode_lower_nibble_and_rpm_table_selection() {
        assert_eq!(manual_mode_byte(0x5A), 0x0A);
        let (_, _, _, use_96_old) = identify_nuvoton(0xD121).unwrap();
        let (_, _, _, use_96_new) = identify_nuvoton(0xD42B).unwrap();
        assert!(!use_96_old);
        assert!(use_96_new);
    }
}
