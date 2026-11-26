use serde::Serialize;
use std::f32::consts::TAU;

pub const FFT_LEN: usize = 512;

/// Real-time system render spectrum (peak + 8 logarithmic frequency bands in `[0.0, 1.0]`).
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioSpectrum {
    pub peak: f32,
    pub bands: [f32; 8],
}

/// In-place iterative Cooley-Tukey radix-2 FFT over `FFT_LEN` complex samples.
fn fft_radix2_in_place(re: &mut [f32; FFT_LEN], im: &mut [f32; FFT_LEN]) {
    let mut j = 0usize;
    for i in 1..FFT_LEN {
        let mut bit = FFT_LEN >> 1;
        while (j & bit) != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2usize;
    while len <= FFT_LEN {
        let half = len / 2;
        let angle_step = -TAU / (len as f32);
        let (w_sin_step, w_cos_step) = angle_step.sin_cos();
        for start in (0..FFT_LEN).step_by(len) {
            let mut wr = 1.0f32;
            let mut wi = 0.0f32;
            for k in 0..half {
                let even = start + k;
                let odd = even + half;
                let tr = wr * re[odd] - wi * im[odd];
                let ti = wr * im[odd] + wi * re[odd];
                re[odd] = re[even] - tr;
                im[odd] = im[even] - ti;
                re[even] += tr;
                im[even] += ti;
                let next_wr = wr * w_cos_step - wi * w_sin_step;
                wi = wr * w_sin_step + wi * w_cos_step;
                wr = next_wr;
            }
        }
        len <<= 1;
    }
}

/// Computes 8 normalized frequency band energies (`20 Hz..16 kHz`) from mono PCM samples.
pub fn compute_fft_bands(samples: &[f32], sample_rate: u32) -> [f32; 8] {
    if samples.is_empty() || sample_rate == 0 {
        return [0.0; 8];
    }
    let slice = if samples.len() > FFT_LEN {
        &samples[samples.len() - FFT_LEN..]
    } else {
        samples
    };
    let n = slice.len();
    let mean = slice.iter().copied().sum::<f32>() / (n as f32);
    let offset = FFT_LEN - n;

    let mut re = [0.0f32; FFT_LEN];
    let mut im = [0.0f32; FFT_LEN];
    let denom = (n.saturating_sub(1)).max(1) as f32;
    for (idx, &s) in slice.iter().enumerate() {
        let w = 0.5 * (1.0 - (TAU * (idx as f32) / denom).cos());
        re[offset + idx] = (s - mean) * w;
    }

    fft_radix2_in_place(&mut re, &mut im);

    const EDGES_HZ: [f32; 9] = [
        20.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
    ];
    let nyquist_bin = FFT_LEN / 2 - 1;
    let bin_hz = sample_rate as f32 / FFT_LEN as f32;
    let scale = 2.0 / (n as f32 * 0.5).max(1.0);

    let mut bands = [0.0f32; 8];
    for b in 0..8 {
        let lo = ((EDGES_HZ[b] / bin_hz).floor() as usize).clamp(1, nyquist_bin);
        let hi = ((EDGES_HZ[b + 1] / bin_hz).ceil() as usize).clamp(lo, nyquist_bin);
        let mut sum_sq = 0.0f32;
        let mut peak = 0.0f32;
        let count = (hi - lo + 1) as f32;
        for k in lo..=hi {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() * scale;
            sum_sq += mag * mag;
            if mag > peak {
                peak = mag;
            }
        }
        let rms = (sum_sq / count).sqrt();
        // Blend RMS and peak so both sustained synths and sharp kicks pop visually.
        let energy = (rms * 0.45 + peak * 0.55) * 2.4;
        bands[b] = energy.sqrt().clamp(0.0, 1.0);
    }
    bands
}

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

    struct TaskMemGuard(*mut windows::Win32::Media::Audio::WAVEFORMATEX);

    impl Drop for TaskMemGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    windows::Win32::System::Com::CoTaskMemFree(Some(self.0 as *const _));
                }
            }
        }
    }

    struct WorkerResetGuard;
    impl Drop for WorkerResetGuard {
        fn drop(&mut self) {
            LOOPBACK_RUNNING.store(false, std::sync::atomic::Ordering::Release);
        }
    }

    static LOOPBACK_SPECTRUM: std::sync::OnceLock<std::sync::Mutex<super::AudioSpectrum>> =
        std::sync::OnceLock::new();
    static LOOPBACK_RUNNING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    static LOOPBACK_ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    static LOOPBACK_LAST_REQ_MS: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);

    fn spectrum_store() -> &'static std::sync::Mutex<super::AudioSpectrum> {
        LOOPBACK_SPECTRUM.get_or_init(|| std::sync::Mutex::new(super::AudioSpectrum::default()))
    }

    fn elapsed_ms_since_origin() -> u64 {
        let origin = LOOPBACK_ORIGIN.get_or_init(std::time::Instant::now);
        origin.elapsed().as_millis() as u64
    }

    pub fn read_audio_spectrum() -> super::AudioSpectrum {
        use std::sync::atomic::Ordering;

        LOOPBACK_LAST_REQ_MS.store(elapsed_ms_since_origin(), Ordering::Relaxed);
        if LOOPBACK_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && std::thread::Builder::new()
                .name("opengear-wasapi-fft".into())
                .spawn(run_loopback_worker)
                .is_err()
        {
            LOOPBACK_RUNNING.store(false, Ordering::Release);
        }

        let spec = spectrum_store().lock().map(|g| *g).unwrap_or_default();
        if spec.peak > 0.0 || spec.bands.iter().any(|&b| b > 0.0) {
            spec
        } else {
            let p = read_output_peak();
            super::AudioSpectrum {
                peak: p,
                bands: [p; 8],
            }
        }
    }

    fn run_loopback_worker() {
        use std::sync::atomic::Ordering;
        use windows::Win32::Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
            IAudioCaptureClient, IAudioClient, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
        };
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

        let _reset = WorkerResetGuard;
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let should_uninit = hr.is_ok();
        struct MultiComGuard(bool);
        impl Drop for MultiComGuard {
            fn drop(&mut self) {
                if self.0 {
                    unsafe { CoUninitialize() };
                }
            }
        }
        let _com = MultiComGuard(should_uninit);
        if hr.is_err() && hr != RPC_E_CHANGED_MODE {
            return;
        }

        let Ok(dev) = get_default_endpoint(eRender) else {
            return;
        };
        let Ok(client): windows::core::Result<IAudioClient> =
            (unsafe { dev.Activate(CLSCTX_ALL, None) })
        else {
            return;
        };
        let Ok(mix_ptr) = (unsafe { client.GetMixFormat() }) else {
            return;
        };
        let _mix_guard = TaskMemGuard(mix_ptr);
        if mix_ptr.is_null() {
            return;
        }
        let fmt: WAVEFORMATEX = unsafe { *mix_ptr };
        let channels = (fmt.nChannels as usize).max(1);
        let sample_rate = fmt.nSamplesPerSec.max(8_000);
        let bits = fmt.wBitsPerSample;
        let is_float = if fmt.wFormatTag == 3 {
            true
        } else if fmt.wFormatTag == 0xFFFE {
            let ext: WAVEFORMATEXTENSIBLE = unsafe { *(mix_ptr as *const WAVEFORMATEXTENSIBLE) };
            ext.SubFormat.data1 == 3
        } else {
            false
        };

        // 25 ms loopback ring buffer in 100-ns units
        if unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                250_000,
                0,
                mix_ptr,
                None,
            )
        }
        .is_err()
        {
            return;
        }
        let Ok(capture): windows::core::Result<IAudioCaptureClient> =
            (unsafe { client.GetService() })
        else {
            return;
        };
        if unsafe { client.Start() }.is_err() {
            return;
        }

        let mut ring = std::collections::VecDeque::with_capacity(super::FFT_LEN * 2);
        ring.resize(super::FFT_LEN, 0.0f32);
        let mut smoothed_bands = [0.0f32; 8];
        let mut smoothed_peak = 0.0f32;

        loop {
            let last_req = LOOPBACK_LAST_REQ_MS.load(Ordering::Relaxed);
            if elapsed_ms_since_origin().saturating_sub(last_req) > 2500 {
                break;
            }

            let mut updated = false;
            loop {
                let Ok(packet_frames) = (unsafe { capture.GetNextPacketSize() }) else {
                    break;
                };
                if packet_frames == 0 {
                    break;
                }
                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                let mut frames_read: u32 = 0;
                let mut flags: u32 = 0;
                if unsafe {
                    capture.GetBuffer(&mut data_ptr, &mut frames_read, &mut flags, None, None)
                }
                .is_err()
                {
                    break;
                }

                let n_frames = frames_read as usize;
                let silent =
                    (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data_ptr.is_null();
                if n_frames > 0 {
                    updated = true;
                    if silent {
                        for _ in 0..n_frames {
                            ring.pop_front();
                            ring.push_back(0.0);
                        }
                    } else if is_float && bits == 32 {
                        let floats = unsafe {
                            std::slice::from_raw_parts(data_ptr as *const f32, n_frames * channels)
                        };
                        for ch in floats.chunks_exact(channels) {
                            let mono = ch.iter().copied().sum::<f32>() / (channels as f32);
                            ring.pop_front();
                            ring.push_back(mono.clamp(-1.0, 1.0));
                        }
                    } else if !is_float && bits == 16 {
                        let pcm = unsafe {
                            std::slice::from_raw_parts(data_ptr as *const i16, n_frames * channels)
                        };
                        for ch in pcm.chunks_exact(channels) {
                            let mono = ch.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                / (channels as f32);
                            ring.pop_front();
                            ring.push_back(mono.clamp(-1.0, 1.0));
                        }
                    }
                }
                let _ = unsafe { capture.ReleaseBuffer(frames_read) };
            }

            if updated {
                let slice: Vec<f32> = ring.iter().copied().collect();
                let raw_bands = super::compute_fft_bands(&slice, sample_rate);
                let raw_peak = slice.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                smoothed_peak = if raw_peak > smoothed_peak {
                    smoothed_peak * 0.3 + raw_peak * 0.7
                } else {
                    smoothed_peak * 0.85
                };
                for i in 0..8 {
                    smoothed_bands[i] = if raw_bands[i] > smoothed_bands[i] {
                        smoothed_bands[i] * 0.25 + raw_bands[i] * 0.75
                    } else {
                        smoothed_bands[i] * 0.80
                    };
                }
            } else {
                smoothed_peak *= 0.88;
                for b in &mut smoothed_bands {
                    *b *= 0.85;
                }
            }

            if let Ok(mut guard) = spectrum_store().lock() {
                *guard = super::AudioSpectrum {
                    peak: smoothed_peak.clamp(0.0, 1.0),
                    bands: smoothed_bands,
                };
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }

        let _ = unsafe { client.Stop() };
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

/// Returns real-time system output peak + 8 frequency bands (`20 Hz..16 kHz`).
/// Spawns a lightweight WASAPI loopback capture worker on demand and stops automatically
/// when `AudioVisualizer` is idle.
pub fn read_audio_spectrum() -> AudioSpectrum {
    #[cfg(windows)]
    return win_audio::read_audio_spectrum();
    #[cfg(not(windows))]
    AudioSpectrum::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_distinguishes_bass_from_treble_sine() {
        let fs = 48_000u32;
        let bass: Vec<f32> = (0..FFT_LEN)
            .map(|i| (TAU * 120.0 * (i as f32) / (fs as f32)).sin())
            .collect();
        let bass_bands = compute_fft_bands(&bass, fs);
        assert!(
            bass_bands[0] > 0.5 && bass_bands[0] > bass_bands[6] * 3.0,
            "expected bass peak in band 0, got {bass_bands:?}"
        );

        let treble: Vec<f32> = (0..FFT_LEN)
            .map(|i| (TAU * 6000.0 * (i as f32) / (fs as f32)).sin())
            .collect();
        let treble_bands = compute_fft_bands(&treble, fs);
        assert!(
            treble_bands[6] > 0.5 && treble_bands[6] > treble_bands[0] * 3.0,
            "expected treble peak in band 6, got {treble_bands:?}"
        );
    }
}
