//! Universal hardware access via Microsoft WHQL-signed **PawnIO** (`namazso/PawnIO`).
//!
//! Uses official cryptographically signed PawnIO bytecode modules (`v0.2.12`) loaded as external
//! replaceable files from `pawnio_modules/` (LGPL-2.1 compliance):
//! - `SmbusPIIX4.bin` : Universal AMD (AM4 / AM5 / FCH) SMBus controller
//! - `SmbusI801.bin`  : Universal Intel (ICH / PCH) SMBus controller
//! - `AMDFamily17.bin`: Universal AMD Ryzen (1000..9000 / Zen 1..5) native CPU Tctl/Tdie temperature
//! - `IntelMSR.bin`   : Universal Intel Core native CPU Package temperature
//!
//! Safety guarantees:
//! - Acquires standard Windows cross-app hardware mutexes (`Global\Access_SMBUS.HTP.Method`,
//!   `Global\Access_PCI`) with world-accessible NULL-DACL descriptors matching OpenRGB/LHM/HWiNFO.
//! - Refuses hardware access whenever the required bus mutex cannot be acquired.
//! - NEVER probes SPD EEPROM (`0x50..0x57`), DDR4 SPD page/WP (`0x30..0x37`), DDR5 PMICs
//!   (`0x48..0x4F`), or thermal sensors (`0x18..0x1F`), not even in diagnostics.
//! - Verifies the 16-byte read-only ENE hardware fingerprint (`0xA0..0xAF == 0x00..0x0F`) and
//!   rejects Micron (`0x1030 == "Micron"`) before writing a single LED register.

use crate::ram_rgb::{EneRamMode, EneRamModule};
use anyhow::{Context, Result};
use opengear_core::Rgb;
use opengear_platform::is_elevated;
use std::path::{Path, PathBuf};

const I2C_SMBUS_WRITE: i64 = 0;
const I2C_SMBUS_READ: i64 = 1;
const I2C_SMBUS_BYTE_DATA: i64 = 2;
const I2C_SMBUS_WORD_DATA: i64 = 3;

/// Pinned SHA-256 for official `PawnIO_setup.exe` v2.2.0 (`namazso/PawnIO.Setup`).
const PAWNIO_SETUP_2_2_0_SHA256: &str =
    "1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032";

/// Load an external PawnIO bytecode module (`.bin`) from `pawnio_modules/`.
/// Rejects path traversal and checks beside the executable, parent dir, and dev manifest dir.
pub(crate) fn load_pawnio_module(filename: &str) -> Result<Vec<u8>> {
    if filename.is_empty()
        || filename.contains(['/', '\\', ':'])
        || filename.contains("..")
        || !filename.ends_with(".bin")
    {
        anyhow::bail!("Nom de module PawnIO invalide : {filename:?}");
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("pawnio_modules").join(filename));
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join("pawnio_modules").join(filename));
        }
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("pawnio_modules")
            .join(filename),
    );

    for path in &candidates {
        if path.is_file() {
            return std::fs::read(path).with_context(|| {
                format!("Impossible de lire le module PawnIO {}", path.display())
            });
        }
    }

    anyhow::bail!(
        "Module PawnIO '{filename}' introuvable dans pawnio_modules/ ({})",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Sensitive SMBus 7-bit address ranges excluded even from read-only diagnostic scans:
/// - `0x18..=0x1F`: SMBus thermal sensors
/// - `0x30..=0x37`: DDR4 SPD page-select / write-protect commands
/// - `0x48..=0x4F`: DDR5 PMIC power controllers
/// - `0x50..=0x57`: SPD EEPROM / DDR5 SPD hub
pub(crate) fn is_diag_excluded_addr(addr7: u8) -> bool {
    (0x18..=0x1F).contains(&addr7)
        || (0x30..=0x37).contains(&addr7)
        || (0x48..=0x4F).contains(&addr7)
        || (0x50..=0x57).contains(&addr7)
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
    use std::sync::{Mutex, OnceLock};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::{
        InitializeSecurityDescriptor, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        SECURITY_DESCRIPTOR, SetSecurityDescriptorDacl,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

    const DEVICE_TYPE: u32 = 41394u32 << 16;
    const IOCTL_PIO_LOAD_BINARY: u32 = DEVICE_TYPE | (0x821 << 2);
    const IOCTL_PIO_EXECUTE_FN: u32 = DEVICE_TYPE | (0x841 << 2);
    const FN_NAME_LEN: usize = 32;
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    /// Cross-application Windows hardware mutex guard (compatible with HWiNFO / LHM / OpenRGB).
    /// Uses a NULL DACL so both elevated and non-elevated monitoring tools can share the lock.
    pub struct NamedMutexGuard {
        handle: HANDLE,
    }

    impl NamedMutexGuard {
        pub fn acquire(name: &str, timeout_ms: u32) -> Option<Self> {
            unsafe {
                let mut sd: SECURITY_DESCRIPTOR = std::mem::zeroed();
                let mut sa = SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: std::ptr::null_mut(),
                    bInheritHandle: 0,
                };
                if InitializeSecurityDescriptor(
                    &mut sd as *mut SECURITY_DESCRIPTOR as PSECURITY_DESCRIPTOR,
                    SECURITY_DESCRIPTOR_REVISION,
                ) != 0
                    && SetSecurityDescriptorDacl(
                        &mut sd as *mut SECURITY_DESCRIPTOR as PSECURITY_DESCRIPTOR,
                        1,
                        std::ptr::null(),
                        0,
                    ) != 0
                {
                    sa.lpSecurityDescriptor =
                        &mut sd as *mut SECURITY_DESCRIPTOR as *mut core::ffi::c_void;
                }

                let w = to_wide(name);
                let sa_ptr = if sa.lpSecurityDescriptor.is_null() {
                    std::ptr::null()
                } else {
                    &sa
                };
                let h = CreateMutexW(sa_ptr, 0, w.as_ptr());
                if h.is_null() {
                    return None;
                }
                let res = WaitForSingleObject(h, timeout_ms);
                if res == 0x80 {
                    tracing::warn!(
                        "Mutex matériel {name} abandonné par un processus précédent (WAIT_ABANDONED)"
                    );
                }
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

    // Wrapped in Mutex when cached globally; PawnIoSession only performs stateless IOCTLs on its kernel handle.
    unsafe impl Send for PawnIoSession {}

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
        let rc =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
        if rc == 0 && !hkey.is_null() {
            unsafe { RegCloseKey(hkey) };
            true
        } else {
            false
        }
    }

    fn ensure_service_started() {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let sc = opengear_platform::wmi::system32_path("sc.exe");
        let _ = std::process::Command::new(sc)
            .args(["start", "PawnIO"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }

    impl PawnIoSession {
        pub fn load_file(filename: &str) -> Result<Self> {
            let bytes = load_pawnio_module(filename)?;
            Self::load(&bytes)
        }

        pub fn load(module_bytes: &[u8]) -> Result<Self> {
            if !is_elevated() {
                anyhow::bail!("Droits Administrateur (UAC) requis pour PawnIO");
            }
            let mut h = open_device_raw();
            if (h == INVALID_HANDLE_VALUE || h.is_null()) && is_service_installed() {
                ensure_service_started();
                std::thread::sleep(std::time::Duration::from_millis(120));
                h = open_device_raw();
            }
            if h == INVALID_HANDLE_VALUE || h.is_null() {
                let err = unsafe { GetLastError() };
                anyhow::bail!(
                    "Périphérique PnP Root\\PawnIO absent (Win32 err={err}) — clique sur « Installer / Réparer PawnIO (WHQL) »."
                );
            }

            let in_len = u32::try_from(module_bytes.len()).context("Module PawnIO trop grand")?;
            let mut bytes_ret: u32 = 0;
            let ok = unsafe {
                DeviceIoControl(
                    h,
                    IOCTL_PIO_LOAD_BINARY,
                    module_bytes.as_ptr() as *const core::ffi::c_void,
                    in_len,
                    std::ptr::null_mut(),
                    0,
                    &mut bytes_ret,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                unsafe { CloseHandle(h) };
                anyhow::bail!(
                    "PawnIO a rejeté le chargement du module (plateforme non correspondante, Win32 err={err})"
                );
            }
            Ok(Self { handle: h })
        }

        pub fn execute(&self, fn_name: &str, inputs: &[i64], out_len: usize) -> Result<Vec<i64>> {
            let name_bytes = fn_name.as_bytes();
            if name_bytes.is_empty() || name_bytes.len() >= FN_NAME_LEN {
                anyhow::bail!("Nom de fonction PawnIO invalide : {fn_name:?}");
            }
            let in_payload_bytes = inputs
                .len()
                .checked_mul(8)
                .and_then(|b| b.checked_add(FN_NAME_LEN))
                .context("Dépassement taille entrée PawnIO")?;
            let in_len_u32 =
                u32::try_from(in_payload_bytes).context("Entrée PawnIO trop grande")?;
            let out_bytes_len = out_len
                .checked_mul(8)
                .context("Dépassement taille sortie PawnIO")?;
            let out_len_u32 = u32::try_from(out_bytes_len).context("Sortie PawnIO trop grande")?;

            let mut in_buf = vec![0u8; in_payload_bytes];
            in_buf[..name_bytes.len()].copy_from_slice(name_bytes);
            for (i, &val) in inputs.iter().enumerate() {
                let off = FN_NAME_LEN + i * 8;
                in_buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
            }

            let mut out_raw = vec![0u8; out_bytes_len];
            let mut bytes_ret: u32 = 0;
            let ok = unsafe {
                DeviceIoControl(
                    self.handle,
                    IOCTL_PIO_EXECUTE_FN,
                    in_buf.as_ptr() as *const core::ffi::c_void,
                    in_len_u32,
                    if out_len == 0 {
                        std::ptr::null_mut()
                    } else {
                        out_raw.as_mut_ptr() as *mut core::ffi::c_void
                    },
                    out_len_u32,
                    &mut bytes_ret,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                anyhow::bail!("PawnIO execute '{fn_name}' échoué (Win32 err={err})");
            }
            let count = ((bytes_ret as usize) / 8).min(out_len);
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let off = i * 8;
                out.push(i64::from_le_bytes(
                    out_raw[off..off + 8].try_into().unwrap(),
                ));
            }
            Ok(out)
        }
    }

    enum CachedCpuTemp {
        Amd(PawnIoSession),
        Intel(PawnIoSession),
    }

    static CPU_TEMP_CACHE: OnceLock<Mutex<Option<CachedCpuTemp>>> = OnceLock::new();

    fn read_amd_smn_temp(sess: &PawnIoSession) -> Option<(f32, &'static str)> {
        let _lock = NamedMutexGuard::acquire(r"Global\Access_PCI", 500)?;
        let out = sess.execute("ioctl_read_smn", &[0x0005_9800], 1).ok()?;
        let raw = *out.first()?;
        let v = raw as u32;
        let range_sel = (v & 0x0008_0000) != 0;
        let steps = (v >> 21) & 0x7FF;
        let mut temp = (steps as f32) * 0.125;
        if range_sel {
            temp -= 49.0;
        }
        if (5.0..=125.0).contains(&temp) {
            Some((temp, "PawnIO AMD Ryzen Tctl/Tdie"))
        } else {
            None
        }
    }

    fn read_intel_msr_temp(sess: &PawnIoSession) -> Option<(f32, &'static str)> {
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
            .and_then(|v| v.first().copied())?;
        let valid = (therm & (1 << 31)) != 0;
        if !valid {
            return None;
        }
        let delta = ((therm >> 16) & 0x7F) as f32;
        let temp = tj_max - delta;
        if (5.0..=125.0).contains(&temp) {
            Some((temp, "PawnIO Intel Package Temp"))
        } else {
            None
        }
    }

    pub fn read_native_cpu_temp() -> Option<(f32, &'static str)> {
        if !is_elevated() || !is_service_installed() {
            return None;
        }
        let cache = CPU_TEMP_CACHE.get_or_init(|| Mutex::new(None));
        let mut guard = cache.lock().ok()?;

        if let Some(cached) = guard.as_ref() {
            let res = match cached {
                CachedCpuTemp::Amd(s) => read_amd_smn_temp(s),
                CachedCpuTemp::Intel(s) => read_intel_msr_temp(s),
            };
            if res.is_some() {
                return res;
            }
            *guard = None;
        }

        if let Ok(sess) = PawnIoSession::load_file("AMDFamily17.bin")
            && let Some(res) = read_amd_smn_temp(&sess)
        {
            *guard = Some(CachedCpuTemp::Amd(sess));
            return Some(res);
        }
        if let Ok(sess) = PawnIoSession::load_file("IntelMSR.bin")
            && let Some(res) = read_intel_msr_temp(&sess)
        {
            *guard = Some(CachedCpuTemp::Intel(sess));
            return Some(res);
        }
        None
    }

    // ---- Universal AMD (PIIX4) + Intel (I801) SMBus ENE DDR4/DDR5 RAM RGB ----

    fn smbus_read_byte(sess: &PawnIoSession, addr7: u8, cmd: u8) -> Option<u8> {
        let out = sess
            .execute(
                "ioctl_smbus_xfer",
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

    fn is_micron_dimm(sess: &PawnIoSession, addr7: u8) -> bool {
        let mut mbytes = [0u8; 6];
        for (i, b) in mbytes.iter_mut().enumerate() {
            *b = ene_read_reg(sess, addr7, 0x1030 + i as u16).unwrap_or(0);
        }
        &mbytes == b"Micron"
    }

    const ENE_RAM_ADDRESSES: &[u8] = &[
        0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x66, 0x67, 0x39, 0x3A, 0x3B, 0x3C, 0x3D,
    ];
    const ENE_REG_SLOT_INDEX: u16 = 0x80F8;
    const ENE_REG_I2C_ADDRESS: u16 = 0x80F9;

    fn smbus_read_byte_only(sess: &PawnIoSession, addr7: u8) -> Option<u8> {
        let out = sess
            .execute("ioctl_smbus_xfer", &[addr7 as i64, I2C_SMBUS_READ, 0, 1], 1)
            .ok()?;
        out.first().map(|&v| (v & 0xFF) as u8)
    }

    fn remap_ene_from_0x77(sess: &PawnIoSession) {
        let mut addr_idx: usize = 0;
        for slot in 0u8..8 {
            if smbus_read_byte_only(sess, 0x77).is_none()
                && smbus_read_byte(sess, 0x77, 0x00).is_none()
            {
                break;
            }
            if !test_ene_fingerprint(sess, 0x77) || is_micron_dimm(sess, 0x77) {
                break;
            }
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
            if !test_ene_fingerprint(sess, addr7) || is_micron_dimm(sess, addr7) {
                continue;
            }
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

    pub fn probe_pawnio_ene_ram() -> Vec<EneRamModule> {
        if !is_elevated() || !is_service_installed() {
            return Vec::new();
        }
        let Some(_lock) = NamedMutexGuard::acquire(r"Global\Access_SMBUS.HTP.Method", 1500) else {
            return Vec::new();
        };

        if let Ok(sess) = PawnIoSession::load_file("SmbusPIIX4.bin") {
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

        if let Ok(sess) = PawnIoSession::load_file("SmbusI801.bin") {
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

        let is_intel = targets
            .first()
            .map(|m| m.backend == "pawnio_i801")
            .unwrap_or(false);
        let sess = if is_intel {
            PawnIoSession::load_file("SmbusI801.bin")?
        } else {
            PawnIoSession::load_file("SmbusPIIX4.bin")?
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
                    skipped.push(format!(
                        "0x{:02X} (port {} inaccessible)",
                        m.address_7bit, m.wmi_slave_arg
                    ));
                    continue;
                }
                let addr = m.address_7bit;
                if !test_ene_fingerprint(&sess, addr) || is_micron_dimm(&sess, addr) {
                    skipped.push(format!("0x{addr:02X} (empreinte ENE absente)"));
                    continue;
                }
                let effect_reg: u16 = if m.is_v2 { 0x8160 } else { 0x8010 };
                let leds = m.led_count.clamp(1, 16) as u16;

                ene_write_reg(&sess, addr, 0x8020, 0)?;
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
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                applied += 1;
            }
            Ok(())
        })();

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
            PawnIoSession::load_file("SmbusI801.bin")?
        } else {
            PawnIoSession::load_file("SmbusPIIX4.bin")?
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

        match PawnIoSession::load_file("SmbusPIIX4.bin") {
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

        match PawnIoSession::load_file("SmbusI801.bin") {
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

    pub(super) fn verify_authenticode_file(path: &Path) -> Result<()> {
        use std::ffi::c_void;
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::core::GUID;

        const WINTRUST_ACTION_GENERIC_VERIFY_V2: GUID = GUID {
            data1: 0x00AAC56B,
            data2: 0xCD44,
            data3: 0x11D0,
            data4: [0x8C, 0xC2, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE],
        };
        const WTD_UI_NONE: u32 = 2;
        const WTD_REVOKE_NONE: u32 = 0;
        const WTD_CHOICE_FILE: u32 = 1;
        const WTD_STATEACTION_VERIFY: u32 = 1;
        const WTD_STATEACTION_CLOSE: u32 = 2;

        #[repr(C)]
        struct WintrustFileInfo {
            cb_struct: u32,
            pcwsz_file_path: *const u16,
            h_file: HANDLE,
            pg_known_subject: *const GUID,
        }

        #[repr(C)]
        struct WintrustData {
            cb_struct: u32,
            p_policy_callback_data: *mut c_void,
            p_sip_client_data: *mut c_void,
            dw_ui_choice: u32,
            fdw_revocation_checks: u32,
            dw_union_choice: u32,
            p_file: *mut WintrustFileInfo,
            dw_state_action: u32,
            h_wv_t_state_data: HANDLE,
            pwsz_url_reference: *mut u16,
            dw_prov_flags: u32,
            dw_ui_context: u32,
            p_signature_settings: *mut c_void,
        }

        type FnWinVerifyTrust = unsafe extern "system" fn(HWND, *const GUID, *mut c_void) -> i32;

        let lib = opengear_platform::wmi::load_system32_library("wintrust.dll")?;
        let win_verify_trust: FnWinVerifyTrust = *unsafe { lib.get(b"WinVerifyTrust\0") }?;

        let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut file_info = WintrustFileInfo {
            cb_struct: std::mem::size_of::<WintrustFileInfo>() as u32,
            pcwsz_file_path: wide_path.as_ptr(),
            h_file: std::ptr::null_mut(),
            pg_known_subject: std::ptr::null(),
        };
        let mut data = WintrustData {
            cb_struct: std::mem::size_of::<WintrustData>() as u32,
            p_policy_callback_data: std::ptr::null_mut(),
            p_sip_client_data: std::ptr::null_mut(),
            dw_ui_choice: WTD_UI_NONE,
            fdw_revocation_checks: WTD_REVOKE_NONE,
            dw_union_choice: WTD_CHOICE_FILE,
            p_file: &mut file_info,
            dw_state_action: WTD_STATEACTION_VERIFY,
            h_wv_t_state_data: std::ptr::null_mut(),
            pwsz_url_reference: std::ptr::null_mut(),
            dw_prov_flags: 0,
            dw_ui_context: 0,
            p_signature_settings: std::ptr::null_mut(),
        };

        let status = unsafe {
            win_verify_trust(
                std::ptr::null_mut(),
                &WINTRUST_ACTION_GENERIC_VERIFY_V2,
                &mut data as *mut WintrustData as *mut c_void,
            )
        };
        data.dw_state_action = WTD_STATEACTION_CLOSE;
        unsafe {
            let _ = win_verify_trust(
                std::ptr::null_mut(),
                &WINTRUST_ACTION_GENERIC_VERIFY_V2,
                &mut data as *mut WintrustData as *mut c_void,
            );
        }

        if status != 0 {
            anyhow::bail!(
                "Signature Authenticode invalide ou non approuvée (0x{:08X})",
                status as u32
            );
        }
        Ok(())
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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Download & verify (SHA-256 + Authenticode) the official WHQL-signed `PawnIO_setup.exe` (v2.2.0),
/// then launch it via Windows UAC.
pub fn install_official_pawnio() -> Result<String> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let base_cache = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("OpenGear")
            .join("cache");
        std::fs::create_dir_all(&base_cache)?;

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let stage_dir = base_cache.join(format!("pawnio-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&stage_dir)
            .with_context(|| format!("Impossible de créer {}", stage_dir.display()))?;
        let setup_path = stage_dir.join("PawnIO_setup.exe");
        let url =
            "https://github.com/namazso/PawnIO.Setup/releases/download/2.2.0/PawnIO_setup.exe";

        let ps = opengear_platform::wmi::system32_path(r"WindowsPowerShell\v1.0\powershell.exe");
        let dl_script = format!(
            "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
             Invoke-WebRequest -UseBasicParsing -Uri '{url}' -OutFile '{dst}' -TimeoutSec 25",
            dst = setup_path.display().to_string().replace('\'', "''")
        );
        let out = std::process::Command::new(ps)
            .args(["-NoProfile", "-Command", &dl_script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("Échec téléchargement PawnIO_setup.exe")?;
        if !out.status.success() || !setup_path.exists() {
            let _ = std::fs::remove_dir_all(&stage_dir);
            anyhow::bail!(
                "Téléchargement PawnIO_setup.exe échoué : {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }

        // Verify pinned SHA-256 before elevation
        let bytes = match std::fs::read(&setup_path) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&stage_dir);
                return Err(e).context("Lecture PawnIO_setup.exe impossible");
            }
        };
        let actual_hash = sha256_hex(&bytes);
        if actual_hash != PAWNIO_SETUP_2_2_0_SHA256 {
            let _ = std::fs::remove_dir_all(&stage_dir);
            anyhow::bail!(
                "Empreinte SHA-256 invalide pour PawnIO_setup.exe (attendu {PAWNIO_SETUP_2_2_0_SHA256}, reçu {actual_hash}) — exécution bloquée"
            );
        }

        // Verify Authenticode signature before UAC elevation
        if let Err(e) = win_pio::verify_authenticode_file(&setup_path) {
            let _ = std::fs::remove_dir_all(&stage_dir);
            return Err(e);
        }

        let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = setup_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let args: Vec<u16> = OsStr::new("-install")
            .encode_wide()
            .chain(Some(0))
            .collect();
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
            let _ = std::fs::remove_dir_all(&stage_dir);
            anyhow::bail!("Lancement PawnIO_setup.exe annulé ou refusé (code {rc})");
        }
        Ok("Installateur officiel PawnIO v2.2.0 (vérifié SHA-256 + Authenticode) lancé — clique sur Rafraîchir une fois terminé !".into())
    }
    #[cfg(not(windows))]
    anyhow::bail!("Non supporté hors Windows")
}

#[cfg(test)]
mod tests {
    use super::{is_diag_excluded_addr, load_pawnio_module, sha256_hex};

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

    #[test]
    fn load_pawnio_module_rejects_traversal_and_loads_external_bin() {
        assert!(load_pawnio_module("../LpcIO.bin").is_err());
        assert!(load_pawnio_module("sub/LpcIO.bin").is_err());
        assert!(load_pawnio_module("not_a_module.txt").is_err());
        let bytes =
            load_pawnio_module("LpcIO.bin").expect("LpcIO.bin should load from pawnio_modules/");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
