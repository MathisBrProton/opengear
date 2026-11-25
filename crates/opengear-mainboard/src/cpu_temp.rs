//! Native CPU package temperature through PawnIO (AMD SMN `Tctl/Tdie`, Intel MSR).

#[cfg(windows)]
mod imp {
    use opengear_platform::PawnIoAvailability;
    use opengear_platform::named_mutex::{NamedMutexGuard, PCI_MUTEX};
    use opengear_platform::pawnio::{PawnIoSession, pawnio_availability};
    use std::sync::{Mutex, OnceLock};

    enum CachedCpuTemp {
        Amd(PawnIoSession),
        Intel(PawnIoSession),
    }

    static CPU_TEMP_CACHE: OnceLock<Mutex<Option<CachedCpuTemp>>> = OnceLock::new();

    fn read_amd_smn_temp(sess: &PawnIoSession) -> Option<(f32, &'static str)> {
        let _lock = NamedMutexGuard::acquire(PCI_MUTEX, 500)?;
        let out = sess.execute("ioctl_read_smn", &[0x0005_9800], 1).ok()?;
        let raw = *out.first()?;
        let v = raw as u32;
        let range_sel = (v & 0x0008_0000) != 0;
        let steps = (v >> 21) & 0x7FF;
        let mut temp = (steps as f32) * 0.125;
        if range_sel {
            temp -= 49.0;
        }
        if (5.0..=125.0).contains(&temp) {
            Some((temp, "PawnIO AMD Ryzen Tctl/Tdie"))
        } else {
            None
        }
    }

    fn read_intel_msr_temp(sess: &PawnIoSession) -> Option<(f32, &'static str)> {
        let tj_max = sess
            .execute("ioctl_read_msr", &[0x1A2], 1)
            .ok()
            .and_then(|v| v.first().copied())
            .map(|r| ((r >> 16) & 0xFF) as f32)
            .filter(|&t| (70.0..=115.0).contains(&t))
            .unwrap_or(100.0);
        let therm = sess
            .execute("ioctl_read_msr", &[0x1B1], 1)
            .ok()
            .or_else(|| sess.execute("ioctl_read_msr", &[0x19C], 1).ok())
            .and_then(|v| v.first().copied())?;
        let valid = (therm & (1 << 31)) != 0;
        if !valid {
            return None;
        }
        let delta = ((therm >> 16) & 0x7F) as f32;
        let temp = tj_max - delta;
        if (5.0..=125.0).contains(&temp) {
            Some((temp, "PawnIO Intel Package Temp"))
        } else {
            None
        }
    }

    pub fn read_native_cpu_temp() -> Option<(f32, &'static str)> {
        if pawnio_availability() != PawnIoAvailability::Ready {
            return None;
        }
        let cache = CPU_TEMP_CACHE.get_or_init(|| Mutex::new(None));
        let mut guard = cache.lock().ok()?;

        if let Some(cached) = guard.as_ref() {
            let res = match cached {
                CachedCpuTemp::Amd(s) => read_amd_smn_temp(s),
                CachedCpuTemp::Intel(s) => read_intel_msr_temp(s),
            };
            if res.is_some() {
                return res;
            }
            *guard = None;
        }

        if let Ok(sess) = PawnIoSession::load_file("AMDFamily17.bin")
            && let Some(res) = read_amd_smn_temp(&sess)
        {
            *guard = Some(CachedCpuTemp::Amd(sess));
            return Some(res);
        }
        if let Ok(sess) = PawnIoSession::load_file("IntelMSR.bin")
            && let Some(res) = read_intel_msr_temp(&sess)
        {
            *guard = Some(CachedCpuTemp::Intel(sess));
            return Some(res);
        }
        None
    }
}

/// `(°C, source)` of the CPU package, `None` without PawnIO / admin rights.
pub fn read_native_cpu_temp() -> Option<(f32, &'static str)> {
    #[cfg(windows)]
    {
        imp::read_native_cpu_temp()
    }
    #[cfg(not(windows))]
    None
}
