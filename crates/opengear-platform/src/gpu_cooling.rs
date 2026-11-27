//! Universal GPU fan control via NVIDIA NVML (`nvml.dll`) and AMD ADL2 (`atiadlxx.dll`),
//! exposed through OpenGear's shared `Cooling` capability.
//!
//! Safety guarantees:
//! - Manual speeds are clamped to `GPU_MIN_FAN_PCT..=100` (`FanInfo::min_pct`).
//! - Restoring firmware/VBIOS Auto control returns zero-RPM and stock curves to the GPU driver.
//! - Read-only discovery probes capabilities without modifying GPU fan state.

use anyhow::{Result, bail};
use opengear_core::driver::{Connection, Device, DeviceInfo, DeviceKind, DeviceSpec, Driver};
use opengear_core::fans::{Cooling, FanInfo, FanTarget, clamp_target};

/// Minimum manual GPU fan duty percentage allowed when overriding VBIOS curves.
pub const GPU_MIN_FAN_PCT: u8 = 30;

/// Scale a requested `0..=100` percentage into an AMD OverdriveN fan parameter range (`min..=max`),
/// supporting both percentage-based (`max <= 100`) and RPM-based (`max > 100`) VBIOS tables.
pub(crate) fn odn_scale_pct_to_driver(pct: u8, min: i32, max: i32) -> i32 {
    if max <= min {
        return max;
    }
    let clamped = pct.clamp(GPU_MIN_FAN_PCT, 100) as i64;
    if max <= 100 {
        return (clamped as i32).clamp(min, max);
    }
    let span = (max as i64) - (min as i64);
    let scaled = min as i64 + (span * clamped + 50) / 100;
    (scaled as i32).clamp(min, max)
}

/// Convert an AMD OverdriveN fan reading back to `0..=100` percentage using driver limits.
pub(crate) fn odn_driver_to_pct(val: i32, min: i32, max: i32, current_mode_is_rpm: bool) -> u8 {
    if val <= 0 {
        return 0;
    }
    if !current_mode_is_rpm && val <= 100 && max <= 100 {
        return val.clamp(0, 100) as u8;
    }
    if max <= 0 {
        return val.clamp(0, 100) as u8;
    }
    if max <= 100 {
        return val.clamp(0, 100) as u8;
    }
    let base = min.max(0);
    if val <= base {
        return if base > 0 { 1 } else { 0 };
    }
    let span = (max - base).max(1) as i64;
    let pct = ((val - base) as i64 * 100 + span / 2) / span;
    pct.clamp(0, 100) as u8
}

/// Build a valid, monotonic 5-point AMD Overdrive8 manual fan curve pinned to `target_pct`.
///
/// AMD Overdrive8 rejects curve writes unless temperatures are non-decreasing within `[min_t..=max_t]`
/// and speeds are monotonic within `[min_s..=max_s]`.
pub(crate) fn build_od8_manual_curve(
    min_temp: i32,
    max_temp: i32,
    min_speed: i32,
    max_speed: i32,
    base_temps: [i32; 5],
    target_pct: u8,
) -> ([i32; 5], [i32; 5]) {
    let lo_t = min_temp.min(max_temp);
    let hi_t = max_temp.max(lo_t);
    let mut temps = [lo_t; 5];
    let fallback_step = ((hi_t - lo_t) / 4).max(1);
    for i in 0..5 {
        let candidate = if base_temps[i] > 0 {
            base_temps[i]
        } else {
            lo_t + (i as i32) * fallback_step
        };
        let clamped = candidate.clamp(lo_t, hi_t);
        temps[i] = if i == 0 {
            clamped
        } else {
            clamped.max(temps[i - 1])
        };
    }

    let lo_s = min_speed.min(max_speed);
    let hi_s = max_speed.max(lo_s);
    let speed = (target_pct.clamp(GPU_MIN_FAN_PCT, 100) as i32).clamp(lo_s, hi_s);
    (temps, [speed; 5])
}

trait GpuCoolingBackend: Send {
    fn id(&self) -> &str;
    fn vendor(&self) -> &str;
    fn model(&self) -> &str;
    fn firmware(&self) -> Option<String>;
    fn backend_label(&self) -> &str;
    fn fans(&self) -> Vec<FanInfo>;
    fn refresh(&mut self);
    fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()>;
    fn overridden(&self) -> bool;
    fn restore_firmware_control(&mut self) -> Result<()>;
}

#[cfg(windows)]
mod win_gpu {
    use super::*;
    use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
    use std::sync::Arc;
    use tracing::info;

    // -------------------------------------------------------------------------
    // NVIDIA NVML (`System32\nvml.dll`)
    // -------------------------------------------------------------------------
    type NvmlDevice = *mut c_void;
    type FnNvmlInitV2 = unsafe extern "C" fn() -> c_int;
    type FnNvmlShutdown = unsafe extern "C" fn() -> c_int;
    type FnNvmlDeviceGetCountV2 = unsafe extern "C" fn(*mut c_uint) -> c_int;
    type FnNvmlDeviceGetHandleByIndexV2 = unsafe extern "C" fn(c_uint, *mut NvmlDevice) -> c_int;
    type FnNvmlDeviceGetName = unsafe extern "C" fn(NvmlDevice, *mut c_char, c_uint) -> c_int;
    type FnNvmlDeviceGetNumFans = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_int;
    type FnNvmlDeviceGetFanSpeed = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_int;
    type FnNvmlDeviceGetFanSpeedV2 = unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> c_int;
    type FnNvmlDeviceSetFanSpeedV2 = unsafe extern "C" fn(NvmlDevice, c_uint, c_uint) -> c_int;
    type FnNvmlDeviceSetDefaultFanSpeedV2 = unsafe extern "C" fn(NvmlDevice, c_uint) -> c_int;

    const NVML_SUCCESS: c_int = 0;

    struct NvmlSession {
        fn_shutdown: FnNvmlShutdown,
        fn_get_num_fans: Option<FnNvmlDeviceGetNumFans>,
        fn_get_fan_speed: Option<FnNvmlDeviceGetFanSpeed>,
        fn_get_fan_speed_v2: Option<FnNvmlDeviceGetFanSpeedV2>,
        fn_set_fan_speed_v2: FnNvmlDeviceSetFanSpeedV2,
        fn_set_default_fan_v2: FnNvmlDeviceSetDefaultFanSpeedV2,
        // Drop DLL after calling `fn_shutdown`.
        _lib: libloading::Library,
    }

    unsafe impl Send for NvmlSession {}
    unsafe impl Sync for NvmlSession {}

    impl Drop for NvmlSession {
        fn drop(&mut self) {
            unsafe {
                (self.fn_shutdown)();
            }
        }
    }

    struct NvidiaGpuDevice {
        id: String,
        model: String,
        device: NvmlDevice,
        fan_count: usize,
        live_pcts: Vec<Option<u8>>,
        manual_pcts: Vec<Option<u8>>,
        session: Arc<NvmlSession>,
    }

    unsafe impl Send for NvidiaGpuDevice {}

    impl NvidiaGpuDevice {
        fn read_fan_speed_pct(&self, idx: usize) -> Option<u8> {
            unsafe {
                let mut speed: c_uint = 0;
                if let Some(f2) = self.session.fn_get_fan_speed_v2
                    && f2(self.device, idx as c_uint, &mut speed) == NVML_SUCCESS
                {
                    return Some(speed.min(100) as u8);
                }
                if idx == 0
                    && let Some(f1) = self.session.fn_get_fan_speed
                    && f1(self.device, &mut speed) == NVML_SUCCESS
                {
                    return Some(speed.min(100) as u8);
                }
            }
            None
        }

        fn discover() -> Vec<Box<dyn GpuCoolingBackend>> {
            unsafe {
                let Ok(lib) = crate::wmi::load_system32_library("nvml.dll") else {
                    return Vec::new();
                };
                let Ok(fn_init) = lib.get::<FnNvmlInitV2>(b"nvmlInit_v2\0").map(|s| *s) else {
                    return Vec::new();
                };
                let Ok(fn_shutdown) = lib.get::<FnNvmlShutdown>(b"nvmlShutdown\0").map(|s| *s)
                else {
                    return Vec::new();
                };
                let Ok(fn_count) = lib
                    .get::<FnNvmlDeviceGetCountV2>(b"nvmlDeviceGetCount_v2\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };
                let Ok(fn_handle) = lib
                    .get::<FnNvmlDeviceGetHandleByIndexV2>(b"nvmlDeviceGetHandleByIndex_v2\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };
                let Ok(fn_set_fan) = lib
                    .get::<FnNvmlDeviceSetFanSpeedV2>(b"nvmlDeviceSetFanSpeed_v2\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };
                let Ok(fn_reset_fan) = lib
                    .get::<FnNvmlDeviceSetDefaultFanSpeedV2>(b"nvmlDeviceSetDefaultFanSpeed_v2\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };

                let fn_name = lib
                    .get::<FnNvmlDeviceGetName>(b"nvmlDeviceGetName\0")
                    .ok()
                    .map(|s| *s);
                let fn_get_num_fans = lib
                    .get::<FnNvmlDeviceGetNumFans>(b"nvmlDeviceGetNumFans\0")
                    .ok()
                    .map(|s| *s);
                let fn_get_fan_speed = lib
                    .get::<FnNvmlDeviceGetFanSpeed>(b"nvmlDeviceGetFanSpeed\0")
                    .ok()
                    .map(|s| *s);
                let fn_get_fan_speed_v2 = lib
                    .get::<FnNvmlDeviceGetFanSpeedV2>(b"nvmlDeviceGetFanSpeed_v2\0")
                    .ok()
                    .map(|s| *s);

                if fn_init() != NVML_SUCCESS {
                    return Vec::new();
                }

                let mut gpu_count: c_uint = 0;
                if fn_count(&mut gpu_count) != NVML_SUCCESS || gpu_count == 0 {
                    fn_shutdown();
                    return Vec::new();
                }

                let session = Arc::new(NvmlSession {
                    fn_shutdown,
                    fn_get_num_fans,
                    fn_get_fan_speed,
                    fn_get_fan_speed_v2,
                    fn_set_fan_speed_v2: fn_set_fan,
                    fn_set_default_fan_v2: fn_reset_fan,
                    _lib: lib,
                });

                let mut out: Vec<Box<dyn GpuCoolingBackend>> = Vec::new();
                for gpu_idx in 0..gpu_count.min(8) {
                    let mut device: NvmlDevice = std::ptr::null_mut();
                    if fn_handle(gpu_idx, &mut device) != NVML_SUCCESS || device.is_null() {
                        continue;
                    }
                    let mut fan_count = 0usize;
                    if let Some(f_nf) = session.fn_get_num_fans {
                        let mut nf: c_uint = 0;
                        if f_nf(device, &mut nf) == NVML_SUCCESS {
                            fan_count = (nf as usize).min(6);
                        }
                    }
                    if fan_count == 0
                        && let Some(f1) = session.fn_get_fan_speed
                    {
                        let mut spd: c_uint = 0;
                        if f1(device, &mut spd) == NVML_SUCCESS {
                            fan_count = 1;
                        }
                    }
                    if fan_count == 0 {
                        continue;
                    }

                    let mut name_buf = [0 as c_char; 128];
                    let model = if let Some(f_name) = fn_name
                        && f_name(device, name_buf.as_mut_ptr(), name_buf.len() as c_uint)
                            == NVML_SUCCESS
                    {
                        CStr::from_ptr(name_buf.as_ptr())
                            .to_string_lossy()
                            .trim()
                            .to_string()
                    } else {
                        format!("NVIDIA GPU #{}", gpu_idx + 1)
                    };

                    let mut dev = NvidiaGpuDevice {
                        id: format!("gpu:nvidia_{gpu_idx}"),
                        model,
                        device,
                        fan_count,
                        live_pcts: vec![None; fan_count],
                        manual_pcts: vec![None; fan_count],
                        session: Arc::clone(&session),
                    };
                    dev.refresh();
                    info!(
                        "NVML GPU refroidissement prêt : {} ({} ventilateur(s))",
                        dev.model, dev.fan_count
                    );
                    out.push(Box::new(dev));
                }
                out
            }
        }
    }

    impl GpuCoolingBackend for NvidiaGpuDevice {
        fn id(&self) -> &str {
            &self.id
        }

        fn vendor(&self) -> &str {
            "NVIDIA"
        }

        fn model(&self) -> &str {
            &self.model
        }

        fn firmware(&self) -> Option<String> {
            None
        }

        fn backend_label(&self) -> &str {
            "NVIDIA NVML (nvml.dll)"
        }

        fn fans(&self) -> Vec<FanInfo> {
            (0..self.fan_count)
                .map(|i| FanInfo {
                    id: format!("fan_{i}"),
                    name: if self.fan_count == 1 {
                        format!("{} — Ventilateur", self.model)
                    } else {
                        format!("{} — Ventilateur #{}", self.model, i + 1)
                    },
                    rpm: None,
                    max_rpm: None,
                    min_pct: GPU_MIN_FAN_PCT,
                    manual_pct: self.manual_pcts[i],
                })
                .collect()
        }

        fn refresh(&mut self) {
            for i in 0..self.fan_count {
                self.live_pcts[i] = self.read_fan_speed_pct(i);
            }
        }

        fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()> {
            let indices: Vec<usize> = match fan {
                Some(fid) => {
                    let idx = fid
                        .strip_prefix("fan_")
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|&i| i < self.fan_count)
                        .ok_or_else(|| anyhow::anyhow!("Ventilateur GPU introuvable : {fid}"))?;
                    vec![idx]
                }
                None => (0..self.fan_count).collect(),
            };

            let infos = self.fans();
            for idx in indices {
                let clamped = clamp_target(&infos[idx], target);
                let rc = match clamped {
                    FanTarget::Auto => unsafe {
                        (self.session.fn_set_default_fan_v2)(self.device, idx as c_uint)
                    },
                    FanTarget::Fixed(pct) => unsafe {
                        (self.session.fn_set_fan_speed_v2)(
                            self.device,
                            idx as c_uint,
                            pct as c_uint,
                        )
                    },
                };
                if rc != NVML_SUCCESS {
                    let detail = match rc {
                        3 => "non pris en charge par le VBIOS de ce GPU",
                        4 => "droits administrateur requis par le pilote NVIDIA",
                        _ => "échec appel NVML",
                    };
                    bail!("NVIDIA GPU {} fan_{idx} : {detail} (code {rc})", self.model);
                }
                self.manual_pcts[idx] = match clamped {
                    FanTarget::Auto => None,
                    FanTarget::Fixed(pct) => Some(pct),
                };
            }
            self.refresh();
            Ok(())
        }

        fn overridden(&self) -> bool {
            self.manual_pcts.iter().any(|p| p.is_some())
        }

        fn restore_firmware_control(&mut self) -> Result<()> {
            let mut first_err = None;
            for idx in 0..self.fan_count {
                let rc =
                    unsafe { (self.session.fn_set_default_fan_v2)(self.device, idx as c_uint) };
                if rc == NVML_SUCCESS {
                    self.manual_pcts[idx] = None;
                } else if first_err.is_none() {
                    first_err = Some(anyhow::anyhow!(
                        "NVIDIA GPU {} fan_{idx} : restauration Auto échouée (code {rc})",
                        self.model
                    ));
                }
            }
            self.refresh();
            match first_err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
    }

    // -------------------------------------------------------------------------
    // AMD ADL2 (`System32\atiadlxx.dll`): Overdrive8 + OverdriveN
    // -------------------------------------------------------------------------
    const ADL_OK: c_int = 0;
    const ADL_OK_WARNING: c_int = 1;
    const ADL_MAX_PATH: usize = 256;
    const ADL_PMLOG_MAX_SENSORS: usize = 256;
    const PMLOG_FAN_RPM: usize = 14;

    const OD8_COUNT: usize = 77;
    const ADL_OD8_FAN_ZERO_RPM_CONTROL: c_int = 1 << 9;
    const ADL_OD8_FAN_CURVE: c_int = 1 << 13;
    const ADL_OD8_OPTIMIZED_GPU_POWER_MODE: c_int = 1 << 16;

    const OD8_FAN_ZERORPM_CONTROL: usize = 15;
    const OD8_FAN_CURVE_TEMPERATURE_1: usize = 19;
    const OD8_FAN_CURVE_SPEED_1: usize = 20;
    const OD8_OPTIMZED_POWER_MODE: usize = 36;

    const ODN_CONTROL_TYPE_AUTO: c_int = 2;
    const ODN_CONTROL_TYPE_MANUAL: c_int = 3;

    type AdlContextHandle = *mut c_void;
    type AdlMainMallocCallback = unsafe extern "system" fn(c_int) -> *mut c_void;
    type FnAdl2MainControlCreate =
        unsafe extern "system" fn(AdlMainMallocCallback, c_int, *mut AdlContextHandle) -> c_int;
    type FnAdl2MainControlDestroy = unsafe extern "system" fn(AdlContextHandle) -> c_int;
    type FnAdl2AdapterNumberOfAdaptersGet =
        unsafe extern "system" fn(AdlContextHandle, *mut c_int) -> c_int;
    type FnAdl2AdapterAdapterInfoGet =
        unsafe extern "system" fn(AdlContextHandle, *mut AdlAdapterInfo, c_int) -> c_int;
    type FnAdl2NewQueryPmLogDataGet =
        unsafe extern "system" fn(AdlContextHandle, c_int, *mut AdlPmLogDataOutput) -> c_int;

    type FnAdl2Overdrive8InitSettingGet =
        unsafe extern "system" fn(AdlContextHandle, c_int, *mut AdlOd8InitSetting) -> c_int;
    type FnAdl2Overdrive8CurrentSettingGet =
        unsafe extern "system" fn(AdlContextHandle, c_int, *mut AdlOd8CurrentSetting) -> c_int;
    type FnAdl2Overdrive8SettingSet = unsafe extern "system" fn(
        AdlContextHandle,
        c_int,
        *mut AdlOd8SetSetting,
        *mut AdlOd8CurrentSetting,
    ) -> c_int;

    type FnAdl2OverdriveNCapabilitiesGet =
        unsafe extern "system" fn(AdlContextHandle, c_int, *mut AdlOdnCapabilities) -> c_int;
    type FnAdl2OverdriveNFanControlGet =
        unsafe extern "system" fn(AdlContextHandle, c_int, *mut AdlOdnFanControl) -> c_int;
    type FnAdl2OverdriveNFanControlSet =
        unsafe extern "system" fn(AdlContextHandle, c_int, *mut AdlOdnFanControl) -> c_int;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AdlAdapterInfo {
        i_size: c_int,
        i_adapter_index: c_int,
        str_udid: [c_char; ADL_MAX_PATH],
        i_bus_number: c_int,
        i_device_number: c_int,
        i_function_number: c_int,
        i_vendor_id: c_int,
        str_adapter_name: [c_char; ADL_MAX_PATH],
        str_display_name: [c_char; ADL_MAX_PATH],
        i_present: c_int,
        i_exist: c_int,
        str_driver_path: [c_char; ADL_MAX_PATH],
        str_driver_path_ext: [c_char; ADL_MAX_PATH],
        str_pnp_string: [c_char; ADL_MAX_PATH],
        i_os_display_index: c_int,
    }

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

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AdlOd8SingleInitSetting {
        feature_id: c_int,
        min_value: c_int,
        max_value: c_int,
        default_value: c_int,
    }

    #[repr(C)]
    #[derive(Clone)]
    struct AdlOd8InitSetting {
        count: c_int,
        overdrive8_capabilities: c_int,
        od8_setting_table: [AdlOd8SingleInitSetting; OD8_COUNT],
    }

    #[repr(C)]
    #[derive(Clone)]
    struct AdlOd8CurrentSetting {
        count: c_int,
        od8_setting_table: [c_int; OD8_COUNT],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AdlOd8SingleSetSetting {
        value: c_int,
        requested: c_int,
        reset: c_int,
    }

    #[repr(C)]
    struct AdlOd8SetSetting {
        count: c_int,
        od8_setting_table: [AdlOd8SingleSetSetting; OD8_COUNT],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AdlOdnParameterRange {
        i_min: c_int,
        i_max: c_int,
        i_step: c_int,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AdlOdnCapabilities {
        i_maximum_number_of_performance_levels: c_int,
        s_engine_clock_range: AdlOdnParameterRange,
        s_memory_clock_range: AdlOdnParameterRange,
        s_vddc_range: AdlOdnParameterRange,
        power: AdlOdnParameterRange,
        power_tune_temperature: AdlOdnParameterRange,
        fan_temperature: AdlOdnParameterRange,
        fan_speed: AdlOdnParameterRange,
        minimum_performance_clock: AdlOdnParameterRange,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AdlOdnFanControl {
        i_mode: c_int,
        i_fan_control_mode: c_int,
        i_current_fan_speed_mode: c_int,
        i_current_fan_speed: c_int,
        i_target_fan_speed: c_int,
        i_target_temperature: c_int,
        i_min_performance_clock: c_int,
        i_min_fan_limit: c_int,
    }

    unsafe extern "system" fn adl_malloc(size: c_int) -> *mut c_void {
        if size <= 0 {
            return std::ptr::null_mut();
        }
        unsafe { libc::calloc(1, size as usize) }
    }

    struct AdlSession {
        ctx: AdlContextHandle,
        fn_destroy: FnAdl2MainControlDestroy,
        fn_pmlog: Option<FnAdl2NewQueryPmLogDataGet>,
        fn_od8_init: Option<FnAdl2Overdrive8InitSettingGet>,
        fn_od8_current: Option<FnAdl2Overdrive8CurrentSettingGet>,
        fn_od8_set: Option<FnAdl2Overdrive8SettingSet>,
        fn_odn_caps: Option<FnAdl2OverdriveNCapabilitiesGet>,
        fn_odn_fan_get: Option<FnAdl2OverdriveNFanControlGet>,
        fn_odn_fan_set: Option<FnAdl2OverdriveNFanControlSet>,
        _lib: libloading::Library,
    }

    unsafe impl Send for AdlSession {}
    unsafe impl Sync for AdlSession {}

    impl Drop for AdlSession {
        fn drop(&mut self) {
            unsafe {
                (self.fn_destroy)(self.ctx);
            }
        }
    }

    #[derive(Clone)]
    enum AmdFanMode {
        Od8 { init: Box<AdlOd8InitSetting> },
        Odn { speed_range: AdlOdnParameterRange },
    }

    struct AmdAdlGpuDevice {
        id: String,
        model: String,
        adapter_idx: c_int,
        mode: AmdFanMode,
        live_rpm: Option<u32>,
        manual_pct: Option<u8>,
        saved_od8: Option<AdlOd8CurrentSetting>,
        saved_odn: Option<AdlOdnFanControl>,
        session: Arc<AdlSession>,
    }

    unsafe impl Send for AmdAdlGpuDevice {}

    impl AmdAdlGpuDevice {
        fn is_adl_ok(rc: c_int) -> bool {
            rc == ADL_OK || rc == ADL_OK_WARNING
        }

        fn query_od8_current(&self) -> Option<AdlOd8CurrentSetting> {
            let f_cur = self.session.fn_od8_current?;
            unsafe {
                let mut cur: AdlOd8CurrentSetting = std::mem::zeroed();
                cur.count = OD8_COUNT as c_int;
                if Self::is_adl_ok(f_cur(self.session.ctx, self.adapter_idx, &mut cur)) {
                    Some(cur)
                } else {
                    None
                }
            }
        }

        fn apply_od8_fan_curve(&mut self, init: &AdlOd8InitSetting, pct: u8) -> Result<()> {
            let f_set = self
                .session
                .fn_od8_set
                .ok_or_else(|| anyhow::anyhow!("ADL2 Overdrive8 indisponible"))?;
            let cur = self
                .query_od8_current()
                .ok_or_else(|| anyhow::anyhow!("Lecture Overdrive8 échouée"))?;
            if self.saved_od8.is_none() {
                self.saved_od8 = Some(cur.clone());
            }

            let t1_init = init.od8_setting_table[OD8_FAN_CURVE_TEMPERATURE_1];
            let s1_init = init.od8_setting_table[OD8_FAN_CURVE_SPEED_1];
            let mut base_temps = [0i32; 5];
            for (i, slot) in base_temps.iter_mut().enumerate() {
                let v = cur.od8_setting_table[OD8_FAN_CURVE_TEMPERATURE_1 + i * 2];
                *slot = if v > 0 {
                    v
                } else {
                    init.od8_setting_table[OD8_FAN_CURVE_TEMPERATURE_1 + i * 2].default_value
                };
            }
            let (temps, speeds) = build_od8_manual_curve(
                t1_init.min_value.max(20),
                t1_init.max_value.max(95),
                s1_init.min_value.clamp(0, 100),
                s1_init.max_value.clamp(GPU_MIN_FAN_PCT as i32, 100),
                base_temps,
                pct,
            );

            let od8_cnt = init.count.clamp(29, OD8_COUNT as c_int);
            let build_set = |with_manual_pwr_mode: bool| -> AdlOd8SetSetting {
                let mut set_req: AdlOd8SetSetting = unsafe { std::mem::zeroed() };
                set_req.count = od8_cnt;
                if with_manual_pwr_mode
                    && (init.overdrive8_capabilities & ADL_OD8_OPTIMIZED_GPU_POWER_MODE) != 0
                {
                    set_req.od8_setting_table[OD8_OPTIMZED_POWER_MODE] = AdlOd8SingleSetSetting {
                        value: 3,
                        requested: 1,
                        reset: 0,
                    };
                }
                if (init.overdrive8_capabilities & ADL_OD8_FAN_ZERO_RPM_CONTROL) != 0 {
                    set_req.od8_setting_table[OD8_FAN_ZERORPM_CONTROL] = AdlOd8SingleSetSetting {
                        value: 0,
                        requested: 1,
                        reset: 0,
                    };
                }
                for i in 0..5 {
                    set_req.od8_setting_table[OD8_FAN_CURVE_TEMPERATURE_1 + i * 2] =
                        AdlOd8SingleSetSetting {
                            value: temps[i],
                            requested: 1,
                            reset: 0,
                        };
                    set_req.od8_setting_table[OD8_FAN_CURVE_SPEED_1 + i * 2] =
                        AdlOd8SingleSetSetting {
                            value: speeds[i],
                            requested: 1,
                            reset: 0,
                        };
                }
                set_req
            };

            let mut out_cur: AdlOd8CurrentSetting = unsafe { std::mem::zeroed() };
            out_cur.count = od8_cnt;
            let mut req1 = build_set(false);
            let mut rc =
                unsafe { f_set(self.session.ctx, self.adapter_idx, &mut req1, &mut out_cur) };
            if !Self::is_adl_ok(rc)
                && (init.overdrive8_capabilities & ADL_OD8_OPTIMIZED_GPU_POWER_MODE) != 0
            {
                let mut req2 = build_set(true);
                rc = unsafe { f_set(self.session.ctx, self.adapter_idx, &mut req2, &mut out_cur) };
            }
            if !Self::is_adl_ok(rc) {
                bail!("AMD Overdrive8 refusé par le pilote (code {rc})");
            }
            self.manual_pct = Some(speeds[0].clamp(0, 100) as u8);
            Ok(())
        }

        fn restore_od8_auto(&mut self, init: &AdlOd8InitSetting) -> Result<()> {
            let f_set = self
                .session
                .fn_od8_set
                .ok_or_else(|| anyhow::anyhow!("ADL2 Overdrive8 indisponible"))?;
            let od8_cnt = init.count.clamp(29, OD8_COUNT as c_int);
            let mut set_req: AdlOd8SetSetting = unsafe { std::mem::zeroed() };
            set_req.count = od8_cnt;

            for i in 0..5 {
                let tid = OD8_FAN_CURVE_TEMPERATURE_1 + i * 2;
                let sid = OD8_FAN_CURVE_SPEED_1 + i * 2;
                if let Some(ref prev) = self.saved_od8 {
                    set_req.od8_setting_table[tid] = AdlOd8SingleSetSetting {
                        value: prev.od8_setting_table[tid],
                        requested: 1,
                        reset: 0,
                    };
                    set_req.od8_setting_table[sid] = AdlOd8SingleSetSetting {
                        value: prev.od8_setting_table[sid],
                        requested: 1,
                        reset: 0,
                    };
                } else {
                    set_req.od8_setting_table[tid] = AdlOd8SingleSetSetting {
                        value: init.od8_setting_table[tid].default_value,
                        requested: 1,
                        reset: 1,
                    };
                    set_req.od8_setting_table[sid] = AdlOd8SingleSetSetting {
                        value: init.od8_setting_table[sid].default_value,
                        requested: 1,
                        reset: 1,
                    };
                }
            }
            if (init.overdrive8_capabilities & ADL_OD8_FAN_ZERO_RPM_CONTROL) != 0 {
                let zero_val = self
                    .saved_od8
                    .as_ref()
                    .map(|p| p.od8_setting_table[OD8_FAN_ZERORPM_CONTROL])
                    .unwrap_or(init.od8_setting_table[OD8_FAN_ZERORPM_CONTROL].default_value);
                set_req.od8_setting_table[OD8_FAN_ZERORPM_CONTROL] = AdlOd8SingleSetSetting {
                    value: zero_val,
                    requested: 1,
                    reset: i32::from(self.saved_od8.is_none()),
                };
            }

            let mut out_cur: AdlOd8CurrentSetting = unsafe { std::mem::zeroed() };
            out_cur.count = od8_cnt;
            let rc = unsafe {
                f_set(
                    self.session.ctx,
                    self.adapter_idx,
                    &mut set_req,
                    &mut out_cur,
                )
            };
            if !Self::is_adl_ok(rc) {
                bail!("AMD Overdrive8 restauration Auto échouée (code {rc})");
            }
            self.saved_od8 = None;
            self.manual_pct = None;
            Ok(())
        }

        fn discover() -> Vec<Box<dyn GpuCoolingBackend>> {
            unsafe {
                let Ok(lib) = crate::wmi::load_system32_library("atiadlxx.dll") else {
                    return Vec::new();
                };
                let Ok(fn_create) = lib
                    .get::<FnAdl2MainControlCreate>(b"ADL2_Main_Control_Create\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };
                let Ok(fn_destroy) = lib
                    .get::<FnAdl2MainControlDestroy>(b"ADL2_Main_Control_Destroy\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };
                let Ok(fn_num) = lib
                    .get::<FnAdl2AdapterNumberOfAdaptersGet>(b"ADL2_Adapter_NumberOfAdapters_Get\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };
                let Ok(fn_info) = lib
                    .get::<FnAdl2AdapterAdapterInfoGet>(b"ADL2_Adapter_AdapterInfo_Get\0")
                    .map(|s| *s)
                else {
                    return Vec::new();
                };

                let fn_pmlog = lib
                    .get::<FnAdl2NewQueryPmLogDataGet>(b"ADL2_New_QueryPMLogData_Get\0")
                    .ok()
                    .map(|s| *s);
                let fn_od8_init = lib
                    .get::<FnAdl2Overdrive8InitSettingGet>(b"ADL2_Overdrive8_Init_Setting_Get\0")
                    .ok()
                    .map(|s| *s);
                let fn_od8_current = lib
                    .get::<FnAdl2Overdrive8CurrentSettingGet>(
                        b"ADL2_Overdrive8_Current_Setting_Get\0",
                    )
                    .ok()
                    .map(|s| *s);
                let fn_od8_set = lib
                    .get::<FnAdl2Overdrive8SettingSet>(b"ADL2_Overdrive8_Setting_Set\0")
                    .ok()
                    .map(|s| *s);
                let fn_odn_caps = lib
                    .get::<FnAdl2OverdriveNCapabilitiesGet>(b"ADL2_OverdriveN_Capabilities_Get\0")
                    .ok()
                    .map(|s| *s);
                let fn_odn_fan_get = lib
                    .get::<FnAdl2OverdriveNFanControlGet>(b"ADL2_OverdriveN_FanControl_Get\0")
                    .ok()
                    .map(|s| *s);
                let fn_odn_fan_set = lib
                    .get::<FnAdl2OverdriveNFanControlSet>(b"ADL2_OverdriveN_FanControl_Set\0")
                    .ok()
                    .map(|s| *s);

                let mut ctx: AdlContextHandle = std::ptr::null_mut();
                if fn_create(adl_malloc, 1, &mut ctx) != ADL_OK || ctx.is_null() {
                    return Vec::new();
                }

                let mut count: c_int = 0;
                if fn_num(ctx, &mut count) != ADL_OK || count <= 0 {
                    fn_destroy(ctx);
                    return Vec::new();
                }

                let alloc_count = count.clamp(1, 64) as usize;
                let mut infos: Vec<AdlAdapterInfo> = vec![std::mem::zeroed(); alloc_count];
                for item in &mut infos {
                    item.i_size = std::mem::size_of::<AdlAdapterInfo>() as c_int;
                }
                let buf_bytes = (alloc_count * std::mem::size_of::<AdlAdapterInfo>()) as c_int;
                if fn_info(ctx, infos.as_mut_ptr(), buf_bytes) != ADL_OK {
                    fn_destroy(ctx);
                    return Vec::new();
                }

                let session = Arc::new(AdlSession {
                    ctx,
                    fn_destroy,
                    fn_pmlog,
                    fn_od8_init,
                    fn_od8_current,
                    fn_od8_set,
                    fn_odn_caps,
                    fn_odn_fan_get,
                    fn_odn_fan_set,
                    _lib: lib,
                });

                let mut seen_bdf: Vec<(c_int, c_int, c_int)> = Vec::new();
                let mut gpu_ordinal = 0usize;
                let mut out: Vec<Box<dyn GpuCoolingBackend>> = Vec::new();

                for info in &infos {
                    let vid = info.i_vendor_id & 0xFFFF;
                    if info.i_exist == 0 || (vid != 0x1002 && vid != 1002) {
                        continue;
                    }
                    let bdf = (
                        info.i_bus_number,
                        info.i_device_number,
                        info.i_function_number,
                    );
                    if seen_bdf.contains(&bdf) {
                        continue;
                    }
                    let adapter_idx = info.i_adapter_index;

                    let od8_mode = if let (Some(f_init), Some(_), Some(_)) = (
                        session.fn_od8_init,
                        session.fn_od8_current,
                        session.fn_od8_set,
                    ) {
                        let mut init: AdlOd8InitSetting = std::mem::zeroed();
                        init.count = OD8_COUNT as c_int;
                        if Self::is_adl_ok(f_init(session.ctx, adapter_idx, &mut init))
                            && init.count > 28
                            && (init.overdrive8_capabilities & ADL_OD8_FAN_CURVE) != 0
                        {
                            Some(AmdFanMode::Od8 {
                                init: Box::new(init),
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let mode = if let Some(m) = od8_mode {
                        Some(m)
                    } else if let (Some(f_get), Some(_)) =
                        (session.fn_odn_fan_get, session.fn_odn_fan_set)
                    {
                        let mut fc = AdlOdnFanControl::default();
                        if Self::is_adl_ok(f_get(session.ctx, adapter_idx, &mut fc)) {
                            let mut range = AdlOdnParameterRange {
                                i_min: 0,
                                i_max: 100,
                                i_step: 1,
                            };
                            if let Some(f_caps) = session.fn_odn_caps {
                                let mut caps = AdlOdnCapabilities::default();
                                if Self::is_adl_ok(f_caps(session.ctx, adapter_idx, &mut caps))
                                    && caps.fan_speed.i_max > caps.fan_speed.i_min
                                {
                                    range = caps.fan_speed;
                                }
                            }
                            Some(AmdFanMode::Odn { speed_range: range })
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let Some(mode) = mode else {
                        continue;
                    };
                    seen_bdf.push(bdf);

                    let model = CStr::from_ptr(info.str_adapter_name.as_ptr())
                        .to_string_lossy()
                        .trim()
                        .to_string();
                    let model = if model.is_empty() {
                        format!("AMD Radeon GPU #{}", gpu_ordinal + 1)
                    } else {
                        model
                    };

                    let mut dev = AmdAdlGpuDevice {
                        id: format!("gpu:amd_{gpu_ordinal}"),
                        model,
                        adapter_idx,
                        mode,
                        live_rpm: None,
                        manual_pct: None,
                        saved_od8: None,
                        saved_odn: None,
                        session: Arc::clone(&session),
                    };
                    gpu_ordinal += 1;
                    dev.refresh();
                    info!(
                        "ADL2 GPU refroidissement prêt : {} (adapter #{adapter_idx})",
                        dev.model
                    );
                    out.push(Box::new(dev));
                }
                out
            }
        }
    }

    impl GpuCoolingBackend for AmdAdlGpuDevice {
        fn id(&self) -> &str {
            &self.id
        }

        fn vendor(&self) -> &str {
            "AMD"
        }

        fn model(&self) -> &str {
            &self.model
        }

        fn firmware(&self) -> Option<String> {
            None
        }

        fn backend_label(&self) -> &str {
            match self.mode {
                AmdFanMode::Od8 { .. } => "AMD ADL2 Overdrive8 (atiadlxx.dll)",
                AmdFanMode::Odn { .. } => "AMD ADL2 OverdriveN (atiadlxx.dll)",
            }
        }

        fn fans(&self) -> Vec<FanInfo> {
            vec![FanInfo {
                id: "fan_0".into(),
                name: format!("{} — Ventilateur GPU", self.model),
                rpm: self.live_rpm,
                max_rpm: None,
                min_pct: GPU_MIN_FAN_PCT,
                manual_pct: self.manual_pct,
            }]
        }

        fn refresh(&mut self) {
            if let Some(f_pm) = self.session.fn_pmlog {
                unsafe {
                    let mut out: AdlPmLogDataOutput = std::mem::zeroed();
                    out.size = std::mem::size_of::<AdlPmLogDataOutput>() as c_int;
                    if Self::is_adl_ok(f_pm(self.session.ctx, self.adapter_idx, &mut out)) {
                        self.live_rpm = (out.sensors[PMLOG_FAN_RPM].supported != 0)
                            .then_some(out.sensors[PMLOG_FAN_RPM].value.max(0) as u32);
                    }
                }
            }
        }

        fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()> {
            if let Some(fid) = fan
                && fid != "fan_0"
            {
                bail!("Ventilateur GPU introuvable : {fid}");
            }
            let info = self.fans().into_iter().next().expect("single GPU fan");
            match clamp_target(&info, target) {
                FanTarget::Auto => self.restore_firmware_control(),
                FanTarget::Fixed(pct) => {
                    let mode = self.mode.clone();
                    match mode {
                        AmdFanMode::Od8 { init } => self.apply_od8_fan_curve(&init, pct)?,
                        AmdFanMode::Odn { speed_range } => {
                            let f_get = self
                                .session
                                .fn_odn_fan_get
                                .ok_or_else(|| anyhow::anyhow!("ADL2 OverdriveN indisponible"))?;
                            let f_set = self
                                .session
                                .fn_odn_fan_set
                                .ok_or_else(|| anyhow::anyhow!("ADL2 OverdriveN indisponible"))?;
                            let mut fc = AdlOdnFanControl::default();
                            let rc = unsafe { f_get(self.session.ctx, self.adapter_idx, &mut fc) };
                            if !Self::is_adl_ok(rc) {
                                bail!("Lecture OverdriveN échouée (code {rc})");
                            }
                            if self.saved_odn.is_none() {
                                self.saved_odn = Some(fc);
                            }
                            let drv_val =
                                odn_scale_pct_to_driver(pct, speed_range.i_min, speed_range.i_max);
                            fc.i_mode = ODN_CONTROL_TYPE_MANUAL;
                            fc.i_target_fan_speed = drv_val;
                            let rc = unsafe { f_set(self.session.ctx, self.adapter_idx, &mut fc) };
                            if !Self::is_adl_ok(rc) {
                                bail!("AMD OverdriveN refusé par le pilote (code {rc})");
                            }
                            self.manual_pct = Some(odn_driver_to_pct(
                                drv_val,
                                speed_range.i_min,
                                speed_range.i_max,
                                speed_range.i_max > 100,
                            ));
                        }
                    }
                    self.refresh();
                    Ok(())
                }
            }
        }

        fn overridden(&self) -> bool {
            self.manual_pct.is_some()
        }

        fn restore_firmware_control(&mut self) -> Result<()> {
            let mode = self.mode.clone();
            match mode {
                AmdFanMode::Od8 { init } => self.restore_od8_auto(&init)?,
                AmdFanMode::Odn { .. } => {
                    let f_get = self
                        .session
                        .fn_odn_fan_get
                        .ok_or_else(|| anyhow::anyhow!("ADL2 OverdriveN indisponible"))?;
                    let f_set = self
                        .session
                        .fn_odn_fan_set
                        .ok_or_else(|| anyhow::anyhow!("ADL2 OverdriveN indisponible"))?;
                    let mut fc = match self.saved_odn.take() {
                        Some(prev) => prev,
                        None => {
                            let mut cur = AdlOdnFanControl::default();
                            let _ = unsafe { f_get(self.session.ctx, self.adapter_idx, &mut cur) };
                            cur.i_mode = ODN_CONTROL_TYPE_AUTO;
                            cur
                        }
                    };
                    let rc = unsafe { f_set(self.session.ctx, self.adapter_idx, &mut fc) };
                    if !Self::is_adl_ok(rc) {
                        bail!("AMD OverdriveN restauration Auto échouée (code {rc})");
                    }
                    self.manual_pct = None;
                }
            }
            self.refresh();
            Ok(())
        }
    }

    pub(super) fn discover_all() -> Vec<Box<dyn GpuCoolingBackend>> {
        let mut out = NvidiaGpuDevice::discover();
        out.extend(AmdAdlGpuDevice::discover());
        out
    }
}

/// Shared `Driver` exposing controllable GPU cooling devices via the common `Cooling` trait.
pub struct GpuCoolingDriver;

impl Driver for GpuCoolingDriver {
    fn name(&self) -> &'static str {
        "gpu"
    }

    fn display_name(&self) -> &'static str {
        "Ventilation carte graphique (NVML / AMD ADL2)"
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        #[cfg(windows)]
        {
            win_gpu::discover_all()
                .into_iter()
                .map(|backend| -> Box<dyn Device> { Box::new(GpuCoolingDevice { backend }) })
                .collect()
        }
        #[cfg(not(windows))]
        Vec::new()
    }
}

/// Physical GPU exposed through the shared `Cooling` capability.
pub struct GpuCoolingDevice {
    backend: Box<dyn GpuCoolingBackend>,
}

impl Device for GpuCoolingDevice {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.backend.id().to_string(),
            vendor: self.backend.vendor().to_string(),
            model: self.backend.model().to_string(),
            kind: DeviceKind::Gpu,
            connection: Connection::Internal,
            firmware: self.backend.firmware(),
        }
    }

    fn specs(&self) -> Vec<DeviceSpec> {
        vec![
            DeviceSpec::simple("Interface", self.backend.backend_label().to_string()),
            DeviceSpec::simple("Canaux ventilateur", self.backend.fans().len().to_string()),
        ]
    }

    fn cooling(&mut self) -> Option<&mut dyn Cooling> {
        Some(self)
    }
}

impl Cooling for GpuCoolingDevice {
    fn fans(&self) -> Vec<FanInfo> {
        self.backend.fans()
    }

    fn refresh(&mut self) {
        self.backend.refresh();
    }

    fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()> {
        self.backend.set_fan(fan, target)
    }

    fn overridden(&self) -> bool {
        self.backend.overridden()
    }

    fn restore_firmware_control(&mut self) -> Result<()> {
        self.backend.restore_firmware_control()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odn_scaling_handles_percentage_and_rpm_ranges() {
        assert_eq!(odn_scale_pct_to_driver(0, 20, 100), GPU_MIN_FAN_PCT as i32);
        assert_eq!(odn_scale_pct_to_driver(75, 20, 100), 75);
        assert_eq!(odn_scale_pct_to_driver(120, 20, 100), 100);
        // Polaris/Vega cards often expose RPM range (e.g. 500..3000 RPM)
        assert_eq!(odn_scale_pct_to_driver(50, 500, 3000), 1750);
        assert_eq!(odn_scale_pct_to_driver(100, 500, 3000), 3000);
        assert_eq!(odn_driver_to_pct(1750, 500, 3000, true), 50);
    }

    #[test]
    fn od8_curve_builder_enforces_monotonic_temps_and_safety_floor() {
        let (temps, speeds) = build_od8_manual_curve(30, 95, 25, 100, [30, 55, 50, 75, 85], 10);
        assert_eq!(temps, [30, 55, 55, 75, 85]);
        assert_eq!(speeds, [GPU_MIN_FAN_PCT as i32; 5]);

        let (temps2, speeds2) = build_od8_manual_curve(35, 90, 20, 100, [0, 0, 0, 0, 0], 82);
        for i in 1..5 {
            assert!(temps2[i] >= temps2[i - 1]);
        }
        assert_eq!(speeds2, [82; 5]);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires live AMD GPU hardware"]
    fn live_probe_amd_gpu_cooling() {
        use std::ffi::{CStr, c_int, c_void};
        let lib = crate::wmi::load_system32_library("atiadlxx.dll").expect("atiadlxx.dll");
        type FnCreate = unsafe extern "system" fn(
            unsafe extern "system" fn(c_int) -> *mut c_void,
            c_int,
            *mut *mut c_void,
        ) -> c_int;
        type FnDestroy = unsafe extern "system" fn(*mut c_void) -> c_int;
        type FnNum = unsafe extern "system" fn(*mut c_void, *mut c_int) -> c_int;
        type FnInfo = unsafe extern "system" fn(*mut c_void, *mut c_void, c_int) -> c_int;
        type FnCaps = unsafe extern "system" fn(
            *mut c_void,
            c_int,
            *mut c_int,
            *mut c_int,
            *mut c_int,
        ) -> c_int;

        unsafe extern "system" fn malloc_cb(sz: c_int) -> *mut c_void {
            if sz <= 0 {
                std::ptr::null_mut()
            } else {
                unsafe { libc::calloc(1, sz as usize) }
            }
        }

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct RawAdapterInfo {
            i_size: c_int,
            i_adapter_index: c_int,
            str_udid: [std::ffi::c_char; 256],
            i_bus: c_int,
            i_dev: c_int,
            i_func: c_int,
            i_vendor: c_int,
            str_name: [std::ffi::c_char; 256],
            str_disp: [std::ffi::c_char; 256],
            i_present: c_int,
            i_exist: c_int,
            str_drv: [std::ffi::c_char; 256],
            str_drv_ext: [std::ffi::c_char; 256],
            str_pnp: [std::ffi::c_char; 256],
            i_os_idx: c_int,
        }

        unsafe {
            let fn_create: FnCreate = *lib.get(b"ADL2_Main_Control_Create\0").unwrap();
            let fn_destroy: FnDestroy = *lib.get(b"ADL2_Main_Control_Destroy\0").unwrap();
            let fn_num: FnNum = *lib.get(b"ADL2_Adapter_NumberOfAdapters_Get\0").unwrap();
            let fn_info: FnInfo = *lib.get(b"ADL2_Adapter_AdapterInfo_Get\0").unwrap();
            let fn_caps = lib.get::<FnCaps>(b"ADL2_Overdrive_Caps\0").ok().map(|s| *s);

            let mut ctx: *mut c_void = std::ptr::null_mut();
            assert_eq!(fn_create(malloc_cb, 1, &mut ctx), 0);
            let mut cnt: c_int = 0;
            assert_eq!(fn_num(ctx, &mut cnt), 0);
            println!("ADL adapter count = {cnt}");
            let mut infos = vec![std::mem::zeroed::<RawAdapterInfo>(); cnt as usize];
            for it in &mut infos {
                it.i_size = std::mem::size_of::<RawAdapterInfo>() as c_int;
            }
            let rc_info = fn_info(
                ctx,
                infos.as_mut_ptr() as *mut c_void,
                cnt * std::mem::size_of::<RawAdapterInfo>() as c_int,
            );
            println!("AdapterInfo_Get rc = {rc_info}");
            for a in &infos {
                let name = CStr::from_ptr(a.str_name.as_ptr()).to_string_lossy();
                let mut sup = -1;
                let mut ena = -1;
                let mut ver = -1;
                let caps_rc = if let Some(fc) = fn_caps {
                    fc(ctx, a.i_adapter_index, &mut sup, &mut ena, &mut ver)
                } else {
                    -999
                };
                println!(
                    "idx={} exist={} present={} vendor=0x{:X} bdf={}.{}.{} name='{}' OD_caps(rc={caps_rc}, sup={sup}, ena={ena}, ver={ver})",
                    a.i_adapter_index,
                    a.i_exist,
                    a.i_present,
                    a.i_vendor,
                    a.i_bus,
                    a.i_dev,
                    a.i_func,
                    name
                );
            }
            if let Some(first) = infos.first() {
                type FnOd8Init =
                    unsafe extern "system" fn(*mut c_void, c_int, *mut c_void) -> c_int;
                type FnOd8Cur = unsafe extern "system" fn(*mut c_void, c_int, *mut c_void) -> c_int;
                #[repr(C)]
                struct ProbeSingleInit {
                    f: c_int,
                    min: c_int,
                    max: c_int,
                    def: c_int,
                }
                #[repr(C)]
                struct ProbeInit {
                    cnt: c_int,
                    caps: c_int,
                    t: [ProbeSingleInit; 77],
                }
                #[repr(C)]
                struct ProbeCur {
                    cnt: c_int,
                    t: [c_int; 77],
                }
                if let (Ok(fi), Ok(fc)) = (
                    lib.get::<FnOd8Init>(b"ADL2_Overdrive8_Init_Setting_Get\0"),
                    lib.get::<FnOd8Cur>(b"ADL2_Overdrive8_Current_Setting_Get\0"),
                ) {
                    let mut init: ProbeInit = std::mem::zeroed();
                    init.cnt = 77;
                    let r_i = fi(
                        ctx,
                        first.i_adapter_index,
                        &mut init as *mut _ as *mut c_void,
                    );
                    let mut cur: ProbeCur = std::mem::zeroed();
                    cur.cnt = 77;
                    let r_c = fc(
                        ctx,
                        first.i_adapter_index,
                        &mut cur as *mut _ as *mut c_void,
                    );
                    println!(
                        "OD8 Init rc={r_i} cnt={} caps=0x{:X} Cur rc={r_c} zero_rpm=(min={},max={},def={},cur={}) opt_mode=(def={},cur={})",
                        init.cnt,
                        init.caps,
                        init.t[15].min,
                        init.t[15].max,
                        init.t[15].def,
                        cur.t[15],
                        init.t[36].def,
                        cur.t[36]
                    );
                    for p in 0..5 {
                        let ti = 19 + p * 2;
                        let si = 20 + p * 2;
                        println!(
                            "  pt{}: T(min={},max={},def={},cur={}) S(min={},max={},def={},cur={})",
                            p + 1,
                            init.t[ti].min,
                            init.t[ti].max,
                            init.t[ti].def,
                            cur.t[ti],
                            init.t[si].min,
                            init.t[si].max,
                            init.t[si].def,
                            cur.t[si]
                        );
                    }
                }
            }
            fn_destroy(ctx);
        }

        let mut discovered = GpuCoolingDriver.discover();
        println!("GpuCoolingDriver.discover().len() = {}", discovered.len());
        assert!(!discovered.is_empty(), "Expected live AMD GPU");
        let cooling = discovered[0].cooling().expect("cooling capability");
        println!("fans before = {:?}", cooling.fans());
        cooling
            .set_fan(Some("fan_0"), FanTarget::Fixed(45))
            .expect("set_fan(45%)");
        std::thread::sleep(std::time::Duration::from_millis(800));
        cooling.refresh();
        println!("fans at 45% = {:?}", cooling.fans());
        assert!(cooling.overridden());
        cooling
            .restore_firmware_control()
            .expect("restore_firmware_control()");
        cooling.refresh();
        println!("fans restored = {:?}", cooling.fans());
        assert!(!cooling.overridden());
    }
}
