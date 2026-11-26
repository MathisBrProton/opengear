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
                        if let Some(name) = std::path::Path::new(&full)
                            .file_name()
                            .and_then(|s| s.to_str())
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

/// Downsamples a raw top-down BGRA framebuffer into a `dst_w × dst_h` saturated Ambilight grid.
pub fn downsample_bgra_grid(
    bgra: &[u8],
    row_pitch: usize,
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    prev: Option<&AmbilightFrame>,
) -> AmbilightFrame {
    let mut pixels = Vec::with_capacity(dst_w * dst_h);
    if src_w == 0 || src_h == 0 || row_pitch < src_w * 4 {
        return AmbilightFrame {
            w: dst_w,
            h: dst_h,
            pixels: vec![Rgb::BLACK; dst_w * dst_h],
        };
    }
    for gy in 0..dst_h {
        let y0 = (gy * src_h) / dst_h;
        let y1 = (((gy + 1) * src_h) / dst_h).max(y0 + 1).min(src_h);
        // Sample up to 4 rows x 8 cols per grid cell for fast, noise-free averaging (~0.02 ms).
        let y_step = ((y1 - y0) / 4).max(1);
        for gx in 0..dst_w {
            let x0 = (gx * src_w) / dst_w;
            let x1 = (((gx + 1) * src_w) / dst_w).max(x0 + 1).min(src_w);
            let x_step = ((x1 - x0) / 8).max(1);

            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut count = 0u32;
            let mut y = y0;
            while y < y1 {
                let row_off = y * row_pitch;
                let mut x = x0;
                while x < x1 {
                    let px = row_off + x * 4;
                    if px + 2 < bgra.len() {
                        b_sum += bgra[px] as u32;
                        g_sum += bgra[px + 1] as u32;
                        r_sum += bgra[px + 2] as u32;
                        count += 1;
                    }
                    x += x_step;
                }
                y += y_step;
            }
            let c = if count > 0 {
                boost_saturation(Rgb::new(
                    (r_sum / count) as u8,
                    (g_sum / count) as u8,
                    (b_sum / count) as u8,
                ))
            } else {
                Rgb::BLACK
            };
            let idx = gy * dst_w + gx;
            let smoothed = if let Some(p) = prev {
                p.pixels.get(idx).copied().unwrap_or(c).lerp(c, 0.35)
            } else {
                c
            };
            pixels.push(smoothed);
        }
    }
    AmbilightFrame {
        w: dst_w,
        h: dst_h,
        pixels,
    }
}

#[cfg(windows)]
mod dxgi_ambilight {
    use super::{AmbilightFrame, downsample_bgra_grid};
    use std::cell::RefCell;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice,
        ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{
        DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, IDXGIDevice, IDXGIOutput1,
        IDXGIOutputDuplication, IDXGIResource,
    };
    use windows::core::Interface;

    struct DxgiSession {
        _device: ID3D11Device,
        ctx: ID3D11DeviceContext,
        dupl: IDXGIOutputDuplication,
        staging: ID3D11Texture2D,
        width: u32,
        height: u32,
    }

    struct DxgiCapture {
        session: Option<DxgiSession>,
        last_init_fail: Option<Instant>,
    }

    struct AcquiredFrame<'a>(&'a IDXGIOutputDuplication);
    impl Drop for AcquiredFrame<'_> {
        fn drop(&mut self) {
            let _ = unsafe { self.0.ReleaseFrame() };
        }
    }

    thread_local! {
        static DXGI_CAPTURE: RefCell<DxgiCapture> = const {
            RefCell::new(DxgiCapture {
                session: None,
                last_init_fail: None,
            })
        };
    }

    impl DxgiSession {
        fn init() -> windows::core::Result<Self> {
            let mut dev_opt: Option<ID3D11Device> = None;
            let mut ctx_opt: Option<ID3D11DeviceContext> = None;
            unsafe {
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_HARDWARE,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_FLAG(0),
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut dev_opt),
                    None,
                    Some(&mut ctx_opt),
                )?;
            }
            let device = dev_opt.ok_or_else(windows::core::Error::from_win32)?;
            let ctx = ctx_opt.ok_or_else(windows::core::Error::from_win32)?;
            let dxgi_dev: IDXGIDevice = device.cast()?;
            let adapter = unsafe { dxgi_dev.GetAdapter()? };
            let output = unsafe { adapter.EnumOutputs(0)? };
            let output1: IDXGIOutput1 = output.cast()?;
            let dupl = unsafe { output1.DuplicateOutput(&device)? };

            let desc = unsafe { dupl.GetDesc() };
            if desc.ModeDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(-1i32),
                    "Unsupported DXGI desktop pixel format (HDR fallback to GDI)",
                ));
            }
            let width = desc.ModeDesc.Width.max(1);
            let height = desc.ModeDesc.Height.max(1);

            let tex_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging_opt: Option<ID3D11Texture2D> = None;
            unsafe { device.CreateTexture2D(&tex_desc, None, Some(&mut staging_opt))? };
            let staging = staging_opt.ok_or_else(windows::core::Error::from_win32)?;

            Ok(Self {
                _device: device,
                ctx,
                dupl,
                staging,
                width,
                height,
            })
        }
    }

    pub fn try_sample(prev: Option<&AmbilightFrame>) -> Option<AmbilightFrame> {
        DXGI_CAPTURE.with(|cell| {
            let mut st = cell.borrow_mut();
            if st.session.is_none() {
                if st
                    .last_init_fail
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
                {
                    return None;
                }
                match DxgiSession::init() {
                    Ok(s) => st.session = Some(s),
                    Err(_) => {
                        st.last_init_fail = Some(Instant::now());
                        return None;
                    }
                }
            }

            let session = st.session.as_ref()?;
            let timeout_ms = if prev.is_some() { 0 } else { 16 };
            let mut info = Default::default();
            let mut res: Option<IDXGIResource> = None;
            let hr = unsafe {
                session
                    .dupl
                    .AcquireNextFrame(timeout_ms, &mut info, &mut res)
            };
            match hr {
                Ok(()) => {
                    let _frame_lock = AcquiredFrame(&session.dupl);
                    let tex: ID3D11Texture2D = match res.and_then(|r| r.cast().ok()) {
                        Some(t) => t,
                        None => return prev.cloned(),
                    };
                    let dst_res: ID3D11Resource = session.staging.cast().ok()?;
                    let src_res: ID3D11Resource = tex.cast().ok()?;
                    unsafe { session.ctx.CopyResource(&dst_res, &src_res) };

                    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                    unsafe {
                        session
                            .ctx
                            .Map(&dst_res, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                            .ok()?;
                    }
                    let row_pitch = mapped.RowPitch as usize;
                    let total_bytes = row_pitch * (session.height as usize);
                    let frame = if mapped.pData.is_null() || total_bytes == 0 {
                        prev.cloned()
                    } else {
                        let slice = unsafe {
                            std::slice::from_raw_parts(mapped.pData as *const u8, total_bytes)
                        };
                        Some(downsample_bgra_grid(
                            slice,
                            row_pitch,
                            session.width as usize,
                            session.height as usize,
                            16,
                            6,
                            prev,
                        ))
                    };
                    unsafe { session.ctx.Unmap(&dst_res, 0) };
                    frame
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => prev.cloned(),
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                    st.session = None;
                    st.last_init_fail = Some(Instant::now());
                    None
                }
                Err(_) => {
                    st.session = None;
                    st.last_init_fail = Some(Instant::now());
                    None
                }
            }
        })
    }
}

/// Capture the primary screen downscaled to 16×6 pixels.
/// Tries DXGI Desktop Duplication first (hardware-accelerated, catches fullscreen/borderless games)
/// and falls back cleanly to Win32 GDI `StretchBlt`.
pub fn sample_ambilight_screen(prev: Option<&AmbilightFrame>) -> Option<AmbilightFrame> {
    #[cfg(windows)]
    {
        if let Some(frame) = dxgi_ambilight::try_sample(prev) {
            return Some(frame);
        }
        sample_ambilight_gdi(prev)
    }
    #[cfg(not(windows))]
    {
        let _ = prev;
        None
    }
}

#[cfg(windows)]
fn sample_ambilight_gdi(prev: Option<&AmbilightFrame>) -> Option<AmbilightFrame> {
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
            hdc_mem, 0, 0, W as i32, H as i32, hdc_screen, 0, 0, sw, sh, SRCCOPY,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsamples_bgra_quadrants_accurately() {
        // 4x2 BGRA image: left half pure red, right half pure blue
        let w = 4usize;
        let h = 2usize;
        let mut buf = Vec::with_capacity(w * h * 4);
        for _y in 0..h {
            for x in 0..w {
                if x < 2 {
                    buf.extend_from_slice(&[0, 0, 255, 255]); // B, G, R, A
                } else {
                    buf.extend_from_slice(&[255, 0, 0, 255]);
                }
            }
        }
        let frame = downsample_bgra_grid(&buf, w * 4, w, h, 2, 1, None);
        assert_eq!(frame.w, 2);
        assert_eq!(frame.h, 1);
        assert!(frame.pixels[0].r > 200 && frame.pixels[0].b < 20);
        assert!(frame.pixels[1].b > 200 && frame.pixels[1].r < 20);
    }
}
