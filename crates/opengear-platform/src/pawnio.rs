//! PawnIO kernel driver broker (`namazso/PawnIO`, WHQL-signed).
//!
//! Loads the official signed PawnIO bytecode modules shipped as external, replaceable files in
//! `pawnio_modules/` (LGPL-2.1) and executes their functions. What the modules are used for
//! (SMBus, SuperIO, CPU MSR…) lives in `opengear-mainboard`; this module only knows the driver.

use crate::is_elevated;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Pinned SHA-256 for official `PawnIO_setup.exe` v2.2.0 (`namazso/PawnIO.Setup`).
const PAWNIO_SETUP_2_2_0_SHA256: &str =
    "1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032";

/// Load an external PawnIO bytecode module (`.bin`) from `pawnio_modules/`.
/// Rejects path traversal and checks beside the executable, parent dir, and dev manifest dir.
pub fn load_pawnio_module(filename: &str) -> Result<Vec<u8>> {
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
        CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const DEVICE_TYPE: u32 = 41394u32 << 16;
    const IOCTL_PIO_LOAD_BINARY: u32 = DEVICE_TYPE | (0x821 << 2);
    const IOCTL_PIO_EXECUTE_FN: u32 = DEVICE_TYPE | (0x841 << 2);
    const FN_NAME_LEN: usize = 32;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
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
        let sc = crate::wmi::system32_path("sc.exe");
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

        let lib = crate::wmi::load_system32_library("wintrust.dll")?;
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
pub use win_pio::PawnIoSession;

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

        let ps = crate::wmi::system32_path(r"WindowsPowerShell\v1.0\powershell.exe");
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
    use super::{load_pawnio_module, sha256_hex};

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
