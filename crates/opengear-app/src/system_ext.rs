//! Native Windows system extensions:
//! - Foreground executable watcher (for per-game Auto-Profile Switch & CS2 SOCD anti-kick guard)
//! - Running visible apps list
//! - Ultra-fast GDI Ambilight screen color grid sampler (< 1.5 ms, zero external crates)

use opengear_core::Rgb;

/// Return the lowercase `.exe` filename of the currently focused Windows application (e.g. `cs2.exe`).
/// Time since the last keyboard / mouse input in this session (`GetLastInputInfo`).
pub fn idle_duration() -> Option<std::time::Duration> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::GetTickCount;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info) == 0 {
            return None;
        }
        // Both are 32-bit tick counts: wrapping_sub handles the 49.7-day rollover.
        let idle_ms = GetTickCount().wrapping_sub(info.dwTime);
        Some(std::time::Duration::from_millis(idle_ms as u64))
    }
    #[cfg(not(windows))]
    None
}

pub fn get_foreground_exe_name() -> Option<String> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 || len == 0 {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&full)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
    }
    #[cfg(not(windows))]
    None
}

/// List visible user application `.exe` names currently open on the desktop (for 1-click profile linking).
pub fn list_visible_apps() -> Vec<String> {
    #[cfg(windows)]
    {
        use std::collections::BTreeSet;
        use windows_sys::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
        };

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                if IsWindowVisible(hwnd) == 0 || GetWindowTextLengthW(hwnd) == 0 {
                    return 1;
                }
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == 0 {
                    return 1;
                }
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if !handle.is_null() {
                    let mut buf = [0u16; 512];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) != 0
                        && len > 0
                    {
                        let full = String::from_utf16_lossy(&buf[..len as usize]);
                        if let Some(name) =
                            std::path::Path::new(&full).file_name().and_then(|s| s.to_str())
                        {
                            let lower = name.to_ascii_lowercase();
                            if !matches!(
                                lower.as_str(),
                                "opengear.exe"
                                    | "explorer.exe"
                                    | "textinputhost.exe"
                                    | "applicationframehost.exe"
                                    | "shellexperiencehost.exe"
                                    | "searchhost.exe"
                                    | "systemsettings.exe"
                            ) {
                                let set = &mut *(lparam as *mut BTreeSet<String>);
                                set.insert(lower);
                            }
                        }
                    }
                    CloseHandle(handle);
                }
            }
            1
        }

        let mut set = BTreeSet::<String>::new();
        unsafe {
            EnumWindows(Some(enum_proc), &mut set as *mut BTreeSet<String> as LPARAM);
        }
        set.into_iter().collect()
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Downscaled 16×6 screen RGB grid for ultra-fast Ambilight / Screen Sync across keyboard & ASUS Aura.
#[derive(Debug, Clone)]
pub struct AmbilightFrame {
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<Rgb>,
}

impl AmbilightFrame {
    pub fn sample_uv(&self, norm_x: f32, norm_y: f32) -> Rgb {
        if self.pixels.is_empty() || self.w == 0 || self.h == 0 {
            return Rgb::CYAN;
        }
        let x = ((norm_x.clamp(0.0, 0.999)) * self.w as f32) as usize;
        let y = ((norm_y.clamp(0.0, 0.999)) * self.h as f32) as usize;
        self.pixels[y * self.w + x]
    }
}

/// Capture the primary screen downscaled to 16×6 pixels via Win32 GDI `StretchBlt` (~1 ms, <0.3% CPU).
pub fn sample_ambilight_screen(prev: Option<&AmbilightFrame>) -> Option<AmbilightFrame> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HALFTONE, ReleaseDC, SRCCOPY,
            SelectObject, SetStretchBltMode, StretchBlt,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
        };

        const W: usize = 16;
        const H: usize = 6;
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        if sw <= 0 || sh <= 0 {
            return None;
        }

        let hdc_screen = GetDC(std::ptr::null_mut());
        if hdc_screen.is_null() {
            return None;
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, W as i32, H as i32);
        let old = SelectObject(hdc_mem, hbm);
        SetStretchBltMode(hdc_mem, HALFTONE);
        let ok = StretchBlt(
            hdc_mem,
            0,
            0,
            W as i32,
            H as i32,
            hdc_screen,
            0,
            0,
            sw,
            sh,
            SRCCOPY,
        );

        let mut raw = vec![0u8; W * H * 4];
        if ok != 0 {
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = W as i32;
            bmi.bmiHeader.biHeight = -(H as i32); // top-down
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            GetDIBits(
                hdc_mem,
                hbm,
                0,
                H as u32,
                raw.as_mut_ptr() as *mut core::ffi::c_void,
                &mut bmi,
                DIB_RGB_COLORS,
            );
        }

        SelectObject(hdc_mem, old);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);

        if ok == 0 {
            return None;
        }

        let mut pixels = Vec::with_capacity(W * H);
        for idx in 0..(W * H) {
            let b = raw[idx * 4];
            let g = raw[idx * 4 + 1];
            let r = raw[idx * 4 + 2];
            let c = boost_saturation(Rgb::new(r, g, b));
            let smoothed = if let Some(p) = prev {
                p.pixels.get(idx).copied().unwrap_or(c).lerp(c, 0.35)
            } else {
                c
            };
            pixels.push(smoothed);
        }
        Some(AmbilightFrame { w: W, h: H, pixels })
    }
    #[cfg(not(windows))]
    {
        let _ = prev;
        None
    }
}

fn boost_saturation(c: Rgb) -> Rgb {
    let r = c.r as f32 / 255.0;
    let g = c.g as f32 / 255.0;
    let b = c.b as f32 / 255.0;
    let avg = (r + g + b) / 3.0;
    let boost = 1.45;
    Rgb::new(
        ((avg + (r - avg) * boost).clamp(0.0, 1.0) * 255.0) as u8,
        ((avg + (g - avg) * boost).clamp(0.0, 1.0) * 255.0) as u8,
        ((avg + (b - avg) * boost).clamp(0.0, 1.0) * 255.0) as u8,
    )
}
