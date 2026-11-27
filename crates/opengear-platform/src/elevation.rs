//! Process elevation (UAC): detection and relaunch as Administrator.

use anyhow::Result;

/// Check whether current process runs with Administrator privileges (native Win32 TokenElevation — 0 subprocesses).
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 || token.is_null() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret_len: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut TOKEN_ELEVATION as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elev.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Relaunch OpenGear elevated via Windows UAC (`ShellExecuteW("runas")`).
#[cfg(windows)]
pub fn relaunch_as_admin() -> Result<()> {
    use anyhow::Context;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().context("Chemin exécutable introuvable")?;
    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let res = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        if (res as isize) <= 32 {
            anyhow::bail!("Élévation UAC refusée ou échouée (code {})", res as isize);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn relaunch_as_admin() -> Result<()> {
    anyhow::bail!("Non supporté hors Windows")
}

/// Run an executable elevated via Windows UAC (`ShellExecuteExW("runas")`) and wait for completion.
#[cfg(windows)]
pub fn run_elevated_command(exe: &std::path::Path, args: &str) -> Result<()> {
    use anyhow::Context;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let params: Vec<u16> = OsStr::new(args).encode_wide().chain(Some(0)).collect();
    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = verb.as_ptr();
    sei.lpFile = file.as_ptr();
    sei.lpParameters = params.as_ptr();
    sei.nShow = SW_HIDE;

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        return Err(err).context("Élévation UAC refusée ou échouée");
    }
    if sei.hProcess.is_null() {
        anyhow::bail!("Processus élevé introuvable après ShellExecuteExW");
    }
    let wait_rc = unsafe { WaitForSingleObject(sei.hProcess, 15_000) };
    let mut exit_code: u32 = 1;
    let got_code = unsafe { GetExitCodeProcess(sei.hProcess, &mut exit_code) };
    unsafe { CloseHandle(sei.hProcess) };

    if wait_rc != WAIT_OBJECT_0 {
        anyhow::bail!("Délai dépassé en attendant la commande élevée (code {wait_rc})");
    }
    if got_code == 0 || exit_code != 0 {
        anyhow::bail!("La commande élevée a échoué (code {exit_code})");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn run_elevated_command(_exe: &std::path::Path, _args: &str) -> Result<()> {
    anyhow::bail!("Non supporté hors Windows")
}
