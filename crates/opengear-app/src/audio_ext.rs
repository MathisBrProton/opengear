//! Native zero-dependency Windows Core Audio (WASAPI) controller & real-time peak meter:
//! - Live output peak level (0.0..=1.0) via `IAudioMeterInformation::GetPeakValue` for RGB Audio Visualizer
//! - Master Output Volume (0..=100%) + Master Mute via `IAudioEndpointVolume`
//! - Microphone Capture Volume (0..=100%) + Microphone Mute via `IAudioEndpointVolume`

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemAudioState {
    pub available: bool,
    pub output_volume_pct: u8,
    pub output_muted: bool,
    pub output_peak: f32,
    pub mic_available: bool,
    pub mic_volume_pct: u8,
    pub mic_muted: bool,
    pub mic_peak: f32,
}

#[cfg(windows)]
mod win_audio {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows_sys::core::GUID;

    const CLSID_MM_DEVICE_ENUMERATOR: GUID = GUID {
        data1: 0xBCDE0395,
        data2: 0xE52F,
        data3: 0x467C,
        data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
    };
    const IID_IMM_DEVICE_ENUMERATOR: GUID = GUID {
        data1: 0xA95664D2,
        data2: 0x9614,
        data3: 0x4F35,
        data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
    };
    const IID_IAUDIO_METER_INFORMATION: GUID = GUID {
        data1: 0xC02216F6,
        data2: 0x8C67,
        data3: 0x4B5B,
        data4: [0x9D, 0x00, 0xD0, 0x08, 0xE7, 0x3E, 0x00, 0x64],
    };
    const IID_IAUDIO_ENDPOINT_VOLUME: GUID = GUID {
        data1: 0x5CDF2C82,
        data2: 0x841E,
        data3: 0x4546,
        data4: [0x97, 0x22, 0x0C, 0xF7, 0x40, 0x78, 0x22, 0x9A],
    };

    const E_RENDER: u32 = 0;
    const E_CAPTURE: u32 = 1;
    const E_CONSOLE: u32 = 0;

    #[repr(C)]
    struct ComObj {
        vtable: *const *const c_void,
    }

    unsafe fn release(ptr: *mut c_void) {
        if !ptr.is_null() {
            unsafe {
                let vtbl = (*(ptr as *const ComObj)).vtable;
                let func: unsafe extern "system" fn(*mut c_void) -> u32 =
                    std::mem::transmute(*vtbl.add(2));
                func(ptr);
            }
        }
    }

    unsafe fn get_default_endpoint(flow: u32) -> *mut c_void {
        unsafe {
            let _ = CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
            let mut enumerator: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_MM_DEVICE_ENUMERATOR,
                std::ptr::null_mut(),
                CLSCTX_ALL,
                &IID_IMM_DEVICE_ENUMERATOR,
                &mut enumerator,
            );
            if hr < 0 || enumerator.is_null() {
                return std::ptr::null_mut();
            }
            let vtbl = (*(enumerator as *const ComObj)).vtable;
            let get_def: unsafe extern "system" fn(*mut c_void, u32, u32, *mut *mut c_void) -> i32 =
                std::mem::transmute(*vtbl.add(4));
            let mut dev: *mut c_void = std::ptr::null_mut();
            let hr2 = get_def(enumerator, flow, E_CONSOLE, &mut dev);
            release(enumerator);
            if hr2 < 0 {
                return std::ptr::null_mut();
            }
            dev
        }
    }

    unsafe fn activate(dev: *mut c_void, iid: &GUID) -> *mut c_void {
        if dev.is_null() {
            return std::ptr::null_mut();
        }
        unsafe {
            let vtbl = (*(dev as *const ComObj)).vtable;
            let act: unsafe extern "system" fn(
                *mut c_void,
                *const GUID,
                u32,
                *mut c_void,
                *mut *mut c_void,
            ) -> i32 = std::mem::transmute(*vtbl.add(3));
            let mut out: *mut c_void = std::ptr::null_mut();
            if act(dev, iid, CLSCTX_ALL, std::ptr::null_mut(), &mut out) < 0 {
                return std::ptr::null_mut();
            }
            out
        }
    }

    unsafe fn read_endpoint(flow: u32) -> Option<(u8, bool, f32)> {
        unsafe {
            let dev = get_default_endpoint(flow);
            if dev.is_null() {
                return None;
            }
            let vol = activate(dev, &IID_IAUDIO_ENDPOINT_VOLUME);
            let meter = activate(dev, &IID_IAUDIO_METER_INFORMATION);
            release(dev);

            let mut pct = 0u8;
            let mut muted = false;
            if !vol.is_null() {
                let vtbl = (*(vol as *const ComObj)).vtable;
                let get_scalar: unsafe extern "system" fn(*mut c_void, *mut f32) -> i32 =
                    std::mem::transmute(*vtbl.add(9));
                let get_mute: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32 =
                    std::mem::transmute(*vtbl.add(15));
                let mut f = 0.0f32;
                if get_scalar(vol, &mut f) >= 0 {
                    pct = (f.clamp(0.0, 1.0) * 100.0).round() as u8;
                }
                let mut m = 0i32;
                if get_mute(vol, &mut m) >= 0 {
                    muted = m != 0;
                }
                release(vol);
            }

            let mut peak = 0.0f32;
            if !meter.is_null() {
                let vtbl = (*(meter as *const ComObj)).vtable;
                let get_peak: unsafe extern "system" fn(*mut c_void, *mut f32) -> i32 =
                    std::mem::transmute(*vtbl.add(3));
                let _ = get_peak(meter, &mut peak);
                release(meter);
            }
            Some((pct, muted, peak.clamp(0.0, 1.0)))
        }
    }

    pub fn read_audio_state() -> SystemAudioState {
        let mut st = SystemAudioState::default();
        unsafe {
            if let Some((pct, muted, peak)) = read_endpoint(E_RENDER) {
                st.available = true;
                st.output_volume_pct = pct;
                st.output_muted = muted;
                st.output_peak = peak;
            }
            if let Some((pct, muted, peak)) = read_endpoint(E_CAPTURE) {
                st.mic_available = true;
                st.mic_volume_pct = pct;
                st.mic_muted = muted;
                st.mic_peak = peak;
            }
        }
        st
    }

    pub fn read_output_peak() -> f32 {
        unsafe {
            let dev = get_default_endpoint(E_RENDER);
            if dev.is_null() {
                return 0.0;
            }
            let meter = activate(dev, &IID_IAUDIO_METER_INFORMATION);
            release(dev);
            if meter.is_null() {
                return 0.0;
            }
            let vtbl = (*(meter as *const ComObj)).vtable;
            let get_peak: unsafe extern "system" fn(*mut c_void, *mut f32) -> i32 =
                std::mem::transmute(*vtbl.add(3));
            let mut peak = 0.0f32;
            let _ = get_peak(meter, &mut peak);
            release(meter);
            peak.clamp(0.0, 1.0)
        }
    }

    pub fn set_endpoint_volume(is_mic: bool, pct: u8) -> anyhow::Result<()> {
        unsafe {
            let dev = get_default_endpoint(if is_mic { E_CAPTURE } else { E_RENDER });
            if dev.is_null() {
                anyhow::bail!("Périphérique audio Windows introuvable");
            }
            let vol = activate(dev, &IID_IAUDIO_ENDPOINT_VOLUME);
            release(dev);
            if vol.is_null() {
                anyhow::bail!("Interface IAudioEndpointVolume indisponible");
            }
            let vtbl = (*(vol as *const ComObj)).vtable;
            let set_scalar: unsafe extern "system" fn(*mut c_void, f32, *const GUID) -> i32 =
                std::mem::transmute(*vtbl.add(7));
            let hr = set_scalar(vol, (pct.min(100) as f32) / 100.0, std::ptr::null());
            release(vol);
            if hr < 0 {
                anyhow::bail!("SetMasterVolumeLevelScalar échoué (HR=0x{:08X})", hr as u32);
            }
            Ok(())
        }
    }

    pub fn set_endpoint_mute(is_mic: bool, muted: bool) -> anyhow::Result<()> {
        unsafe {
            let dev = get_default_endpoint(if is_mic { E_CAPTURE } else { E_RENDER });
            if dev.is_null() {
                anyhow::bail!("Périphérique audio Windows introuvable");
            }
            let vol = activate(dev, &IID_IAUDIO_ENDPOINT_VOLUME);
            release(dev);
            if vol.is_null() {
                anyhow::bail!("Interface IAudioEndpointVolume indisponible");
            }
            let vtbl = (*(vol as *const ComObj)).vtable;
            let set_mute: unsafe extern "system" fn(*mut c_void, i32, *const GUID) -> i32 =
                std::mem::transmute(*vtbl.add(14));
            let hr = set_mute(vol, if muted { 1 } else { 0 }, std::ptr::null());
            release(vol);
            if hr < 0 {
                anyhow::bail!("SetMute échoué (HR=0x{:08X})", hr as u32);
            }
            Ok(())
        }
    }
}

pub fn read_audio_state() -> SystemAudioState {
    #[cfg(windows)]
    {
        win_audio::read_audio_state()
    }
    #[cfg(not(windows))]
    SystemAudioState::default()
}

pub fn read_output_peak() -> f32 {
    #[cfg(windows)]
    {
        win_audio::read_output_peak()
    }
    #[cfg(not(windows))]
    0.0
}

pub fn set_endpoint_volume(is_mic: bool, pct: u8) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        win_audio::set_endpoint_volume(is_mic, pct)
    }
    #[cfg(not(windows))]
    {
        let _ = (is_mic, pct);
        anyhow::bail!("Non supporté hors Windows")
    }
}

pub fn set_endpoint_mute(is_mic: bool, muted: bool) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        win_audio::set_endpoint_mute(is_mic, muted)
    }
    #[cfg(not(windows))]
    {
        let _ = (is_mic, muted);
        anyhow::bail!("Non supporté hors Windows")
    }
}
