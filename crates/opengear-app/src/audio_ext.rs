use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
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
    use super::SystemAudioState;
    use anyhow::Context;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
    use windows::Win32::Media::Audio::{
        EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    /// Balanced RAII COM initializer for threads using Windows Core Audio APIs.
    struct ComGuard {
        should_uninit: bool,
    }

    impl ComGuard {
        fn enter() -> Option<Self> {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            if hr.is_ok() {
                Some(Self {
                    should_uninit: true,
                })
            } else if hr == RPC_E_CHANGED_MODE {
                Some(Self {
                    should_uninit: false,
                })
            } else {
                None
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.should_uninit {
                unsafe { CoUninitialize() };
            }
        }
    }

    thread_local! {
        // Declared BEFORE `ComGuard` so `IAudioMeterInformation::Release()` runs prior to `CoUninitialize()`.
        static RENDER_METER_CACHE: RefCell<Option<(IAudioMeterInformation, ComGuard)>> =
            const { RefCell::new(None) };
    }

    use std::cell::RefCell;

    fn get_default_endpoint(flow: EDataFlow) -> windows::core::Result<IMMDevice> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            enumerator.GetDefaultAudioEndpoint(flow, eConsole)
        }
    }

    fn read_endpoint_state(flow: EDataFlow) -> Option<(u8, bool, f32)> {
        let dev = get_default_endpoint(flow).ok()?;
        let vol: IAudioEndpointVolume = unsafe { dev.Activate(CLSCTX_ALL, None).ok()? };
        let level = unsafe { vol.GetMasterVolumeLevelScalar().unwrap_or(0.0) };
        let muted = unsafe { vol.GetMute().map(|b| b.as_bool()).unwrap_or(false) };

        let peak = unsafe {
            dev.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
                .ok()
                .and_then(|m| m.GetPeakValue().ok())
                .unwrap_or(0.0)
        };

        Some((
            (level * 100.0).round().clamp(0.0, 100.0) as u8,
            muted,
            peak.clamp(0.0, 1.0),
        ))
    }

    pub fn read_audio_state() -> SystemAudioState {
        let _com = match ComGuard::enter() {
            Some(g) => g,
            None => {
                return SystemAudioState {
                    available: false,
                    output_volume_pct: 0,
                    output_muted: false,
                    output_peak: 0.0,
                    mic_available: false,
                    mic_volume_pct: 0,
                    mic_muted: false,
                    mic_peak: 0.0,
                };
            }
        };

        let out = read_endpoint_state(eRender);
        let mic = read_endpoint_state(eCapture);

        SystemAudioState {
            available: out.is_some(),
            output_volume_pct: out.map(|v| v.0).unwrap_or(0),
            output_muted: out.map(|v| v.1).unwrap_or(false),
            output_peak: out.map(|v| v.2).unwrap_or(0.0),
            mic_available: mic.is_some(),
            mic_volume_pct: mic.map(|v| v.0).unwrap_or(0),
            mic_muted: mic.map(|v| v.1).unwrap_or(false),
            mic_peak: mic.map(|v| v.2).unwrap_or(0.0),
        }
    }

    /// Cached 30 Hz render peak reader: avoids re-enumerating WASAPI endpoints every frame and
    /// re-resolves cleanly if the default audio device changes or disconnects.
    pub fn read_output_peak() -> f32 {
        RENDER_METER_CACHE.with(|cell| {
            let mut cache = cell.borrow_mut();
            if let Some((ref meter, _)) = *cache {
                if let Ok(peak) = unsafe { meter.GetPeakValue() } {
                    return peak.clamp(0.0, 1.0);
                }
                *cache = None;
            }

            let com = match ComGuard::enter() {
                Some(g) => g,
                None => return 0.0,
            };
            let meter_res: windows::core::Result<IAudioMeterInformation> = (|| unsafe {
                let dev = get_default_endpoint(eRender)?;
                dev.Activate(CLSCTX_ALL, None)
            })();
            match meter_res {
                Ok(meter) => {
                    let peak = unsafe { meter.GetPeakValue().unwrap_or(0.0) }.clamp(0.0, 1.0);
                    *cache = Some((meter, com));
                    peak
                }
                Err(_) => 0.0,
            }
        })
    }

    pub fn set_endpoint_volume(is_mic: bool, pct: u8) -> anyhow::Result<()> {
        let _com = ComGuard::enter().context("COM init failed")?;
        let flow = if is_mic { eCapture } else { eRender };
        let dev = get_default_endpoint(flow).context("No default audio endpoint")?;
        let vol: IAudioEndpointVolume = unsafe {
            dev.Activate(CLSCTX_ALL, None)
                .context("IAudioEndpointVolume activation failed")?
        };
        let scalar = (pct.min(100) as f32) / 100.0;
        unsafe {
            vol.SetMasterVolumeLevelScalar(scalar, std::ptr::null())
                .context("SetMasterVolumeLevelScalar failed")?;
        }
        Ok(())
    }

    pub fn set_endpoint_mute(is_mic: bool, muted: bool) -> anyhow::Result<()> {
        let _com = ComGuard::enter().context("COM init failed")?;
        let flow = if is_mic { eCapture } else { eRender };
        let dev = get_default_endpoint(flow).context("No default audio endpoint")?;
        let vol: IAudioEndpointVolume = unsafe {
            dev.Activate(CLSCTX_ALL, None)
                .context("IAudioEndpointVolume activation failed")?
        };
        unsafe {
            vol.SetMute(muted, std::ptr::null())
                .context("SetMute failed")?;
        }
        Ok(())
    }
}

pub fn read_audio_state() -> SystemAudioState {
    #[cfg(windows)]
    return win_audio::read_audio_state();
    #[cfg(not(windows))]
    SystemAudioState {
        available: false,
        output_volume_pct: 0,
        output_muted: false,
        output_peak: 0.0,
        mic_available: false,
        mic_volume_pct: 0,
        mic_muted: false,
        mic_peak: 0.0,
    }
}

pub fn read_output_peak() -> f32 {
    #[cfg(windows)]
    return win_audio::read_output_peak();
    #[cfg(not(windows))]
    0.0
}

pub fn set_endpoint_volume(is_mic: bool, pct: u8) -> anyhow::Result<()> {
    #[cfg(windows)]
    return win_audio::set_endpoint_volume(is_mic, pct);
    #[cfg(not(windows))]
    {
        let _ = (is_mic, pct);
        anyhow::bail!("Non supporté")
    }
}

pub fn set_endpoint_mute(is_mic: bool, muted: bool) -> anyhow::Result<()> {
    #[cfg(windows)]
    return win_audio::set_endpoint_mute(is_mic, muted);
    #[cfg(not(windows))]
    {
        let _ = (is_mic, muted);
        anyhow::bail!("Non supporté")
    }
}
