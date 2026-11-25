//! Windows system events that the hardware state depends on, delivered by a hidden top-level
//! window on its own thread:
//! - sleep / resume (`WM_POWERBROADCAST`, incl. Modern Standby via
//!   `RegisterSuspendResumeNotification`): devices lose their software state while asleep;
//! - session lock / unlock (`WM_WTSSESSION_CHANGE`): optional lights-off while locked;
//! - device arrival / removal (`WM_DEVICECHANGE`), debounced: instant hot-plug without polling.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysEvent {
    Suspend,
    Resume,
    Lock,
    Unlock,
    DevicesChanged,
}

impl SysEvent {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn id(self) -> &'static str {
        match self {
            SysEvent::Suspend => "suspend",
            SysEvent::Resume => "resume",
            SysEvent::Lock => "lock",
            SysEvent::Unlock => "unlock",
            SysEvent::DevicesChanged => "devices_changed",
        }
    }
}

type Callback = Box<dyn Fn(SysEvent) + Send>;
static CALLBACK: OnceLock<Mutex<Option<Callback>>> = OnceLock::new();
static LISTENER_TID: AtomicU32 = AtomicU32::new(0);

fn emit(ev: SysEvent) {
    let Some(cell) = CALLBACK.get() else {
        return;
    };
    // Move the callback out while holding the mutex, invoke it unlocked, then put it back.
    let cb = match cell.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => return,
    };
    if let Some(cb) = cb {
        cb(ev);
        if let Ok(mut guard) = cell.lock() {
            if guard.is_none() {
                *guard = Some(cb);
            }
        }
    }
}

/// Start listening (once per process). `on_event` runs on the listener thread: keep it short
/// (typically: forward to the backend channel).
pub fn start(on_event: impl Fn(SysEvent) + Send + 'static) {
    let cell = CALLBACK.get_or_init(|| Mutex::new(None));
    if let Ok(mut g) = cell.lock() {
        if g.is_some() {
            return;
        }
        *g = Some(Box::new(on_event));
    }
    #[cfg(windows)]
    win::spawn_listener();
}

/// Stop the system event listener thread and clear its callback.
pub fn stop() {
    if let Some(cell) = CALLBACK.get() {
        if let Ok(mut g) = cell.lock() {
            *g = None;
        }
    }
    #[cfg(windows)]
    {
        let tid = LISTENER_TID.swap(0, Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    tid,
                    windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT,
                    0,
                    0,
                );
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use super::{LISTENER_TID, SysEvent, emit};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Power::RegisterSuspendResumeNotification;
    use windows_sys::Win32::System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow,
        DispatchMessageW, GetMessageW, KillTimer, MSG, RegisterClassW, SetTimer, TranslateMessage,
        WNDCLASSW, WS_OVERLAPPED,
    };

    const WM_POWERBROADCAST: u32 = 0x0218;
    const WM_DEVICECHANGE: u32 = 0x0219;
    const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
    const WM_TIMER: u32 = 0x0113;
    const PBT_APMSUSPEND: usize = 0x0004;
    const PBT_APMRESUMESUSPEND: usize = 0x0007;
    const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
    const WTS_SESSION_LOCK: usize = 0x7;
    const WTS_SESSION_UNLOCK: usize = 0x8;
    const DBT_DEVNODES_CHANGED: usize = 0x0007;
    const DBT_DEVICEARRIVAL: usize = 0x8000;
    const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;
    /// Device events arrive in bursts (one per interface): wait for quiet before reporting.
    const DEVICE_DEBOUNCE_TIMER: usize = 1;
    const DEVICE_DEBOUNCE_MS: u32 = 700;

    static RESUMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_POWERBROADCAST => {
                match wparam {
                    PBT_APMSUSPEND => {
                        RESUMED.store(false, Ordering::SeqCst);
                        emit(SysEvent::Suspend);
                    }
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                        if !RESUMED.swap(true, Ordering::SeqCst) {
                            emit(SysEvent::Resume);
                        }
                    }
                    _ => {}
                }
                1
            }
            WM_WTSSESSION_CHANGE => {
                match wparam {
                    WTS_SESSION_LOCK => emit(SysEvent::Lock),
                    WTS_SESSION_UNLOCK => emit(SysEvent::Unlock),
                    _ => {}
                }
                0
            }
            WM_DEVICECHANGE => {
                if matches!(wparam, DBT_DEVNODES_CHANGED | DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE) {
                    unsafe { SetTimer(hwnd, DEVICE_DEBOUNCE_TIMER, DEVICE_DEBOUNCE_MS, None) };
                }
                1
            }
            WM_TIMER if wparam == DEVICE_DEBOUNCE_TIMER => {
                unsafe { KillTimer(hwnd, DEVICE_DEBOUNCE_TIMER) };
                emit(SysEvent::DevicesChanged);
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    pub fn spawn_listener() {
        let _ = std::thread::Builder::new()
            .name("opengear-sysevents".into())
            .spawn(|| unsafe {
                LISTENER_TID.store(GetCurrentThreadId(), Ordering::SeqCst);
                let hinst = GetModuleHandleW(std::ptr::null());
                let class_name = to_wide("OpenGearSystemEvents");
                let wc = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinst,
                    hIcon: std::ptr::null_mut(),
                    hCursor: std::ptr::null_mut(),
                    hbrBackground: std::ptr::null_mut(),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class_name.as_ptr(),
                };
                RegisterClassW(&wc);
                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    class_name.as_ptr(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    hinst,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    LISTENER_TID.store(0, Ordering::SeqCst);
                    tracing::error!("Fenêtre d'événements système non créée : veille / verrouillage non suivis");
                    return;
                }
                WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
                RegisterSuspendResumeNotification(hwnd as _, DEVICE_NOTIFY_WINDOW_HANDLE);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = WTSUnRegisterSessionNotification(hwnd);
                DestroyWindow(hwnd);
                LISTENER_TID.store(0, Ordering::SeqCst);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::SysEvent;

    #[test]
    fn event_ids_are_stable() {
        let all = [
            SysEvent::Suspend,
            SysEvent::Resume,
            SysEvent::Lock,
            SysEvent::Unlock,
            SysEvent::DevicesChanged,
        ];
        let ids: Vec<_> = all.iter().map(|e| e.id()).collect();
        assert_eq!(ids, ["suspend", "resume", "lock", "unlock", "devices_changed"]);
    }
}
