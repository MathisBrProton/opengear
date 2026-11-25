//! Native Windows System Tray, procedural OpenGear brand icon (window + tray + .ico),
//! Windows autostart registry toggle, and 1-click Desktop / Start Menu shortcut creator.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tao::event_loop::EventLoopProxy;

#[derive(Debug, Clone)]
pub enum TrayAction {
    ShowWindow,
    ToggleSocd,
    ToggleSyncRgb,
    Quit,
}

/// Generate a crisp anti-aliased RGBA OpenGear logo (obsidian squircle + glowing cyan/blue rim + lightning bolt).
pub fn render_opengear_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let bolt = [
        (0.564f32, 0.175f32),
        (0.285f32, 0.545f32),
        (0.495f32, 0.545f32),
        (0.425f32, 0.825f32),
        (0.715f32, 0.455f32),
        (0.505f32, 0.455f32),
    ];

    for py in 0..size {
        for px in 0..size {
            let mut r_acc = 0.0f32;
            let mut g_acc = 0.0f32;
            let mut b_acc = 0.0f32;
            let mut a_acc = 0.0f32;

            // 2x2 supersampling for razor-sharp edges at 16/32/64px
            for sy in 0..2 {
                for sx in 0..2 {
                    let u = (px as f32 + (sx as f32 + 0.5) * 0.5) / s;
                    let v = (py as f32 + (sy as f32 + 0.5) * 0.5) / s;

                    let (r, g, b, a) = sample_logo_pixel(u, v, &bolt);
                    r_acc += r * a;
                    g_acc += g * a;
                    b_acc += b * a;
                    a_acc += a;
                }
            }

            let idx = ((py * size + px) * 4) as usize;
            let a = (a_acc * 0.25).clamp(0.0, 1.0);
            if a > 0.001 {
                rgba[idx] = ((r_acc / a_acc) * 255.0).clamp(0.0, 255.0) as u8;
                rgba[idx + 1] = ((g_acc / a_acc) * 255.0).clamp(0.0, 255.0) as u8;
                rgba[idx + 2] = ((b_acc / a_acc) * 255.0).clamp(0.0, 255.0) as u8;
                rgba[idx + 3] = (a * 255.0).clamp(0.0, 255.0) as u8;
            }
        }
    }
    rgba
}

fn sample_logo_pixel(u: f32, v: f32, bolt: &[(f32, f32); 6]) -> (f32, f32, f32, f32) {
    // Rounded-square SDF centered at (0.5, 0.5), half-size 0.45, corner radius 0.19
    let dx = (u - 0.5).abs() - 0.26;
    let dy = (v - 0.5).abs() - 0.26;
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);
    let sdf = outside + inside - 0.19;

    if sdf > 0.02 {
        return (0.0, 0.0, 0.0, 0.0);
    }

    // Diagonal cyan (#00D4FF) -> electric blue (#3B82F6) brand gradient
    let t = ((u * 0.75 + v * 0.65) - 0.12).clamp(0.0, 1.0);
    let gr = 0.0 + t * 0.231;
    let gg = 0.831 + t * (0.510 - 0.831);
    let gb = 1.0 + t * (0.965 - 1.0);

    // Base dark carbon gradient inside squircle
    let bg_t = (v * 0.8 + u * 0.2).clamp(0.0, 1.0);
    let mut r = 0.045 + bg_t * 0.03;
    let mut g = 0.075 + bg_t * 0.04;
    let mut b = 0.135 + bg_t * 0.065;

    // Subtle radial center glow
    let dist_c = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
    let glow = (1.0 - dist_c * 2.1).clamp(0.0, 1.0).powi(2) * 0.30;
    r += gr * glow;
    g += gg * glow;
    b += gb * glow;

    // Outer luminous cyan-blue border ring
    if sdf > -0.068 {
        r = gr;
        g = gg;
        b = gb;
    }

    // Crisp lightning bolt polygon
    if point_in_poly(u, v, bolt) {
        r = (gr * 0.55 + 0.45).min(1.0);
        g = (gg * 0.65 + 0.35).min(1.0);
        b = 1.0;
    }

    let alpha = if sdf > 0.0 {
        (1.0 - sdf / 0.02).clamp(0.0, 1.0)
    } else {
        1.0
    };
    (
        r.clamp(0.0, 1.0),
        g.clamp(0.0, 1.0),
        b.clamp(0.0, 1.0),
        alpha,
    )
}

fn point_in_poly(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        let intersect =
            ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi + 1e-9) + xi);
        if intersect {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Build a `tao::window::Icon` for the main OpenGear window & Windows taskbar.
pub fn build_window_icon() -> Option<tao::window::Icon> {
    let rgba = render_opengear_rgba(64);
    tao::window::Icon::from_rgba(rgba, 64, 64).ok()
}

/// Write a standard Windows `.ico` file (32×32 + 64×64 32-bit BGRA BMP-in-ICO) next to `opengear.exe`
/// so Desktop & Start Menu shortcuts render the custom OpenGear icon natively.
pub fn ensure_ico_beside_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let ico_path = dir.join("opengear.ico");
    let bytes = encode_multi_ico(&[32, 64]);
    let _ = std::fs::write(&ico_path, bytes);
    Some(ico_path)
}

fn encode_multi_ico(sizes: &[u32]) -> Vec<u8> {
    let mut images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&sz| (sz, encode_ico_bmp_entry(sz)))
        .collect();

    let count = images.len() as u16;
    let mut out = Vec::new();
    // ICONDIR header
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type = 1 (ICO)
    out.extend_from_slice(&count.to_le_bytes());

    let mut offset = 6u32 + (images.len() as u32) * 16;
    for (sz, data) in &images {
        out.push(if *sz >= 256 { 0 } else { *sz as u8 });
        out.push(if *sz >= 256 { 0 } else { *sz as u8 });
        out.push(0); // palette
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bpp
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images.drain(..) {
        out.extend_from_slice(&data);
    }
    out
}

fn encode_ico_bmp_entry(size: u32) -> Vec<u8> {
    let rgba = render_opengear_rgba(size);
    let and_row_bytes = size.div_ceil(32) * 4;
    let and_size = and_row_bytes * size;
    let xor_size = size * size * 4;

    let mut buf = Vec::with_capacity(40 + (xor_size + and_size) as usize);
    // BITMAPINFOHEADER (height is 2*size per ICO spec)
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(size as i32).to_le_bytes());
    buf.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // planes
    buf.extend_from_slice(&32u16.to_le_bytes()); // bpp
    buf.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    buf.extend_from_slice(&xor_size.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Bottom-up BGRA pixels
    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            buf.push(rgba[idx + 2]); // B
            buf.push(rgba[idx + 1]); // G
            buf.push(rgba[idx]); // R
            buf.push(rgba[idx + 3]); // A
        }
    }
    // AND mask (all 0s — alpha channel handles transparency)
    buf.resize(buf.len() + and_size as usize, 0);
    buf
}

// ================= NATIVE WIN32 SYSTEM TRAY (Shell_NotifyIconW) =================

#[cfg(windows)]
mod win_tray {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
        HICON, HWND_BROADCAST, ICONINFO, MF_SEPARATOR, MF_STRING, MSG, PostMessageW,
        PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
        TPM_BOTTOMALIGN, TPM_LEFTALIGN, TrackPopupMenu, TranslateMessage, WM_APP, WM_COMMAND,
        WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
    };

    const WM_TRAY_CALLBACK: u32 = WM_APP + 42;
    const TRAY_UID: u32 = 1001;
    const CMD_SHOW: usize = 101;
    const CMD_TOGGLE_SOCD: usize = 102;
    const CMD_TOGGLE_SYNC: usize = 103;
    const CMD_QUIT: usize = 109;

    type TraySender = Box<dyn Fn(TrayAction) + Send + Sync>;
    static TRAY_SENDER: OnceLock<std::sync::Mutex<Option<TraySender>>> = OnceLock::new();
    static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
    static TRAY_HICON: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
    static TRAY_ALIVE: AtomicBool = AtomicBool::new(false);
    static WAKE_MSG_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static TASKBAR_CREATED_MSG_ID: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(0);
    static GRACEFUL_QUIT_MSG_ID: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(0);

    pub fn taskbar_created_msg_id() -> u32 {
        let id = TASKBAR_CREATED_MSG_ID.load(Ordering::Relaxed);
        if id != 0 {
            return id;
        }
        let w = to_wide("TaskbarCreated");
        let reg = unsafe { RegisterWindowMessageW(w.as_ptr()) };
        TASKBAR_CREATED_MSG_ID.store(reg, Ordering::Relaxed);
        reg
    }

    pub fn graceful_quit_msg_id() -> u32 {
        let id = GRACEFUL_QUIT_MSG_ID.load(Ordering::Relaxed);
        if id != 0 {
            return id;
        }
        let w = to_wide("OpenGearCommandCenterGracefulQuit");
        let reg = unsafe { RegisterWindowMessageW(w.as_ptr()) };
        GRACEFUL_QUIT_MSG_ID.store(reg, Ordering::Relaxed);
        reg
    }

    unsafe fn register_tray_icon(hwnd: HWND, hicon: HICON) {
        unsafe {
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = TRAY_UID;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY_CALLBACK;
            nid.hIcon = hicon;
            let tip = to_wide("OpenGear — Command Center");
            let copy_len = tip.len().min(nid.szTip.len());
            nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);
            Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    pub fn wake_message_id() -> u32 {
        let cur = WAKE_MSG_ID.load(Ordering::Relaxed);
        if cur != 0 {
            return cur;
        }
        let name = to_wide("OpenGearSingleInstanceWakeUp");
        let id = unsafe { RegisterWindowMessageW(name.as_ptr()) };
        WAKE_MSG_ID.store(id, Ordering::Relaxed);
        id
    }

    pub fn broadcast_wakeup() {
        let id = wake_message_id();
        if id != 0 {
            unsafe {
                PostMessageW(HWND_BROADCAST, id, 0, 0);
            }
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    unsafe fn create_hicon_from_rgba(size: u32) -> HICON {
        let rgba = render_opengear_rgba(size);
        let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size as i32;
        bmi.bmiHeader.biHeight = -(size as i32); // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbm_color = unsafe {
            CreateDIBSection(
                std::ptr::null_mut(),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            )
        };
        if !bits.is_null() {
            let dst = unsafe {
                std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize)
            };
            for i in (0..rgba.len()).step_by(4) {
                dst[i] = rgba[i + 2]; // B
                dst[i + 1] = rgba[i + 1]; // G
                dst[i + 2] = rgba[i]; // R
                dst[i + 3] = rgba[i + 3]; // A
            }
        }
        let hbm_mask = unsafe { CreateBitmap(size as i32, size as i32, 1, 1, std::ptr::null()) };
        let ii = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hbm_mask,
            hbmColor: hbm_color,
        };
        let hicon = unsafe { CreateIconIndirect(&ii) };
        unsafe {
            DeleteObject(hbm_color);
            DeleteObject(hbm_mask);
        }
        hicon
    }

    fn emit(act: TrayAction) {
        if let Some(lock) = TRAY_SENDER.get()
            && let Ok(guard) = lock.lock()
            && let Some(cb) = guard.as_ref()
        {
            cb(act);
        }
    }

    unsafe extern "system" fn tray_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_TRAY_CALLBACK => {
                let ev = (lparam & 0xFFFF) as u32;
                if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                    emit(TrayAction::ShowWindow);
                } else if ev == WM_RBUTTONUP {
                    let hmenu = unsafe { CreatePopupMenu() };
                    if !hmenu.is_null() {
                        let s_open = to_wide("Ouvrir OpenGear");
                        let s_socd = to_wide("Basculer SOCD / Snap Tap");
                        let s_sync = to_wide("Basculer OpenGear Sync RGB");
                        let s_quit = to_wide("Quitter OpenGear");
                        unsafe {
                            AppendMenuW(hmenu, MF_STRING, CMD_SHOW, s_open.as_ptr());
                            AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
                            AppendMenuW(hmenu, MF_STRING, CMD_TOGGLE_SOCD, s_socd.as_ptr());
                            AppendMenuW(hmenu, MF_STRING, CMD_TOGGLE_SYNC, s_sync.as_ptr());
                            AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
                            AppendMenuW(hmenu, MF_STRING, CMD_QUIT, s_quit.as_ptr());
                            let mut pt = POINT { x: 0, y: 0 };
                            GetCursorPos(&mut pt);
                            SetForegroundWindow(hwnd);
                            TrackPopupMenu(
                                hmenu,
                                TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                                pt.x,
                                pt.y,
                                0,
                                hwnd,
                                std::ptr::null(),
                            );
                            PostMessageW(hwnd, WM_NULL, 0, 0);
                            DestroyMenu(hmenu);
                        }
                    }
                }
                0
            }
            WM_COMMAND => {
                match wparam & 0xFFFF {
                    CMD_SHOW => emit(TrayAction::ShowWindow),
                    CMD_TOGGLE_SOCD => emit(TrayAction::ToggleSocd),
                    CMD_TOGGLE_SYNC => emit(TrayAction::ToggleSyncRgb),
                    CMD_QUIT => emit(TrayAction::Quit),
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                0
            }
            other if other != 0 && other == wake_message_id() => {
                emit(TrayAction::ShowWindow);
                0
            }
            other if other != 0 && other == graceful_quit_msg_id() => {
                emit(TrayAction::Quit);
                0
            }
            other if other != 0 && other == taskbar_created_msg_id() => {
                let hicon = TRAY_HICON.load(Ordering::SeqCst) as HICON;
                if !hicon.is_null() {
                    unsafe { register_tray_icon(hwnd, hicon) };
                }
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    pub fn start<F>(on_action: F)
    where
        F: Fn(TrayAction) + Send + Sync + 'static,
    {
        if TRAY_ALIVE.swap(true, Ordering::SeqCst) {
            return;
        }
        let cell = TRAY_SENDER.get_or_init(|| std::sync::Mutex::new(None));
        if let Ok(mut g) = cell.lock() {
            *g = Some(Box::new(on_action));
        }

        std::thread::Builder::new()
            .name("opengear-tray".into())
            .spawn(|| unsafe {
                let hinst = GetModuleHandleW(std::ptr::null());
                let class_name = to_wide("OpenGearTrayWindowClass");
                let wc = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(tray_wnd_proc),
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
                    0,
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
                    TRAY_ALIVE.store(false, Ordering::SeqCst);
                    return;
                }
                TRAY_HWND.store(hwnd as isize, Ordering::SeqCst);

                let hicon = create_hicon_from_rgba(32);
                TRAY_HICON.store(hicon as isize, Ordering::SeqCst);
                let _ = taskbar_created_msg_id();
                let _ = graceful_quit_msg_id();
                let _ = wake_message_id();
                register_tray_icon(hwnd, hicon);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = TRAY_UID;
                Shell_NotifyIconW(NIM_DELETE, &nid);
                TRAY_HICON.store(0, Ordering::SeqCst);
                if !hicon.is_null() {
                    DestroyIcon(hicon);
                }
                DestroyWindow(hwnd);
                TRAY_HWND.store(0, Ordering::SeqCst);
                TRAY_ALIVE.store(false, Ordering::SeqCst);
            })
            .ok();
    }

    /// Show a Windows notification (tray balloon, rendered as a toast on Windows 10/11).
    pub fn notify(title: &str, body: &str) -> bool {
        use windows_sys::Win32::UI::Shell::{NIF_INFO, NIIF_WARNING, NIM_MODIFY};
        let raw = TRAY_HWND.load(Ordering::SeqCst);
        if raw == 0 {
            return false;
        }
        unsafe {
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = raw as HWND;
            nid.uID = TRAY_UID;
            nid.uFlags = NIF_INFO;
            nid.dwInfoFlags = NIIF_WARNING;
            let t = to_wide(title);
            let n = t.len().min(nid.szInfoTitle.len() - 1);
            nid.szInfoTitle[..n].copy_from_slice(&t[..n]);
            let b = to_wide(body);
            let n = b.len().min(nid.szInfo.len() - 1);
            nid.szInfo[..n].copy_from_slice(&b[..n]);
            Shell_NotifyIconW(NIM_MODIFY, &nid) != 0
        }
    }

    pub fn stop() {
        let raw = TRAY_HWND.load(Ordering::SeqCst);
        if raw != 0 {
            unsafe {
                PostMessageW(raw as HWND, WM_DESTROY, 0, 0);
            }
        }
    }
}

pub fn start_system_tray<E, F>(proxy: EventLoopProxy<E>, mapper: F)
where
    E: 'static + Send,
    F: Fn(TrayAction) -> E + Send + Sync + 'static,
{
    #[cfg(windows)]
    win_tray::start(move |act| {
        let _ = proxy.send_event(mapper(act));
    });
    #[cfg(not(windows))]
    let _ = (proxy, mapper);
}

pub fn stop_system_tray() {
    #[cfg(windows)]
    win_tray::stop();
}

pub fn wake_message_id() -> u32 {
    #[cfg(windows)]
    {
        win_tray::wake_message_id()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Windows notification from the tray icon. Returns `false` if the tray is not running.
pub fn tray_notify(title: &str, body: &str) -> bool {
    #[cfg(windows)]
    {
        win_tray::notify(title, body)
    }
    #[cfg(not(windows))]
    {
        let _ = (title, body);
        false
    }
}

#[cfg(windows)]
static SINGLE_INSTANCE_MUTEX: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);

/// Ensure only ONE OpenGear GUI instance runs at a time.
/// If another instance is already running (even hidden in the System Tray), sends a wake-up signal
/// so the existing window pops open immediately and returns `false` (caller should exit cleanly).
pub fn acquire_single_instance() -> bool {
    #[cfg(windows)]
    unsafe {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Security::{
            InitializeSecurityDescriptor, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            SECURITY_DESCRIPTOR, SetSecurityDescriptorDacl,
        };
        use windows_sys::Win32::Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError,
        };
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let mut sd = SECURITY_DESCRIPTOR::default();
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: false.into(),
        };
        let psd = PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _);
        let sa_ptr = if InitializeSecurityDescriptor(psd, 1).is_ok()
            && SetSecurityDescriptorDacl(psd, true, None, false).is_ok()
        {
            sa.lpSecurityDescriptor = &mut sd as *mut _ as *mut _;
            &sa as *const SECURITY_ATTRIBUTES as *const _
        } else {
            std::ptr::null()
        };

        let name: Vec<u16> = OsStr::new("Local\\OpenGearCommandCenterSingleInstance")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let h = CreateMutexW(sa_ptr, 1, name.as_ptr());
        let err = GetLastError();
        if err == ERROR_ALREADY_EXISTS || err == ERROR_ACCESS_DENIED {
            if !h.is_null() {
                CloseHandle(h);
            }
            win_tray::broadcast_wakeup();
            return false;
        }
        SINGLE_INSTANCE_MUTEX.store(h as isize, Ordering::SeqCst);
        true
    }
    #[cfg(not(windows))]
    true
}

/// Explicitly release the single-instance mutex before spawning an elevated Admin replacement process.
pub fn release_single_instance() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        let raw = SINGLE_INSTANCE_MUTEX.swap(0, Ordering::SeqCst);
        if raw != 0 {
            CloseHandle(raw as HANDLE);
        }
    }
}

/// Ask Windows to reclaim unused physical RAM pages when OpenGear enters Ultra-Light Tray mode.
pub fn trim_working_set() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Memory::SetProcessWorkingSetSizeEx;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let _ = SetProcessWorkingSetSizeEx(GetCurrentProcess(), usize::MAX, usize::MAX, 0);
    }
}

// ================= WINDOWS AUTOSTART (HKCU Run) & SHORTCUTS =================

pub fn is_autostart_enabled() -> bool {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
        };
        let subkey: Vec<u16> = OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let val_name: Vec<u16> = OsStr::new("OpenGear")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut hkey: HKEY = std::ptr::null_mut();
        let open_rc =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
        if open_rc != 0 {
            return false;
        }
        let mut sz: u32 = 0;
        let query_rc = unsafe {
            RegQueryValueExW(
                hkey,
                val_name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sz,
            )
        };
        unsafe { RegCloseKey(hkey) };
        query_rc == 0 && sz > 0
    }
    #[cfg(not(windows))]
    false
}

pub fn set_autostart_enabled(enabled: bool, start_minimized: bool) -> Result<()> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCloseKey, RegDeleteValueW,
            RegOpenKeyExW, RegSetValueExW,
        };
        let subkey: Vec<u16> = OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let val_name: Vec<u16> = OsStr::new("OpenGear")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_WRITE, &mut hkey) };
        if rc != 0 {
            anyhow::bail!("Impossible d'ouvrir HKCU\\...\\Run (code {rc})");
        }
        if enabled {
            let exe = std::env::current_exe().context("current_exe")?;
            let cmd = if start_minimized {
                format!("\"{}\" --minimized", exe.display())
            } else {
                format!("\"{}\"", exe.display())
            };
            let wide: Vec<u16> = OsStr::new(&cmd).encode_wide().chain(Some(0)).collect();
            let set_rc = unsafe {
                RegSetValueExW(
                    hkey,
                    val_name.as_ptr(),
                    0,
                    REG_SZ,
                    wide.as_ptr() as *const u8,
                    (wide.len() * 2) as u32,
                )
            };
            unsafe { RegCloseKey(hkey) };
            if set_rc != 0 {
                anyhow::bail!("Échec écriture registre démarrage Windows (code {set_rc})");
            }
        } else {
            unsafe {
                RegDeleteValueW(hkey, val_name.as_ptr());
                RegCloseKey(hkey);
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (enabled, start_minimized);
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Create crisp `.lnk` shortcuts with the custom OpenGear `.ico` on Desktop & Start Menu.
#[cfg(windows)]
fn known_folder_path(id: &windows::core::GUID) -> Result<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        let pwstr =
            SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).context("SHGetKnownFolderPath")?;
        let mut len = 0usize;
        while *pwstr.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(pwstr.0, len);
        let os = String::from_utf16_lossy(slice);
        CoTaskMemFree(Some(pwstr.0 as *const _));
        Ok(PathBuf::from(os))
    }
}

#[cfg(windows)]
fn create_shortcut_com(
    lnk_path: &Path,
    target_exe: &Path,
    work_dir: &Path,
    ico_path: &Path,
    description: &str,
) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{Interface, PCWSTR};

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let should_uninit = hr.is_ok();
        let res = (|| -> Result<()> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .context("CoCreateInstance(ShellLink)")?;
            let w_exe = wide(target_exe.as_os_str());
            let w_dir = wide(work_dir.as_os_str());
            let w_ico = wide(ico_path.as_os_str());
            let w_desc = wide(OsStr::new(description));
            let w_lnk = wide(lnk_path.as_os_str());

            link.SetPath(PCWSTR(w_exe.as_ptr())).context("SetPath")?;
            link.SetWorkingDirectory(PCWSTR(w_dir.as_ptr()))
                .context("SetWorkingDirectory")?;
            link.SetIconLocation(PCWSTR(w_ico.as_ptr()), 0)
                .context("SetIconLocation")?;
            link.SetDescription(PCWSTR(w_desc.as_ptr()))
                .context("SetDescription")?;

            let pf: IPersistFile = link.cast().context("QueryInterface(IPersistFile)")?;
            pf.Save(PCWSTR(w_lnk.as_ptr()), true)
                .context("IPersistFile::Save")?;
            Ok(())
        })();
        if should_uninit {
            CoUninitialize();
        }
        res
    }
}

pub fn create_windows_shortcuts() -> Result<String> {
    let exe = std::env::current_exe().context("current_exe")?;
    create_shortcuts_for(&exe)
}

pub fn create_shortcuts_for(exe: &Path) -> Result<String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs};

        let dir = exe.parent().unwrap_or_else(|| Path::new("."));
        let ico = dir.join("opengear.ico");
        if !ico.exists() {
            let _ = std::fs::write(&ico, encode_multi_ico(&[32, 64]));
        }
        let desktop = known_folder_path(&FOLDERID_Desktop)?.join("OpenGear.lnk");
        let start_menu = known_folder_path(&FOLDERID_Programs)?.join("OpenGear.lnk");
        for dest in [&desktop, &start_menu] {
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            create_shortcut_com(
                dest,
                exe,
                dir,
                &ico,
                "OpenGear — Command Center Matériel Unifié",
            )?;
        }
        Ok(
            "Raccourcis créés sur le Bureau et dans le Menu Démarrer (avec icône OpenGear) !"
                .into(),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Remove a temporary installer extraction folder only if it resolves cleanly inside `temp_dir`
/// and begins with `opengear_setup_`.
pub fn cleanup_installer_temp_dir(dir: &Path) {
    let Ok(base) = std::env::temp_dir().canonicalize() else {
        return;
    };
    let Ok(target) = dir.canonicalize() else {
        return;
    };
    let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    if !target.starts_with(&base) || !name.starts_with("opengear_setup_") {
        return;
    }
    for _ in 0..10 {
        if std::fs::remove_dir_all(&target).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(windows)]
fn request_running_instances_exit_gracefully() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, HWND_BROADCAST, PostMessageW};
    let msg_id = win_tray::graceful_quit_msg_id();
    unsafe {
        PostMessageW(HWND_BROADCAST, msg_id, 0, 0);
    }
    let class_name: Vec<u16> = OsStr::new("OpenGearTrayWindowClass")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        let hwnd = unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) };
        if hwnd.is_null() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(windows)]
fn copy_dir_files(src: &Path, dst: &Path) {
    if !src.is_dir() {
        return;
    }
    if std::fs::create_dir_all(dst).is_err() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file()
                && let Some(name) = p.file_name()
            {
                let _ = std::fs::copy(&p, dst.join(name));
            }
        }
    }
}

/// Self-install OpenGear cleanly into `%LOCALAPPDATA%\Programs\OpenGear\`:
/// - Signals any running instance to quit gracefully (waiting up to 2 s before fallback)
/// - Copies `OpenGear.exe`, `WebView2Loader.dll`, `pawnio_modules/`, and `opengear.ico`
/// - Registers OpenGear in Windows Installed Apps (HKCU Uninstall key)
/// - Creates Desktop & Start Menu shortcuts via native COM `IShellLinkW`
/// - Launches the installed `OpenGear.exe` (passing `--cleanup-dir` when invoked from setup)
pub fn install_to_windows(cleanup_dir: Option<&Path>) -> Result<()> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCloseKey, RegCreateKeyExW,
            RegSetValueExW,
        };

        let local_app_data = std::env::var("LOCALAPPDATA")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .context("Variable LOCALAPPDATA introuvable")?;
        let install_dir = PathBuf::from(&local_app_data)
            .join("Programs")
            .join("OpenGear");
        std::fs::create_dir_all(&install_dir)
            .with_context(|| format!("Impossible de créer {}", install_dir.display()))?;

        let src_exe = std::env::current_exe().context("current_exe")?;
        let src_dir = src_exe.parent().unwrap_or_else(|| Path::new("."));
        let dst_exe = install_dir.join("OpenGear.exe");
        let dst_dll = install_dir.join("WebView2Loader.dll");
        let dst_ico = install_dir.join("opengear.ico");

        // Ask any existing OpenGear instance to quit gracefully first so fans/RGB restore cleanly.
        request_running_instances_exit_gracefully();

        // Fallback termination only if the previous process did not exit or still locks dst_exe.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let my_pid = std::process::id();
        let pid_filter = format!("PID ne {my_pid}");
        if src_exe != dst_exe && std::fs::copy(&src_exe, &dst_exe).is_err() {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/IM", "OpenGear.exe", "/FI", &pid_filter])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            std::thread::sleep(std::time::Duration::from_millis(300));
            std::fs::copy(&src_exe, &dst_exe)
                .with_context(|| format!("Échec copie vers {}", dst_exe.display()))?;
        }
        let src_dll = src_dir.join("WebView2Loader.dll");
        if src_dll.exists() && src_dll != dst_dll {
            let _ = std::fs::copy(&src_dll, &dst_dll);
        }
        copy_dir_files(
            &src_dir.join("pawnio_modules"),
            &install_dir.join("pawnio_modules"),
        );
        std::fs::write(&dst_ico, encode_multi_ico(&[32, 64]))?;

        // Create Desktop & Start Menu shortcuts pointing to installed OpenGear.exe
        let _ = create_shortcuts_for(&dst_exe);

        // Register in Windows "Applications installées" (HKCU Uninstall)
        let subkey: Vec<u16> =
            OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OpenGear")
                .encode_wide()
                .chain(Some(0))
                .collect();
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };
        if rc == 0 && !hkey.is_null() {
            let entries = [
                ("DisplayName", "OpenGear — Command Center".to_string()),
                ("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
                ("Publisher", "OpenGear".to_string()),
                ("InstallLocation", install_dir.display().to_string()),
                ("DisplayIcon", dst_ico.display().to_string()),
                (
                    "UninstallString",
                    format!("\"{}\" --uninstall", dst_exe.display()),
                ),
            ];
            for (k, v) in entries {
                let wk: Vec<u16> = OsStr::new(k).encode_wide().chain(Some(0)).collect();
                let wv: Vec<u16> = OsStr::new(&v).encode_wide().chain(Some(0)).collect();
                unsafe {
                    let _ = RegSetValueExW(
                        hkey,
                        wk.as_ptr(),
                        0,
                        REG_SZ,
                        wv.as_ptr() as *const u8,
                        (wv.len() * 2) as u32,
                    );
                }
            }
            unsafe { RegCloseKey(hkey) };
        }

        // Launch installed OpenGear GUI cleanly (zero console window!)
        let mut launch = std::process::Command::new(&dst_exe);
        launch.current_dir(&install_dir);
        if let Some(dir) = cleanup_dir {
            launch.arg("--cleanup-dir").arg(dir);
        }
        let _ = launch.spawn();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = cleanup_dir;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Cleanly uninstall OpenGear shortcuts, autostart entry, uninstall registry key, and install folder.
pub fn uninstall_from_windows() -> Result<()> {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::process::CommandExt;
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs};
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};

        let _ = set_autostart_enabled(false, false);

        let subkey: Vec<u16> =
            OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OpenGear")
                .encode_wide()
                .chain(Some(0))
                .collect();
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, subkey.as_ptr());
        }

        if let Ok(desktop) = known_folder_path(&FOLDERID_Desktop) {
            let _ = std::fs::remove_file(desktop.join("OpenGear.lnk"));
        }
        if let Ok(programs) = known_folder_path(&FOLDERID_Programs) {
            let _ = std::fs::remove_file(programs.join("OpenGear.lnk"));
        }

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let local_app_data = std::env::var("LOCALAPPDATA")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .context("Variable LOCALAPPDATA introuvable — suppression annulée par sécurité")?;
        let install_dir = PathBuf::from(&local_app_data)
            .join("Programs")
            .join("OpenGear");
        let is_expected_target = install_dir.file_name() == Some(OsStr::new("OpenGear"))
            && install_dir.parent().and_then(|p| p.file_name()) == Some(OsStr::new("Programs"))
            && install_dir.is_absolute();
        anyhow::ensure!(
            is_expected_target,
            "Chemin d'installation inattendu refusé : {}",
            install_dir.display()
        );

        let sys_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let cmd_exe = PathBuf::from(sys_root).join("System32").join("cmd.exe");
        let rm_cmd = format!(
            "timeout /T 1 /NOBREAK > nul & rmdir /S /Q \"{}\"",
            install_dir.display()
        );
        let _ = std::process::Command::new(cmd_exe)
            .args(["/C", &rm_cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        Ok(())
    }
    #[cfg(not(windows))]
    anyhow::bail!("Non supporté hors Windows")
}
