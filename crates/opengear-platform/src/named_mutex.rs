//! Cross-application named mutexes guarding shared hardware buses.
//!
//! Uses the names and NULL DACL that HWiNFO, LibreHardwareMonitor and OpenRGB use, so elevated and
//! non-elevated tools serialise their access to the same bus.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Security::{
    InitializeSecurityDescriptor, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
    SetSecurityDescriptorDacl,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

/// Mutex shared by every SMBus user (HWiNFO, LHM, OpenRGB…).
pub const SMBUS_MUTEX: &str = r"Global\Access_SMBUS.HTP.Method";
/// Mutex shared by every PCI configuration-space user.
pub const PCI_MUTEX: &str = r"Global\Access_PCI";
/// Mutex shared by every SuperIO / ISA bus user.
pub const ISA_MUTEX: &str = r"Global\Access_ISABUS.HTP.Method";
/// Mutex shared by every embedded-controller user.
pub const EC_MUTEX: &str = r"Global\Access_EC";

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
