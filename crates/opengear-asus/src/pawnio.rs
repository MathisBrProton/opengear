//! Universal hardware access via Microsoft WHQL-signed **PawnIO** (`namazso/PawnIO`).
//!
//! Uses official cryptographically signed PawnIO bytecode modules (`v0.2.12`):
//! - `SmbusPIIX4.bin` : Universal AMD (AM4 / AM5 / FCH) SMBus controller
//! - `SmbusI801.bin`  : Universal Intel (ICH / PCH) SMBus controller
//! - `AMDFamily17.bin`: Universal AMD Ryzen (1000..9000 / Zen 1..5) native CPU Tctl/Tdie temperature
//! - `IntelMSR.bin`   : Universal Intel Core native CPU Package temperature
//!
//! Safety guarantees:
//! - Acquires standard Windows cross-app hardware mutexes (`Global\Access_SMBUS.HTP.Method`,
//!   `Global\Access_PCI`) so OpenGear never collides with HWiNFO, OpenRGB, or LibreHardwareMonitor.
//! - Never touches the SMBus at all if the global SMBus mutex cannot be acquired.
//! - NEVER probes SPD EEPROM (`0x50..0x57`) nor the DDR4 SPD page-select / write-protect
//!   command range (`0x30..0x37`), not even in the diagnostic scan.
//! - Verifies the 16-byte read-only ENE hardware fingerprint (`0xA0..0xAF == 0x00..0x0F`) and
//!   rejects Micron (`0x1030 == "Micron"`) before writing a single LED register — including the
//!   boot-default `0x77` remap, and again right before every RGB write.

use crate::cooling::is_elevated;
use crate::ram_rgb::{EneRamMode, EneRamModule};
use anyhow::{Context, Result};
use opengear_core::Rgb;

const MOD_SMBUS_PIIX4: &[u8] = include_bytes!("../pawnio_modules/SmbusPIIX4.bin");
const MOD_SMBUS_I801: &[u8] = include_bytes!("../pawnio_modules/SmbusI801.bin");
const MOD_AMD_FAM17: &[u8] = include_bytes!("../pawnio_modules/AMDFamily17.bin");
const MOD_INTEL_MSR: &[u8] = include_bytes!("../pawnio_modules/IntelMSR.bin");

const I2C_SMBUS_WRITE: i64 = 0;
const I2C_SMBUS_READ: i64 = 1;
const I2C_SMBUS_BYTE_DATA: i64 = 2;
const I2C_SMBUS_WORD_DATA: i64 = 3;

/// DDR4 SPD page-select / write-protect commands (`0x30..=0x37`) and SPD EEPROM / DDR5 SPD hub
/// (`0x50..=0x57`): any SMBus transaction there (even the command phase of a "read") can flip
/// the SPD page or touch write protection, so diagnostics never address them.
pub(crate) fn is_diag_excluded_addr(addr7: u8) -> bool {
    (0x30..=0x37).contains(&addr7) || (0x50..=0x57).contains(&addr7)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PawnIoAvailability {
    Ready,
    NeedsAdmin,
    NotInstalled,
}

#[cfg(windows)]
mod win_pio {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Threading::{
        CreateMutexW, ReleaseMutex, WaitForSingleObject,
    };

    const DEVICE_TYPE: u32 = 41394u32 << 16;
    const IOCTL_PIO_LOAD_BINARY: u32 = DEVICE_TYPE | (0x821 << 2);
    const IOCTL_PIO_EXECUTE_FN: u32 = DEVICE_TYPE | (0x841 << 2);
    const FN_NAME_LEN: usize = 32;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    /// Cross-application Windows hardware mutex guard (compatible with HWiNFO / LHM / OpenRGB).
    pub struct NamedMutexGuard {
        handle: HANDLE,
    }

    impl NamedMutexGuard {
        pub fn acquire(name: &str, timeout_ms: u32) -> Option<Self> {
            unsafe {
                let w = to_wide(name);
                let h = CreateMutexW(std::ptr::null(), 0, w.as_ptr());
                if h.is_null() {
                    return None;
                }
                let res = WaitForSingleObject(h, timeout_ms);
                // WAIT_OBJECT_0 (0) or WAIT_ABANDONED (0x80) both grant ownership
                if res == WAIT_OBJECT_0 || res == 0x80 {
                    Some(Self { handle: h })
                } else {
                    CloseHandle(h);
                    None
                }
            }
        }
    }

    impl Drop for NamedMutexGuard {
        fn drop(&mut self) {
            unsafe {
                ReleaseMutex(self.handle);
                CloseHandle(self.handle);
            }
        }
    }

    pub struct PawnIoSession {
        handle: HANDLE,
    }

    impl Drop for PawnIoSession {
        fn drop(&mut self) {
            if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.handle) };
            }
        }
    }

    fn open_device_raw() -> HANDLE {
        for cand in [r"\\.\PawnIO", r"\\?\GLOBALROOT\Device\PawnIO"] {
            let path = to_wide(cand);
            let h = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            };
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                return h;
            }
        }
        INVALID_HANDLE_VALUE
    }

    /// Check if PawnIO is properly installed (official Uninstall key OR live device handle).
    /// Avoids false positives when a leftover `Services\PawnIO` key exists without a `Root\PawnIO` PnP device node.
    pub fn is_service_installed() -> bool {
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW,
        };
        if is_elevated() {
            let h = open_device_raw();
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(h) };
                return true;
            }
        }
        let subkey = to_wide(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO");
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = unsafe {
            RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey)
        };
        if rc == 0 && !hkey.is_null() {
            unsafe { RegCloseKey(hkey) };
            true
        } else {
            false
        }
    }

    /// Ensure the already-installed WHQL-signed `PawnIO` Windows service is started (Admin only).
    fn ensure_service_started() {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("sc")
            .args(["start", "PawnIO"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }

    impl PawnIoSession {
        pub fn load(module_bytes: &[u8]) -> Result<Self> {
            if !is_elevated() {
                anyhow::bail!("Droits Administrateur (UAC) requis pour PawnIO");
            }
            let mut h = open_device_raw();
            if h == INVALID_HANDLE_VALUE || h.is_null() {
                if is_service_installed() {
                    ensure_service_started();
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    h = open_device_raw();
                }
            }
            if h == INVALID_HANDLE_VALUE || h.is_null() {
                let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                anyhow::bail!(
                    "Périphérique PnP Root\\PawnIO absent (Win32 err={err}) — clique sur « Installer / Réparer PawnIO (WHQL) » pour enregistrer le pilote officiel."
                );
            }

            let mut bytes_ret: u32 = 0;
            let ok = unsafe {
                DeviceIoControl(
                    h,
                    IOCTL_PIO_LOAD_BINARY,
                    module_bytes.as_ptr() as *const core::ffi::c_void,
                    module_bytes.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut bytes_ret,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                unsafe { CloseHandle(h) };
                anyhow::bail!("PawnIO a rejeté le chargement du module (plateforme non correspondante)");
            }
            Ok(Self { handle: h })
        }

        pub fn execute(&self, fn_name: &str, inputs: &[i64], out_len: usize) -> Result<Vec<i64>> {
            let mut in_buf = vec![0u8; FN_NAME_LEN + inputs.len() * 8];
            let name_bytes = fn_name.as_bytes();
            let copy_len = name_bytes.len().min(FN_NAME_LEN - 1);
            in_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            for (i, &val) in inputs.iter().enumerate() {
                let off = FN_NAME_LEN + i * 8;
                in_buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
            }

            let mut out_raw = vec![0u8; out_len * 8];
            let mut bytes_ret: u32 = 0;
            let ok = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_PIO_EXECUTE_FN,
                    in_buf.as_ptr() as *const core::ffi::c_void,
                    in_buf.len() as u32,
                    if out_len == 0 {
                        std::ptr::null_mut()
                    } else {
                        out_raw.as_mut_ptr() as *mut core::ffi::c_void
                    },
                    out_raw.len() as u32,
                    &mut bytes_ret,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                anyhow::bail!("PawnIO execute '{fn_name}' échoué");
            }
            let count = (bytes_ret as usize) / 8;
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let off = i * 8;
                out.push(i64::from_le_bytes(out_raw[off..off + 8].try_into().unwrap()));
            }
            Ok(out)
        }
    }

    pub fn read_native_cpu_temp() -> Option<(f32, &'static str)> {
        if !is_elevated() || !is_service_installed() {
            return None;
        }
        // 1. Try AMD Ryzen (Zen 1..5 / Families 0x17..0x1A) via SMN THM_TCON_CUR_TMP (0x00059800)
        if let Ok(sess) = PawnIoSession::load(MOD_AMD_FAM17) {
            let _lock = NamedMutexGuard::acquire(r"Global\Access_PCI", 500);
            if let Ok(out) = sess.execute("ioctl_read_smn", &[0x0005_9800], 1) {
                if let Some(&raw) = out.first() {
                    let v = raw as u32;
                    let range_sel = (v & 0x0008_0000) != 0;
                    let steps = (v >> 21) & 0x7FF;
                    let mut temp = (steps as f32) * 0.125;
                    if range_sel {
                        temp -= 49.0;
                    }
                    if (5.0..=125.0).contains(&temp) {
                        return Some((temp, "PawnIO AMD Ryzen Tctl/Tdie"));
                    }
                }
            }
        }
        // 2. Try Intel Core (IA32_TEMPERATURE_TARGET 0x1A2 - IA32_PACKAGE_THERM_STATUS 0x1B1)
        if let Ok(sess) = PawnIoSession::load(MOD_INTEL_MSR) {
            let tj_max = sess
                .execute("ioctl_read_msr", &[0x1A2], 1)
                .ok()
                .and_then(|v| v.first().copied())
                .map(|r| ((r >> 16) & 0xFF) as f32)
                .filter(|&t| (70.0..=115.0).contains(&t))
                .unwrap_or(100.0);
            let therm = sess
                .execute("ioctl_read_msr", &[0x1B1], 1)
                .ok()
                .or_else(|| sess.execute("ioctl_read_msr", &[0x19C], 1).ok())
                .and_then(|v| v.first().copied());
            if let Some(raw) = therm {
                let valid = (raw & (1 << 31)) != 0;
                if valid {
                    let delta = ((raw >> 16) & 0x7F) as f32;
                    let temp = tj_max - delta;
                    if (5.0..=125.0).contains(&temp) {
                        return Some((temp, "PawnIO Intel Package Temp"));
                    }
                }
            }
        }
        None
    }

    // ---- Universal AMD (PIIX4) + Intel (I801) SMBus ENE DDR4/DDR5 RAM RGB ----

    fn smbus_read_byte(sess: &PawnIoSession, addr7: u8, cmd: u8) -> Option<u8> {
        let out = sess
            .execute(
                "ioctl_smbus_xfer",
                &[addr7 as i64, I2C_SMBUS_READ, cmd as i64, I2C_SMBUS_BYTE_DATA],
                1,
            )
            .ok()?;
        out.first().map(|&v| (v & 0xFF) as u8)
    }

    fn smbus_write_byte(sess: &PawnIoSession, addr7: u8, cmd: u8, val: u8) -> Result<()> {
        sess.execute(
            "ioctl_smbus_xfer",
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

    fn smbus_write_word(sess: &PawnIoSession, addr7: u8, cmd: u8, word: u16) -> Result<()> {
        sess.execute(
            "ioctl_smbus_xfer",
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

    fn ene_read_reg(sess: &PawnIoSession, addr7: u8, reg: u16) -> Option<u8> {
        smbus_write_word(sess, addr7, 0x00, reg.swap_bytes()).ok()?;
        smbus_read_byte(sess, addr7, 0x81)
    }

    fn ene_write_reg(sess: &PawnIoSession, addr7: u8, reg: u16, val: u8) -> Result<()> {
        smbus_write_word(sess, addr7, 0x00, reg.swap_bytes())?;
        smbus_write_byte(sess, addr7, 0x01, val)?;
        Ok(())
    }

    fn test_ene_fingerprint(sess: &PawnIoSession, addr7: u8) -> bool {
        for i in 0u8..16 {
            match smbus_read_byte(sess, addr7, 0xA0 + i) {
                Some(v) if v == i => {}
                _ => return false,
            }
        }
        true
    }

    const ENE_RAM_ADDRESSES: &[u8] = &[
        0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x4F, 0x66, 0x67, 0x39, 0x3A, 0x3B, 0x3C,
        0x3D,
    ];
    const ENE_REG_SLOT_INDEX: u16 = 0x80F8;
    const ENE_REG_I2C_ADDRESS: u16 = 0x80F9;

    fn smbus_read_byte_only(sess: &PawnIoSession, addr7: u8) -> Option<u8> {
        // I2C_SMBUS_BYTE (1) — exact probe used by OpenRGB before remapping 0x77
        let out = sess
            .execute("ioctl_smbus_xfer", &[addr7 as i64, I2C_SMBUS_READ, 0, 1], 1)
            .ok()?;
        out.first().map(|&v| (v & 0xFF) as u8)
    }

    fn remap_ene_from_0x77(sess: &PawnIoSession) {
        // OpenRGB ENESMBusControllerDetect sequence:
        // If multiple ENE RAM sticks sit at boot default 0x77, remap each slot to free ENE addresses.
        // Unlike a blind remap, we only ever write to 0x77 once it has proven to be an ENE controller
        // (read-only fingerprint), so an unrelated device answering at 0x77 never receives a write.
        let mut addr_idx: usize = 0;
        for slot in 0u8..8 {
            if smbus_read_byte_only(sess, 0x77).is_none()
                && smbus_read_byte(sess, 0x77, 0x00).is_none()
            {
                break;
            }
            if !test_ene_fingerprint(sess, 0x77) {
                break;
            }
            // Find next free address in ENE_RAM_ADDRESSES
            while addr_idx < ENE_RAM_ADDRESSES.len() {
                let cand = ENE_RAM_ADDRESSES[addr_idx];
                if cand != 0x77
                    && smbus_read_byte_only(sess, cand).is_none()
                    && smbus_read_byte(sess, cand, 0x00).is_none()
                {
                    break;
                }
                addr_idx += 1;
            }
            if addr_idx >= ENE_RAM_ADDRESSES.len() {
                break;
            }
            let target_addr = ENE_RAM_ADDRESSES[addr_idx];
            addr_idx += 1;
            let _ = ene_write_reg(sess, 0x77, ENE_REG_SLOT_INDEX, slot);
            let _ = ene_write_reg(sess, 0x77, ENE_REG_I2C_ADDRESS, target_addr << 1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn scan_bus_for_ene(
        sess: &PawnIoSession,
        backend_tag: &str,
        port_tag: u16,
    ) -> Vec<EneRamModule> {
        remap_ene_from_0x77(sess);
        let mut found = Vec::new();
        for &addr7 in ENE_RAM_ADDRESSES {
            if !test_ene_fingerprint(sess, addr7) {
                continue;
            }
            // Reject Micron controllers (0x1030 == "Micron")
            let mut mbytes = [0u8; 6];
            for i in 0..6 {
                mbytes[i] = ene_read_reg(sess, addr7, 0x1030 + i as u16).unwrap_or(0);
            }
            if &mbytes == b"Micron" {
                continue;
            }
            // Read ENE firmware string (0x1000..0x100F)
            let mut name_buf = Vec::with_capacity(16);
            for i in 0..16u16 {
                match ene_read_reg(sess, addr7, 0x1000 + i) {
                    Some(b) if (32..=126).contains(&b) => name_buf.push(b),
                    _ => break,
                }
            }
            let fw = String::from_utf8_lossy(&name_buf).trim().to_string();
            if fw.len() < 4 {
                continue;
            }
            let leds = ene_read_reg(sess, addr7, 0x1C02)
                .filter(|&n| (1..=20).contains(&n))
                .unwrap_or(8);
            let cur_mode = ene_read_reg(sess, addr7, 0x8021).unwrap_or(1);
            let is_v2 = !matches!(
                fw.as_str(),
                "LED-0116" | "DIMM_LED-0102" | "DIMM_LED-0103" | "AUMA0-E8K4-0101"
            );
            found.push(EneRamModule {
                address_7bit: addr7,
                wmi_slave_arg: port_tag,
                firmware: fw,
                is_v2,
                led_count: leds,
                current_mode: cur_mode,
                backend: backend_tag.to_string(),
            });
        }
        found
    }

    /// Probe ALL AMD (PIIX4 ports 0 & 1) or Intel (I801) SMBus controllers via WHQL-signed PawnIO!
    pub fn probe_pawnio_ene_ram() -> Vec<EneRamModule> {
        if !is_elevated() || !is_service_installed() {
            return Vec::new();
        }
        // The probe may remap 0x77 (writes): never run it without exclusive bus ownership.
        let Some(_lock) = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500) else {
            return Vec::new();
        };

        // 1. AMD PIIX4 / AM4 / AM5 FCH SMBus (scan port 0 = primary DRAM, port 1 = aux 0x0B20, port 2 = muxed)
        if let Ok(sess) = PawnIoSession::load(MOD_SMBUS_PIIX4) {
            let mut mods = Vec::new();
            let prev_port = sess
                .execute("ioctl_piix4_port_sel", &[-1], 1)
                .ok()
                .and_then(|v| v.first().copied())
                .unwrap_or(0);
            for &port in &[0i64, 1i64, 2i64] {
                if sess.execute("ioctl_piix4_port_sel", &[port], 1).is_ok() {
                    mods.extend(scan_bus_for_ene(&sess, "pawnio_piix4", port as u16));
                    if !mods.is_empty() {
                        break;
                    }
                }
            }
            let _ = sess.execute("ioctl_piix4_port_sel", &[prev_port], 1);
            if !mods.is_empty() {
                return mods;
            }
        }

        // 2. Intel I801 / PCH SMBus
        if let Ok(sess) = PawnIoSession::load(MOD_SMBUS_I801) {
            let mods = scan_bus_for_ene(&sess, "pawnio_i801", 0);
            if !mods.is_empty() {
                return mods;
            }
        }

        Vec::new()
    }

    pub fn apply_pawnio_ene_ram(
        targets: &[&EneRamModule],
        mode: EneRamMode,
        color: Rgb,
        speed: u8,
        reverse: bool,
        save_eeprom: bool,
    ) -> Result<String> {
        if !is_elevated() {
            anyhow::bail!("Droits Administrateur requis pour PawnIO SMBus");
        }
        let _lock = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500)
            .context("Bus SMBus occupé par un autre logiciel")?;

        let is_intel = targets.first().map(|m| m.backend == "pawnio_i801").unwrap_or(false);
        let sess = if is_intel {
            PawnIoSession::load(MOD_SMBUS_I801)?
        } else {
            PawnIoSession::load(MOD_SMBUS_PIIX4)?
        };

        let prev_port = if !is_intel {
            sess.execute("ioctl_piix4_port_sel", &[-1], 1)
                .ok()
                .and_then(|v| v.first().copied())
                .unwrap_or(0)
        } else {
            0
        };

        let mut applied = 0usize;
        let mut skipped: Vec<String> = Vec::new();
        let write_result = (|| -> Result<()> {
            for m in targets {
                if !is_intel
                    && sess
                        .execute("ioctl_piix4_port_sel", &[m.wmi_slave_arg as i64], 1)
                        .is_err()
                {
                    skipped.push(format!("0x{:02X} (port {} inaccessible)", m.address_7bit, m.wmi_slave_arg));
                    continue;
                }
                let addr = m.address_7bit;
                // Re-verify the read-only ENE fingerprint right before writing: the cached scan
                // result may be stale (remap, other software, wrong port).
                if !test_ene_fingerprint(&sess, addr) {
                    skipped.push(format!("0x{addr:02X} (empreinte ENE absente)"));
                    continue;
                }
                let effect_reg: u16 = if m.is_v2 { 0x8160 } else { 0x8010 };
                let leds = m.led_count.clamp(1, 16) as u16;

                // Hardware mode (disable direct)
                ene_write_reg(&sess, addr, 0x8020, 0)?;
                // Write [Red, Blue, Green] triplet per LED
                for i in 0..leds {
                    let base = effect_reg + i * 3;
                    ene_write_reg(&sess, addr, base, color.r)?;
                    ene_write_reg(&sess, addr, base + 1, color.b)?;
                    ene_write_reg(&sess, addr, base + 2, color.g)?;
                }
                ene_write_reg(&sess, addr, 0x8021, mode as u8)?;
                ene_write_reg(&sess, addr, 0x8022, speed.min(4))?;
                ene_write_reg(&sess, addr, 0x8023, if reverse { 1 } else { 0 })?;
                ene_write_reg(&sess, addr, 0x80A0, 0x01)?;
                if save_eeprom {
                    ene_write_reg(&sess, addr, 0x80A0, 0xAA)?;
                }
                applied += 1;
            }
            Ok(())
        })();

        // Always restore the previously selected PIIX4 port, even after a mid-sequence error.
        if !is_intel {
            let _ = sess.execute("ioctl_piix4_port_sel", &[prev_port], 1);
        }
        write_result?;
        if applied == 0 {
            anyhow::bail!(
                "Aucune barrette ENE vérifiée — aucune écriture effectuée ({})",
                skipped.join(", ")
            );
        }
        Ok(format!(
            "OK PawnIO {} ({applied} DIMM{})",
            if is_intel { "Intel I801" } else { "AMD PIIX4" },
            if save_eeprom { ", EEPROM sauvée" } else { "" }
        ))
    }
    /// Write ONLY the effect mode register of each module (no color / speed / EEPROM writes), so
    /// the stored colors survive: used to switch RAM lighting off and back on exactly.
    pub fn set_pawnio_ene_modes(targets: &[(&EneRamModule, u8)]) -> Result<usize> {
        if !is_elevated() {
            anyhow::bail!("Droits Administrateur requis pour PawnIO SMBus");
        }
        let Some(first) = targets.first() else {
            return Ok(0);
        };
        let _lock = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500)
            .context("Bus SMBus occupé par un autre logiciel")?;
        let is_intel = first.0.backend == "pawnio_i801";
        let sess = if is_intel {
            PawnIoSession::load(MOD_SMBUS_I801)?
        } else {
            PawnIoSession::load(MOD_SMBUS_PIIX4)?
        };
        let prev_port = if !is_intel {
            sess.execute("ioctl_piix4_port_sel", &[-1], 1)
                .ok()
                .and_then(|v| v.first().copied())
                .unwrap_or(0)
        } else {
            0
        };
        let mut applied = 0usize;
        let res = (|| -> Result<()> {
            for (m, mode) in targets {
                if !is_intel
                    && sess
                        .execute("ioctl_piix4_port_sel", &[m.wmi_slave_arg as i64], 1)
                        .is_err()
                {
                    continue;
                }
                if !test_ene_fingerprint(&sess, m.address_7bit) {
                    continue;
                }
                ene_write_reg(&sess, m.address_7bit, 0x8020, 0)?;
                ene_write_reg(&sess, m.address_7bit, 0x8021, *mode)?;
                ene_write_reg(&sess, m.address_7bit, 0x80A0, 0x01)?;
                applied += 1;
            }
            Ok(())
        })();
        if !is_intel {
            let _ = sess.execute("ioctl_piix4_port_sel", &[prev_port], 1);
        }
        res?;
        Ok(applied)
    }

    /// Diagnostic scan of PawnIO SMBus controllers (lists every responding 7-bit I2C address).
    ///
    /// Uses SMBus read transactions only (the "read byte data" command phase sets a register
    /// pointer, it never writes a register value) and skips SPD-sensitive address ranges.
    pub fn diagnose_smbus_readonly() -> Vec<String> {
        let mut lines = Vec::new();
        if !is_elevated() {
            lines.push("Admin UAC requis pour diagnostiquer le bus SMBus PawnIO.".into());
            return lines;
        }
        if !is_service_installed() {
            lines.push("Service PawnIO non installé.".into());
            return lines;
        }
        let Some(_lock) = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500) else {
            lines.push("Bus SMBus occupé par un autre logiciel (HWiNFO, OpenRGB, LHM…) — diagnostic annulé.".into());
            return lines;
        };
        lines.push("Adresses SPD 0x30..0x37 et 0x50..0x57 volontairement ignorées.".into());

        match PawnIoSession::load(MOD_SMBUS_PIIX4) {
            Ok(sess) => {
                if let Ok(id) = sess.execute("ioctl_identity", &[], 3) {
                    lines.push(format!(
                        "SmbusPIIX4 chargé : io_base=0x{:X}, pci=0x{:X}",
                        id.get(1).copied().unwrap_or(0),
                        id.get(2).copied().unwrap_or(0)
                    ));
                }
                let prev = sess
                    .execute("ioctl_piix4_port_sel", &[-1], 1)
                    .ok()
                    .and_then(|v| v.first().copied())
                    .unwrap_or(0);
                for &port in &[0i64, 1i64, 2i64] {
                    if sess.execute("ioctl_piix4_port_sel", &[port], 1).is_ok() {
                        let mut found = Vec::new();
                        for addr in (0x10u8..=0x7Fu8).filter(|&a| !is_diag_excluded_addr(a)) {
                            if smbus_read_byte(&sess, addr, 0x00).is_some()
                                || smbus_read_byte_only(&sess, addr).is_some()
                            {
                                let ene_ok = ENE_RAM_ADDRESSES.contains(&addr)
                                    && test_ene_fingerprint(&sess, addr);
                                found.push(if ene_ok {
                                    format!("0x{addr:02X}[ENE!]")
                                } else {
                                    format!("0x{addr:02X}")
                                });
                            }
                        }
                        lines.push(format!(
                            "  PIIX4 port {port} -> {} adresse(s): [{}]",
                            found.len(),
                            found.join(", ")
                        ));
                    }
                }
                let _ = sess.execute("ioctl_piix4_port_sel", &[prev], 1);
            }
            Err(e) => lines.push(format!("SmbusPIIX4 non chargé : {e}")),
        }

        match PawnIoSession::load(MOD_SMBUS_I801) {
            Ok(sess) => {
                let mut found = Vec::new();
                for addr in (0x10u8..=0x7Fu8).filter(|&a| !is_diag_excluded_addr(a)) {
                    if smbus_read_byte(&sess, addr, 0x00).is_some() {
                        found.push(format!("0x{addr:02X}"));
                    }
                }
                lines.push(format!(
                    "SmbusI801 chargé -> {} adresse(s): [{}]",
                    found.len(),
                    found.join(", ")
                ));
            }
            Err(e) => lines.push(format!("SmbusI801 : {e}")),
        }
        lines
    }
}

#[cfg(windows)]
pub use win_pio::{NamedMutexGuard, PawnIoSession};

pub fn diagnose_smbus_readonly() -> Vec<String> {
    #[cfg(windows)]
    {
        win_pio::diagnose_smbus_readonly()
    }
    #[cfg(not(windows))]
    Vec::new()
}

pub fn pawnio_availability() -> PawnIoAvailability {
    #[cfg(windows)]
    {
        if !win_pio::is_service_installed() {
            PawnIoAvailability::NotInstalled
        } else if !is_elevated() {
            PawnIoAvailability::NeedsAdmin
        } else {
            PawnIoAvailability::Ready
        }
    }
    #[cfg(not(windows))]
    PawnIoAvailability::NotInstalled
}

pub fn read_native_cpu_temp() -> Option<(f32, &'static str)> {
    #[cfg(windows)]
    {
        win_pio::read_native_cpu_temp()
    }
    #[cfg(not(windows))]
    None
}

/// Set only the effect mode of ENE RAM modules over PawnIO (colors and EEPROM untouched).
pub fn set_pawnio_ene_modes(targets: &[(&EneRamModule, u8)]) -> Result<usize> {
    #[cfg(windows)]
    {
        win_pio::set_pawnio_ene_modes(targets)
    }
    #[cfg(not(windows))]
    {
        let _ = targets;
        anyhow::bail!("Non supporté hors Windows")
    }
}

pub fn probe_pawnio_ene_ram() -> Vec<EneRamModule> {
    #[cfg(windows)]
    {
        win_pio::probe_pawnio_ene_ram()
    }
    #[cfg(not(windows))]
    Vec::new()
}

pub fn apply_pawnio_ene_ram(
    targets: &[&EneRamModule],
    mode: EneRamMode,
    color: Rgb,
    speed: u8,
    reverse: bool,
    save_eeprom: bool,
) -> Result<String> {
    #[cfg(windows)]
    {
        win_pio::apply_pawnio_ene_ram(targets, mode, color, speed, reverse, save_eeprom)
    }
    #[cfg(not(windows))]
    {
        let _ = (targets, mode, color, speed, reverse, save_eeprom);
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Download & run the official Microsoft WHQL-signed `PawnIO_setup.exe` (v2.2.0) via standard Windows UAC.
/// Properly creates the `Root\PawnIO` PnP device node so `\Device\PawnIO` and `\\.\PawnIO` work universally.
pub fn install_official_pawnio() -> Result<String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let setup_path = std::env::temp_dir().join("PawnIO_setup.exe");
        let url = "https://github.com/namazso/PawnIO.Setup/releases/download/2.2.0/PawnIO_setup.exe";

        let dl_script = format!(
            "Invoke-WebRequest -UseBasicParsing -Uri '{url}' -OutFile '{dst}' -TimeoutSec 25",
            dst = setup_path.display().to_string().replace('\'', "''")
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &dl_script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("Échec téléchargement PawnIO_setup.exe")?;
        if !out.status.success() || !setup_path.exists() {
            anyhow::bail!(
                "Téléchargement PawnIO_setup.exe échoué : {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }

        let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = setup_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let args: Vec<u16> = OsStr::new("-install").encode_wide().chain(Some(0)).collect();
        let rc = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                args.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            ) as isize
        };
        if rc <= 32 {
            anyhow::bail!("Lancement PawnIO_setup.exe annulé ou refusé (code {rc})");
        }
        Ok("Installateur officiel PawnIO v2.2.0 (WHQL) lancé — clique sur Rafraîchir une fois terminé !".into())
    }
    #[cfg(not(windows))]
    anyhow::bail!("Non supporté hors Windows")
}

#[cfg(test)]
mod tests {
    use super::is_diag_excluded_addr;

    #[test]
    fn spd_ranges_are_never_probed() {
        for a in 0x30u8..=0x37 {
            assert!(is_diag_excluded_addr(a), "0x{a:02X}");
        }
        for a in 0x50u8..=0x57 {
            assert!(is_diag_excluded_addr(a), "0x{a:02X}");
        }
        for a in [0x2Fu8, 0x38, 0x39, 0x4F, 0x58, 0x70, 0x77] {
            assert!(!is_diag_excluded_addr(a), "0x{a:02X}");
        }
    }
}
