use std::ffi::{c_int, c_uint, c_void};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct StaticSystemInfo {
    pub manufacturer: String,
    pub model: String,
    pub motherboard: String,
    pub bios_version: String,
    pub cpu_name: String,
    pub gpu_name: String,
    pub ram_total_gb: f32,
}

impl Default for StaticSystemInfo {
    fn default() -> Self {
        Self {
            manufacturer: String::new(),
            model: String::new(),
            motherboard: "Carte mère".into(),
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
    pub backend: String,
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

// --- Minimal AMD ADL2 bindings (dynamically loaded from System32\atiadlxx.dll) ---
type AdlContextHandle = *mut c_void;
type AdlMainMallocCallback = unsafe extern "system" fn(c_int) -> *mut c_void;
type FnAdl2MainControlCreate =
    unsafe extern "system" fn(AdlMainMallocCallback, c_int, *mut AdlContextHandle) -> c_int;
type FnAdl2MainControlDestroy = unsafe extern "system" fn(AdlContextHandle) -> c_int;
type FnAdl2AdapterNumberOfAdaptersGet =
    unsafe extern "system" fn(AdlContextHandle, *mut c_int) -> c_int;
type FnAdl2NewQueryPmLogDataGet =
    unsafe extern "system" fn(AdlContextHandle, c_int, *mut AdlPmLogDataOutput) -> c_int;

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

// ADL_PMLOG_SENSORS (verified against AMD ADL SDK `adl_defines.h`):
// 7 was `ADL_PMLOG_CLK_VCNCLK` (400 MHz on Ryzen iGPU!), NOT Edge temp (8).
const PMLOG_CLK_GFXCLK: usize = 1;
const PMLOG_CLK_MEMCLK: usize = 2;
const PMLOG_TEMPERATURE_EDGE: usize = 8;
const PMLOG_FAN_RPM: usize = 14;
const PMLOG_INFO_ACTIVITY_GFX: usize = 19;
const PMLOG_TEMPERATURE_HOTSPOT: usize = 27;
const PMLOG_ASIC_POWER: usize = 28;
const PMLOG_BOARD_POWER: usize = 73;

pub(crate) fn sanitize_gpu_temp(v: f32) -> Option<f32> {
    if v.is_finite() && (0.0..=135.0).contains(&v) {
        Some(v)
    } else {
        None
    }
}

pub(crate) fn prefer_nvml_for_gpu(gpu_name: &str) -> bool {
    let l = gpu_name.to_ascii_lowercase();
    l.contains("nvidia")
        || l.contains("geforce")
        || l.contains("rtx")
        || l.contains("gtx")
        || l.contains("quadro")
}

/// Allocate zeroed memory using the C runtime allocator expected by AMD ADL2 (`free`-compatible).
unsafe extern "system" fn adl_malloc(size: c_int) -> *mut c_void {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    unsafe { libc::calloc(1, size as usize) }
}

struct AmdAdlReader {
    ctx: AdlContextHandle,
    adapter_idx: c_int,
    fn_destroy: FnAdl2MainControlDestroy,
    fn_pmlog: FnAdl2NewQueryPmLogDataGet,
    // Kept last so `Drop` executes `fn_destroy` before unloading the DLL.
    _lib: libloading::Library,
}

unsafe impl Send for AmdAdlReader {}

impl AmdAdlReader {
    #[cfg(windows)]
    fn try_init() -> Option<Self> {
        unsafe {
            let lib = crate::wmi_com::load_system32_library("atiadlxx.dll").ok()?;
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

            let mut valid_adapter = None;
            for idx in 0..count.min(16) {
                let mut out: AdlPmLogDataOutput = std::mem::zeroed();
                out.size = std::mem::size_of::<AdlPmLogDataOutput>() as c_int;
                if fn_pmlog(ctx, idx, &mut out) == 0
                    && (out.sensors[PMLOG_TEMPERATURE_EDGE].supported != 0
                        || out.sensors[PMLOG_INFO_ACTIVITY_GFX].supported != 0)
                {
                    valid_adapter = Some(idx);
                    break;
                }
            }

            let Some(adapter_idx) = valid_adapter else {
                fn_destroy(ctx);
                return None;
            };

            debug!("AMD ADL2 initialisé sur adaptateur #{adapter_idx}");
            Some(Self {
                ctx,
                adapter_idx,
                fn_destroy,
                fn_pmlog,
                _lib: lib,
            })
        }
    }

    #[cfg(not(windows))]
    fn try_init() -> Option<Self> {
        None
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
            tel.backend = "AMD ADL2".to_string();

            let get = |id: usize| -> Option<i32> {
                let s = out.sensors.get(id)?;
                if s.supported != 0 {
                    Some(s.value)
                } else {
                    None
                }
            };

            tel.edge_temp_c = get(PMLOG_TEMPERATURE_EDGE).and_then(|v| sanitize_gpu_temp(v as f32));
            tel.hotspot_temp_c =
                get(PMLOG_TEMPERATURE_HOTSPOT).and_then(|v| sanitize_gpu_temp(v as f32));
            tel.fan_rpm = get(PMLOG_FAN_RPM).map(|v| v.max(0) as u32);
            tel.activity_pct = get(PMLOG_INFO_ACTIVITY_GFX).map(|v| v.clamp(0, 100) as u32);
            tel.core_clock_mhz = get(PMLOG_CLK_GFXCLK).map(|v| v.max(0) as u32);
            tel.mem_clock_mhz = get(PMLOG_CLK_MEMCLK).map(|v| v.max(0) as u32);
            tel.power_watts = get(PMLOG_BOARD_POWER)
                .or_else(|| get(PMLOG_ASIC_POWER))
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

// --- Minimal NVIDIA NVML bindings (dynamically loaded from System32\nvml.dll) ---
type NvmlDevice = *mut c_void;
type FnNvmlInit = unsafe extern "C" fn() -> c_int;
type FnNvmlShutdown = unsafe extern "C" fn() -> c_int;
type FnNvmlDeviceGetHandleByIndex = unsafe extern "C" fn(c_uint, *mut NvmlDevice) -> c_int;
type FnNvmlDeviceGetTemperature = unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> c_int;
type FnNvmlDeviceGetUtilizationRates =
    unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> c_int;
type FnNvmlDeviceGetClockInfo = unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> c_int;
type FnNvmlDeviceGetPowerUsage = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_int;

#[repr(C)]
struct NvmlUtilization {
    gpu: c_uint,
    memory: c_uint,
}

struct NvmlReader {
    device: NvmlDevice,
    fn_shutdown: FnNvmlShutdown,
    fn_temp: FnNvmlDeviceGetTemperature,
    fn_util: Option<FnNvmlDeviceGetUtilizationRates>,
    fn_clock: Option<FnNvmlDeviceGetClockInfo>,
    fn_power: Option<FnNvmlDeviceGetPowerUsage>,
    _lib: libloading::Library,
}

unsafe impl Send for NvmlReader {}

impl NvmlReader {
    #[cfg(windows)]
    fn try_init() -> Option<Self> {
        unsafe {
            let lib = crate::wmi_com::load_system32_library("nvml.dll").ok()?;
            let fn_init: FnNvmlInit = *lib.get(b"nvmlInit_v2\0").ok()?;
            let fn_shutdown: FnNvmlShutdown = *lib.get(b"nvmlShutdown\0").ok()?;
            let fn_handle: FnNvmlDeviceGetHandleByIndex =
                *lib.get(b"nvmlDeviceGetHandleByIndex_v2\0").ok()?;
            let fn_temp: FnNvmlDeviceGetTemperature =
                *lib.get(b"nvmlDeviceGetTemperature\0").ok()?;
            let fn_util = lib
                .get::<FnNvmlDeviceGetUtilizationRates>(b"nvmlDeviceGetUtilizationRates\0")
                .ok()
                .map(|s| *s);
            let fn_clock = lib
                .get::<FnNvmlDeviceGetClockInfo>(b"nvmlDeviceGetClockInfo\0")
                .ok()
                .map(|s| *s);
            let fn_power = lib
                .get::<FnNvmlDeviceGetPowerUsage>(b"nvmlDeviceGetPowerUsage\0")
                .ok()
                .map(|s| *s);

            if fn_init() != 0 {
                return None;
            }
            let mut device: NvmlDevice = std::ptr::null_mut();
            if fn_handle(0, &mut device) != 0 || device.is_null() {
                fn_shutdown();
                return None;
            }
            debug!("NVIDIA NVML initialisé sur GPU #0");
            Some(Self {
                device,
                fn_shutdown,
                fn_temp,
                fn_util,
                fn_clock,
                fn_power,
                _lib: lib,
            })
        }
    }

    #[cfg(not(windows))]
    fn try_init() -> Option<Self> {
        None
    }

    fn read(&self) -> GpuTelemetry {
        let mut tel = GpuTelemetry::default();
        unsafe {
            let mut temp: c_uint = 0;
            if (self.fn_temp)(self.device, 0, &mut temp) == 0 {
                if let Some(t) = sanitize_gpu_temp(temp as f32) {
                    tel.available = true;
                    tel.backend = "NVIDIA NVML".to_string();
                    tel.edge_temp_c = Some(t);
                }
            }
            if let Some(f) = self.fn_util {
                let mut u = NvmlUtilization { gpu: 0, memory: 0 };
                if f(self.device, &mut u) == 0 {
                    tel.available = true;
                    tel.backend = "NVIDIA NVML".to_string();
                    tel.activity_pct = Some(u.gpu.min(100));
                }
            }
            if let Some(f) = self.fn_clock {
                let mut gfx: c_uint = 0;
                if f(self.device, 0, &mut gfx) == 0 {
                    tel.core_clock_mhz = Some(gfx);
                }
                let mut mem: c_uint = 0;
                if f(self.device, 2, &mut mem) == 0 {
                    tel.mem_clock_mhz = Some(mem);
                }
            }
            if let Some(f) = self.fn_power {
                let mut mw: c_uint = 0;
                if f(self.device, &mut mw) == 0 {
                    tel.power_watts = Some((mw as f32) / 1000.0);
                }
            }
        }
        tel
    }
}

impl Drop for NvmlReader {
    fn drop(&mut self) {
        unsafe {
            (self.fn_shutdown)();
        }
    }
}

enum GpuReader {
    Amd(AmdAdlReader),
    Nvidia(NvmlReader),
}

impl GpuReader {
    fn read(&self) -> GpuTelemetry {
        match self {
            Self::Amd(r) => r.read(),
            Self::Nvidia(r) => r.read(),
        }
    }
}

pub struct TelemetryCollector {
    pub static_info: StaticSystemInfo,
    prev_idle: u64,
    prev_kernel: u64,
    prev_user: u64,
    gpu_reader: Option<GpuReader>,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        let static_info = Self::query_static_info();
        let (idle, kernel, user) = Self::read_cpu_times();
        let gpu_reader = if prefer_nvml_for_gpu(&static_info.gpu_name) {
            NvmlReader::try_init()
                .map(GpuReader::Nvidia)
                .or_else(|| AmdAdlReader::try_init().map(GpuReader::Amd))
        } else {
            AmdAdlReader::try_init()
                .map(GpuReader::Amd)
                .or_else(|| NvmlReader::try_init().map(GpuReader::Nvidia))
        };
        if gpu_reader.is_none() {
            warn!("Télémétrie GPU directe indisponible (ni AMD ADL2 ni NVIDIA NVML)");
        }
        Self {
            static_info,
            prev_idle: idle,
            prev_kernel: kernel,
            prev_user: user,
            gpu_reader,
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
            (((total.saturating_sub(d_idle)) as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
                as f32
        } else {
            0.0
        };

        let (ram_used_gb, ram_total_gb, ram_usage_pct) = Self::read_memory();
        let gpu = self
            .gpu_reader
            .as_ref()
            .map(|r| r.read())
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
                let to_u64 =
                    |ft: FILETIME| ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
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
        if let Ok(conn) = crate::wmi_com::WmiConn::connect(r"root\cimv2") {
            if let Ok(rows) = conn.query("SELECT Manufacturer, Model FROM Win32_ComputerSystem") {
                if let Some(row) = rows.first() {
                    if let Some(s) = row
                        .get("Manufacturer")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        info.manufacturer = s.to_string();
                    }
                    if let Some(s) = row
                        .get("Model")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        info.model = s.to_string();
                    }
                }
            }
            if let Ok(rows) = conn.query("SELECT Product FROM Win32_BaseBoard") {
                if let Some(s) = rows
                    .first()
                    .and_then(|r| r.get("Product"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    info.motherboard = s.to_string();
                }
            }
            if let Ok(rows) = conn.query("SELECT SMBIOSBIOSVersion FROM Win32_BIOS") {
                if let Some(s) = rows
                    .first()
                    .and_then(|r| r.get("SMBIOSBIOSVersion"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    info.bios_version = s.to_string();
                }
            }
            if let Ok(rows) = conn.query("SELECT Name FROM Win32_Processor") {
                if let Some(s) = rows
                    .first()
                    .and_then(|r| r.get("Name"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    info.cpu_name = s.to_string();
                }
            }
            if let Ok(rows) = conn.query("SELECT Name FROM Win32_VideoController") {
                let names: Vec<String> = rows
                    .iter()
                    .filter_map(|r| r.get("Name").and_then(|v| v.as_str()))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if let Some(picked) = pick_primary_gpu_name(&names) {
                    info.gpu_name = picked;
                }
            }
        }
        info
    }
}

pub(crate) fn pick_primary_gpu_name(names: &[String]) -> Option<String> {
    let is_integrated_or_virtual = |n: &str| {
        let l = n.to_ascii_lowercase();
        l.contains("radeon(tm) graphics")
            || l.contains("intel(r) uhd")
            || l.contains("intel(r) iris")
            || l.contains("microsoft basic display")
            || l.contains("parsec")
    };
    names
        .iter()
        .find(|n| !is_integrated_or_virtual(n))
        .or_else(|| names.first())
        .cloned()
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adl_pmlog_sensor_indices_match_sdk() {
        assert_eq!(PMLOG_CLK_GFXCLK, 1);
        assert_eq!(PMLOG_CLK_MEMCLK, 2);
        assert_eq!(PMLOG_TEMPERATURE_EDGE, 8);
        assert_eq!(PMLOG_FAN_RPM, 14);
        assert_eq!(PMLOG_INFO_ACTIVITY_GFX, 19);
        assert_eq!(PMLOG_TEMPERATURE_HOTSPOT, 27);
        assert_eq!(PMLOG_ASIC_POWER, 28);
        assert_eq!(PMLOG_BOARD_POWER, 73);
    }

    #[test]
    fn gpu_selection_prefers_discrete_nvidia_on_hybrid_laptops() {
        let gpus = vec![
            "AMD Radeon(TM) Graphics".to_string(),
            "NVIDIA GeForce RTX 3070 Laptop GPU".to_string(),
        ];
        let picked = pick_primary_gpu_name(&gpus).unwrap();
        assert_eq!(picked, "NVIDIA GeForce RTX 3070 Laptop GPU");
        assert!(prefer_nvml_for_gpu(&picked));
        assert!(!prefer_nvml_for_gpu("AMD Radeon RX 7800 XT"));
    }

    #[test]
    fn temperature_sanity_rejects_clock_values_like_400() {
        assert_eq!(sanitize_gpu_temp(40.0), Some(40.0));
        assert_eq!(sanitize_gpu_temp(400.0), None);
        assert_eq!(sanitize_gpu_temp(-5.0), None);
        assert_eq!(sanitize_gpu_temp(f32::NAN), None);
    }
}
