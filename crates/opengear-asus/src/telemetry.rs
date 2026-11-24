use std::ffi::{c_int, c_void};
use std::process::Command;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct StaticSystemInfo {
    pub motherboard: String,
    pub bios_version: String,
    pub cpu_name: String,
    pub gpu_name: String,
    pub ram_total_gb: f32,
}

impl Default for StaticSystemInfo {
    fn default() -> Self {
        Self {
            motherboard: "ASUS Motherboard".into(),
            bios_version: "—".into(),
            cpu_name: "CPU".into(),
            gpu_name: "GPU".into(),
            ram_total_gb: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GpuTelemetry {
    pub available: bool,
    pub edge_temp_c: Option<f32>,
    pub hotspot_temp_c: Option<f32>,
    pub fan_rpm: Option<u32>,
    pub activity_pct: Option<u32>,
    pub core_clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
    pub power_watts: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct SystemTelemetry {
    pub cpu_usage_pct: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub ram_usage_pct: f32,
    pub gpu: GpuTelemetry,
}

// --- Minimal AMD ADL2 bindings (dynamically loaded from C:\Windows\System32\atiadlxx.dll) ---
type AdlContextHandle = *mut c_void;
type AdlMainMallocCallback = unsafe extern "C" fn(c_int) -> *mut c_void;
type FnAdl2MainControlCreate =
    unsafe extern "C" fn(AdlMainMallocCallback, c_int, *mut AdlContextHandle) -> c_int;
type FnAdl2MainControlDestroy = unsafe extern "C" fn(AdlContextHandle) -> c_int;
type FnAdl2AdapterNumberOfAdaptersGet =
    unsafe extern "C" fn(AdlContextHandle, *mut c_int) -> c_int;
type FnAdl2NewQueryPmLogDataGet =
    unsafe extern "C" fn(AdlContextHandle, c_int, *mut AdlPmLogDataOutput) -> c_int;

// ADL_PMLOG_MAX_SENSORS_REAL = 256
const ADL_PMLOG_MAX_SENSORS: usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
struct AdlSingleSensorData {
    supported: c_int,
    value: c_int,
}

#[repr(C)]
struct AdlPmLogDataOutput {
    size: c_int,
    sensors: [AdlSingleSensorData; ADL_PMLOG_MAX_SENSORS],
}

// Sensor IDs from official AMD ADL SDK `adl_defines.h` (ADL_PMLOG_SENSORS)
const PMLOG_CLK_GFXCLK: usize = 1;
const PMLOG_CLK_MEMCLK: usize = 2;
const PMLOG_TEMPERATURE_EDGE: usize = 7;
const PMLOG_TEMPERATURE_HOTSPOT: usize = 9;
const PMLOG_FAN_RPM: usize = 13;
const PMLOG_INFO_ACTIVITY_GFX: usize = 19;
const PMLOG_GFX_POWER: usize = 26;
const PMLOG_BOARD_POWER: usize = 51;

unsafe extern "C" fn adl_malloc(size: c_int) -> *mut c_void {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
    unsafe { std::alloc::alloc_zeroed(layout) as *mut c_void }
}

struct AmdAdlReader {
    _lib: libloading::Library,
    ctx: AdlContextHandle,
    adapter_idx: c_int,
    fn_destroy: FnAdl2MainControlDestroy,
    fn_pmlog: FnAdl2NewQueryPmLogDataGet,
}

// ADL2 context is only accessed from TelemetryCollector on one thread at a time.
unsafe impl Send for AmdAdlReader {}

impl AmdAdlReader {
    fn try_init() -> Option<Self> {
        unsafe {
            let lib = libloading::Library::new("atiadlxx.dll").ok()?;
            let fn_create: FnAdl2MainControlCreate = *lib.get(b"ADL2_Main_Control_Create\0").ok()?;
            let fn_destroy: FnAdl2MainControlDestroy = *lib.get(b"ADL2_Main_Control_Destroy\0").ok()?;
            let fn_num: FnAdl2AdapterNumberOfAdaptersGet =
                *lib.get(b"ADL2_Adapter_NumberOfAdapters_Get\0").ok()?;
            let fn_pmlog: FnAdl2NewQueryPmLogDataGet =
                *lib.get(b"ADL2_New_QueryPMLogData_Get\0").ok()?;

            let mut ctx: AdlContextHandle = std::ptr::null_mut();
            if fn_create(adl_malloc, 1, &mut ctx) != 0 || ctx.is_null() {
                return None;
            }
            let mut count: c_int = 0;
            if fn_num(ctx, &mut count) != 0 || count <= 0 {
                fn_destroy(ctx);
                return None;
            }

            // Find the first adapter that successfully returns PMLog data
            let mut valid_adapter = None;
            for idx in 0..count.min(16) {
                let mut out: AdlPmLogDataOutput = std::mem::zeroed();
                out.size = std::mem::size_of::<AdlPmLogDataOutput>() as c_int;
                if fn_pmlog(ctx, idx, &mut out) == 0 {
                    if out.sensors[PMLOG_TEMPERATURE_EDGE].supported != 0
                        || out.sensors[PMLOG_INFO_ACTIVITY_GFX].supported != 0
                    {
                        valid_adapter = Some(idx);
                        break;
                    }
                }
            }

            let Some(adapter_idx) = valid_adapter else {
                fn_destroy(ctx);
                return None;
            };

            debug!("AMD ADL2 initialisé sur adaptateur #{adapter_idx}");
            Some(Self {
                _lib: lib,
                ctx,
                adapter_idx,
                fn_destroy,
                fn_pmlog,
            })
        }
    }

    fn read(&self) -> GpuTelemetry {
        let mut tel = GpuTelemetry::default();
        unsafe {
            let mut out: AdlPmLogDataOutput = std::mem::zeroed();
            out.size = std::mem::size_of::<AdlPmLogDataOutput>() as c_int;
            if (self.fn_pmlog)(self.ctx, self.adapter_idx, &mut out) != 0 {
                return tel;
            }
            tel.available = true;

            let get = |id: usize| -> Option<i32> {
                let s = out.sensors.get(id)?;
                if s.supported != 0 {
                    Some(s.value)
                } else {
                    None
                }
            };

            tel.edge_temp_c = get(PMLOG_TEMPERATURE_EDGE).map(|v| v as f32);
            tel.hotspot_temp_c = get(PMLOG_TEMPERATURE_HOTSPOT).map(|v| v as f32);
            tel.fan_rpm = get(PMLOG_FAN_RPM).map(|v| v.max(0) as u32);
            tel.activity_pct = get(PMLOG_INFO_ACTIVITY_GFX).map(|v| v.clamp(0, 100) as u32);
            tel.core_clock_mhz = get(PMLOG_CLK_GFXCLK).map(|v| v.max(0) as u32);
            tel.mem_clock_mhz = get(PMLOG_CLK_MEMCLK).map(|v| v.max(0) as u32);
            tel.power_watts = get(PMLOG_BOARD_POWER)
                .or_else(|| get(PMLOG_GFX_POWER))
                .map(|v| v as f32);
        }
        tel
    }
}

impl Drop for AmdAdlReader {
    fn drop(&mut self) {
        unsafe {
            (self.fn_destroy)(self.ctx);
        }
    }
}

pub struct TelemetryCollector {
    pub static_info: StaticSystemInfo,
    prev_idle: u64,
    prev_kernel: u64,
    prev_user: u64,
    adl: Option<AmdAdlReader>,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        let static_info = Self::query_static_info();
        let (idle, kernel, user) = Self::read_cpu_times();
        let adl = AmdAdlReader::try_init();
        if adl.is_none() {
            warn!("AMD ADL2 non disponible (GPU non-AMD ou pilote indisponible)");
        }
        Self {
            static_info,
            prev_idle: idle,
            prev_kernel: kernel,
            prev_user: user,
            adl,
        }
    }

    pub fn sample(&mut self) -> SystemTelemetry {
        let (idle, kernel, user) = Self::read_cpu_times();
        let d_idle = idle.saturating_sub(self.prev_idle);
        let d_kernel = kernel.saturating_sub(self.prev_kernel);
        let d_user = user.saturating_sub(self.prev_user);
        self.prev_idle = idle;
        self.prev_kernel = kernel;
        self.prev_user = user;

        let total = d_kernel + d_user;
        let cpu_usage_pct = if total > 0 {
            (((total.saturating_sub(d_idle)) as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32
        } else {
            0.0
        };

        let (ram_used_gb, ram_total_gb, ram_usage_pct) = Self::read_memory();
        let gpu = self
            .adl
            .as_ref()
            .map(|a| a.read())
            .unwrap_or_default();

        SystemTelemetry {
            cpu_usage_pct,
            ram_used_gb,
            ram_total_gb,
            ram_usage_pct,
            gpu,
        }
    }

    #[cfg(windows)]
    fn read_cpu_times() -> (u64, u64, u64) {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::GetSystemTimes;
        unsafe {
            let mut idle: FILETIME = std::mem::zeroed();
            let mut kernel: FILETIME = std::mem::zeroed();
            let mut user: FILETIME = std::mem::zeroed();
            if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
                let to_u64 = |ft: FILETIME| ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
                return (to_u64(idle), to_u64(kernel), to_u64(user));
            }
        }
        (0, 0, 0)
    }

    #[cfg(not(windows))]
    fn read_cpu_times() -> (u64, u64, u64) {
        (0, 0, 0)
    }

    #[cfg(windows)]
    fn read_memory() -> (f32, f32, f32) {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        unsafe {
            let mut st: MEMORYSTATUSEX = std::mem::zeroed();
            st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut st) != 0 {
                let gb = 1024.0 * 1024.0 * 1024.0;
                let total = st.ullTotalPhys as f64 / gb;
                let avail = st.ullAvailPhys as f64 / gb;
                let used = (total - avail).max(0.0);
                let pct = st.dwMemoryLoad as f32;
                return (used as f32, total as f32, pct);
            }
        }
        (0.0, 0.0, 0.0)
    }

    #[cfg(not(windows))]
    fn read_memory() -> (f32, f32, f32) {
        (0.0, 0.0, 0.0)
    }

    fn query_static_info() -> StaticSystemInfo {
        let mut info = StaticSystemInfo::default();
        let (_, total_gb, _) = Self::read_memory();
        info.ram_total_gb = total_gb;

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let script = "$b=(Get-CimInstance Win32_BaseBoard).Product; $bios=(Get-CimInstance Win32_BIOS).SMBIOSBIOSVersion; $c=(Get-CimInstance Win32_Processor | Select -First 1).Name; $g=(Get-CimInstance Win32_VideoController | Select -First 1).Name; \"$b|$bios|$c|$g\"";
            if let Ok(out) = Command::new("powershell")
                .args(["-NoProfile", "-Command", script])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            {
                let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 4 {
                    if !parts[0].trim().is_empty() {
                        info.motherboard = parts[0].trim().to_string();
                    }
                    if !parts[1].trim().is_empty() {
                        info.bios_version = parts[1].trim().to_string();
                    }
                    if !parts[2].trim().is_empty() {
                        info.cpu_name = parts[2].trim().to_string();
                    }
                    if !parts[3].trim().is_empty() {
                        info.gpu_name = parts[3].trim().to_string();
                    }
                }
            }
        }
        info
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}
