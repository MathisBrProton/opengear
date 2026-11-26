//! Fan / pump model shared by every cooling backend: headers, curves, temperature sources and
//! the safety floors no backend may go below.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanHeaderKind {
    CpuFan,
    AioPump,
    ChassisFan1,
    ChassisFan2,
    ChassisFan3,
}

impl FanHeaderKind {
    pub const ALL: &'static [Self] = &[
        Self::AioPump,
        Self::CpuFan,
        Self::ChassisFan1,
        Self::ChassisFan2,
        Self::ChassisFan3,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::CpuFan => "cpu_fan",
            Self::AioPump => "aio_pump",
            Self::ChassisFan1 => "cha_fan1",
            Self::ChassisFan2 => "cha_fan2",
            Self::ChassisFan3 => "cha_fan3",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CpuFan => "CPU_FAN (Ventilateur CPU / Radiateur)",
            Self::AioPump => "AIO_PUMP (Pompe Watercooling)",
            Self::ChassisFan1 => "CHA_FAN1 (Boîtier #1)",
            Self::ChassisFan2 => "CHA_FAN2 (Boîtier #2)",
            Self::ChassisFan3 => "CHA_FAN3 (Boîtier #3)",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "cpu_fan" => Some(Self::CpuFan),
            "aio_pump" => Some(Self::AioPump),
            "cha_fan1" => Some(Self::ChassisFan1),
            "cha_fan2" => Some(Self::ChassisFan2),
            "cha_fan3" => Some(Self::ChassisFan3),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanPolicyMode {
    Manual,
    Silent,
    Standard,
    Turbo,
    FullSpeed,
}

impl FanPolicyMode {
    /// Short user-facing name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manuel",
            Self::Silent => "Silencieux",
            Self::Standard => "Standard",
            Self::Turbo => "Turbo",
            Self::FullSpeed => "Pleine vitesse",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Silent => "silent",
            Self::Standard => "standard",
            Self::Turbo => "turbo",
            Self::FullSpeed => "full_speed",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "silent" => Self::Silent,
            "standard" => Self::Standard,
            "turbo" => Self::Turbo,
            "full_speed" => Self::FullSpeed,
            _ => Self::Manual,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: u8,
    pub duty_pct: u8,
}

/// Thermal sensor driving a fan channel when OpenGear Smart Cooling is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FanTempSource {
    /// Hardware BIOS curve handles it natively (0 background polling needed).
    #[default]
    CpuBios,
    /// Driven dynamically by AMD ADL2 / NVIDIA NVML GPU Edge temperature.
    GpuEdge,
    /// Driven dynamically by AMD ADL2 GPU Hotspot temperature.
    GpuHotspot,
    /// Driven dynamically by `max(CPU, GPU Edge)` — ideal for radiator / case airflow!
    MaxCpuGpu,
    /// Driven dynamically by `avg(CPU, GPU Edge)` — smooth balanced airflow.
    AvgCpuGpu,
    /// Driven dynamically by the hottest NVMe SSD sensor.
    Nvme,
    /// Driven dynamically by `max(CPU, GPU Edge, NVMe)`.
    MaxAll,
    /// Driven dynamically by a user-defined composed formula (`TempMixConfig`).
    CustomMix,
}

impl FanTempSource {
    pub fn id(self) -> &'static str {
        match self {
            Self::CpuBios => "cpu_bios",
            Self::GpuEdge => "gpu_edge",
            Self::GpuHotspot => "gpu_hotspot",
            Self::MaxCpuGpu => "max_cpu_gpu",
            Self::AvgCpuGpu => "avg_cpu_gpu",
            Self::Nvme => "nvme",
            Self::MaxAll => "max_all",
            Self::CustomMix => "custom_mix",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "gpu_edge" => Self::GpuEdge,
            "gpu_hotspot" => Self::GpuHotspot,
            "max_cpu_gpu" => Self::MaxCpuGpu,
            "avg_cpu_gpu" => Self::AvgCpuGpu,
            "nvme" => Self::Nvme,
            "max_all" => Self::MaxAll,
            "custom_mix" => Self::CustomMix,
            _ => Self::CpuBios,
        }
    }
}

/// Individual live sensor inputs available for composed virtual temperature sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SensorKind {
    #[default]
    Cpu,
    GpuEdge,
    GpuHotspot,
    Nvme,
    Motherboard,
}

/// Operation combining two sensor readings into a composed virtual temperature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TempMixOp {
    #[default]
    Max,
    Avg,
    Delta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempMixConfig {
    pub op: TempMixOp,
    pub a: SensorKind,
    pub b: SensorKind,
}

impl Default for TempMixConfig {
    fn default() -> Self {
        Self {
            op: TempMixOp::Max,
            a: SensorKind::Cpu,
            b: SensorKind::GpuEdge,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SensorReadings {
    pub cpu: Option<f32>,
    pub gpu_edge: Option<f32>,
    pub gpu_hotspot: Option<f32>,
    pub nvme: Option<f32>,
    pub motherboard: Option<f32>,
}

impl SensorReadings {
    pub fn get(&self, kind: SensorKind) -> Option<f32> {
        let v = match kind {
            SensorKind::Cpu => self.cpu,
            SensorKind::GpuEdge => self.gpu_edge,
            SensorKind::GpuHotspot => self.gpu_hotspot,
            SensorKind::Nvme => self.nvme,
            SensorKind::Motherboard => self.motherboard,
        };
        v.filter(|t| t.is_finite())
    }
}

/// Resolve the effective temperature (°C) for `source` and optional `custom_mix`.
pub fn resolve_temp_reading(
    source: FanTempSource,
    custom_mix: Option<&TempMixConfig>,
    r: &SensorReadings,
) -> Option<f32> {
    match source {
        FanTempSource::CpuBios => None,
        FanTempSource::GpuEdge => r.gpu_edge.or(r.gpu_hotspot),
        FanTempSource::GpuHotspot => r.gpu_hotspot.or(r.gpu_edge),
        FanTempSource::Nvme => r.nvme,
        FanTempSource::MaxCpuGpu => match (r.cpu, r.gpu_edge) {
            (Some(c), Some(g)) => Some(c.max(g)),
            (c, g) => c.or(g),
        },
        FanTempSource::AvgCpuGpu => match (r.cpu, r.gpu_edge) {
            (Some(c), Some(g)) => Some((c + g) * 0.5),
            (c, g) => c.or(g),
        },
        FanTempSource::MaxAll => [r.cpu, r.gpu_edge, r.nvme]
            .into_iter()
            .flatten()
            .filter(|t| t.is_finite())
            .reduce(f32::max),
        FanTempSource::CustomMix => {
            let mix = custom_mix.copied().unwrap_or_default();
            let va = r.get(mix.a);
            let vb = r.get(mix.b);
            match mix.op {
                TempMixOp::Max => match (va, vb) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                },
                TempMixOp::Avg => match (va, vb) {
                    (Some(a), Some(b)) => Some((a + b) * 0.5),
                    (a, b) => a.or(b),
                },
                TempMixOp::Delta => match (va, vb) {
                    (Some(a), Some(b)) => Some((a - b).abs().clamp(0.0, 120.0)),
                    _ => None,
                },
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanChannelConfig {
    pub header: FanHeaderKind,
    pub policy: FanPolicyMode,
    pub curve: [CurvePoint; 8],
    #[serde(default)]
    pub source: FanTempSource,
    #[serde(default = "default_hysteresis")]
    pub hysteresis_c: u8,
    /// Exponential temperature smoothing time constant in seconds (0 = instant, no smoothing).
    #[serde(default)]
    pub smoothing_secs: u8,
    #[serde(default)]
    pub custom_mix: Option<TempMixConfig>,
}

fn default_hysteresis() -> u8 {
    3
}

impl FanHeaderKind {
    /// Lowest PWM duty OpenGear will ever send to this header.
    ///
    /// An AIO pump must never stop (coolant flow / airlocks) and the CPU fan header usually cools
    /// the CPU radiator or heatsink; chassis headers may legitimately use fan-stop (0 %).
    pub fn min_safe_duty_pct(self) -> u8 {
        match self {
            FanHeaderKind::AioPump => 45,
            FanHeaderKind::CpuFan => 20,
            _ => 0,
        }
    }
}

impl FanChannelConfig {
    /// Return a hardware-safe copy of the 8-point curve:
    /// - points sorted by temperature, temperatures clamped to 20..=100 °C and strictly increasing,
    /// - duty clamped to `min_safe_duty_pct..=100` and monotonically non-decreasing.
    pub fn sanitized_curve(&self) -> [CurvePoint; 8] {
        let min_duty = self.header.min_safe_duty_pct();
        let mut pts = self.curve;
        pts.sort_by_key(|p| p.temp_c);
        let mut prev_temp: Option<u8> = None;
        let mut prev_duty = min_duty;
        for (i, p) in pts.iter_mut().enumerate() {
            let min_t = prev_temp.map(|pt| pt.saturating_add(1)).unwrap_or(20);
            let max_t = 100u8.saturating_sub((7 - i) as u8);
            let t = p.temp_c.clamp(min_t, max_t.max(min_t));
            p.temp_c = t;
            prev_temp = Some(t);
            p.duty_pct = p.duty_pct.clamp(min_duty, 100).max(prev_duty);
            prev_duty = p.duty_pct;
        }
        pts
    }

    /// Linearly interpolate target PWM duty (%) from the sanitized 8-point curve for `temp_c`.
    /// Non-finite sensor values fail safe to 100 % duty, and header safety floors are always enforced.
    pub fn interpolate_duty(&self, temp_c: f32) -> u8 {
        if !temp_c.is_finite() {
            return 100;
        }
        let min_duty = self.header.min_safe_duty_pct();
        let pts = self.sanitized_curve();
        if temp_c <= pts[0].temp_c as f32 {
            return pts[0].duty_pct.clamp(min_duty, 100);
        }
        for w in pts.windows(2) {
            let t0 = w[0].temp_c as f32;
            let t1 = w[1].temp_c as f32;
            if temp_c <= t1 && t1 > t0 {
                let f = ((temp_c - t0) / (t1 - t0)).clamp(0.0, 1.0);
                let d = w[0].duty_pct as f32 + (w[1].duty_pct as f32 - w[0].duty_pct as f32) * f;
                return (d.round() as i32).clamp(min_duty as i32, 100) as u8;
            }
        }
        pts[7].duty_pct.clamp(min_duty, 100)
    }

    pub fn default_for(header: FanHeaderKind) -> Self {
        let curve = match header {
            // AIO pump: keep minimum >= 45% for steady coolant flow & zero airlocks
            FanHeaderKind::AioPump => [
                CurvePoint {
                    temp_c: 30,
                    duty_pct: 55,
                },
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 58,
                },
                CurvePoint {
                    temp_c: 50,
                    duty_pct: 64,
                },
                CurvePoint {
                    temp_c: 60,
                    duty_pct: 72,
                },
                CurvePoint {
                    temp_c: 68,
                    duty_pct: 82,
                },
                CurvePoint {
                    temp_c: 75,
                    duty_pct: 92,
                },
                CurvePoint {
                    temp_c: 82,
                    duty_pct: 100,
                },
                CurvePoint {
                    temp_c: 90,
                    duty_pct: 100,
                },
            ],
            FanHeaderKind::CpuFan => [
                CurvePoint {
                    temp_c: 30,
                    duty_pct: 25,
                },
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 32,
                },
                CurvePoint {
                    temp_c: 50,
                    duty_pct: 42,
                },
                CurvePoint {
                    temp_c: 60,
                    duty_pct: 55,
                },
                CurvePoint {
                    temp_c: 70,
                    duty_pct: 72,
                },
                CurvePoint {
                    temp_c: 78,
                    duty_pct: 88,
                },
                CurvePoint {
                    temp_c: 85,
                    duty_pct: 100,
                },
                CurvePoint {
                    temp_c: 92,
                    duty_pct: 100,
                },
            ],
            _ => [
                CurvePoint {
                    temp_c: 30,
                    duty_pct: 25,
                },
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 30,
                },
                CurvePoint {
                    temp_c: 50,
                    duty_pct: 40,
                },
                CurvePoint {
                    temp_c: 60,
                    duty_pct: 50,
                },
                CurvePoint {
                    temp_c: 70,
                    duty_pct: 65,
                },
                CurvePoint {
                    temp_c: 78,
                    duty_pct: 80,
                },
                CurvePoint {
                    temp_c: 85,
                    duty_pct: 95,
                },
                CurvePoint {
                    temp_c: 92,
                    duty_pct: 100,
                },
            ],
        };
        Self {
            header,
            policy: FanPolicyMode::Manual,
            curve,
            source: FanTempSource::CpuBios,
            hysteresis_c: 3,
            smoothing_secs: 0,
            custom_mix: None,
        }
    }

    /// Preset curves tuned for 4-pin PWM AIO pumps (minimum 45% floor enforced).
    pub fn silent_loop3_preset(preset: &str) -> [CurvePoint; 8] {
        match preset {
            "ultra_silent" => [
                CurvePoint {
                    temp_c: 30,
                    duty_pct: 45,
                },
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 45,
                },
                CurvePoint {
                    temp_c: 50,
                    duty_pct: 50,
                },
                CurvePoint {
                    temp_c: 60,
                    duty_pct: 58,
                },
                CurvePoint {
                    temp_c: 70,
                    duty_pct: 70,
                },
                CurvePoint {
                    temp_c: 78,
                    duty_pct: 82,
                },
                CurvePoint {
                    temp_c: 85,
                    duty_pct: 95,
                },
                CurvePoint {
                    temp_c: 92,
                    duty_pct: 100,
                },
            ],
            "max_perf" => [
                CurvePoint {
                    temp_c: 30,
                    duty_pct: 75,
                },
                CurvePoint {
                    temp_c: 40,
                    duty_pct: 80,
                },
                CurvePoint {
                    temp_c: 50,
                    duty_pct: 85,
                },
                CurvePoint {
                    temp_c: 60,
                    duty_pct: 92,
                },
                CurvePoint {
                    temp_c: 68,
                    duty_pct: 100,
                },
                CurvePoint {
                    temp_c: 75,
                    duty_pct: 100,
                },
                CurvePoint {
                    temp_c: 82,
                    duty_pct: 100,
                },
                CurvePoint {
                    temp_c: 90,
                    duty_pct: 100,
                },
            ],
            _ => Self::default_for(FanHeaderKind::AioPump).curve,
        }
    }
}

/// Stateful per-channel temperature smoothing + asymmetric hysteresis tracker for software curves.
#[derive(Debug, Clone, Copy)]
pub struct FanCurveTracker {
    smoothed_temp: Option<f32>,
    ref_temp: f32,
    last_duty: Option<u8>,
}

impl Default for FanCurveTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FanCurveTracker {
    pub fn new() -> Self {
        Self {
            smoothed_temp: None,
            ref_temp: 40.0,
            last_duty: None,
        }
    }

    pub fn reset(&mut self) {
        self.smoothed_temp = None;
        self.ref_temp = 40.0;
        self.last_duty = None;
    }

    pub fn last_duty(&self) -> Option<u8> {
        self.last_duty
    }

    pub fn mark_applied(&mut self, duty: u8) {
        self.last_duty = Some(duty);
    }

    /// Evaluate the next target duty (%) for `cfg` given a sensor reading (`None` -> failsafe 100 %),
    /// elapsed step `dt_secs`, and minimum duty deadband `deadband_pct`.
    /// Returns `Some(target_duty)` only when the target differs enough to warrant a hardware write.
    pub fn evaluate(
        &mut self,
        cfg: &FanChannelConfig,
        measured: Option<f32>,
        dt_secs: f32,
        deadband_pct: u8,
    ) -> Option<u8> {
        let floor = cfg.header.min_safe_duty_pct();
        let raw_t = match measured {
            Some(t) if t.is_finite() => t,
            _ => {
                // Sensor absent / NaN: bypass smoothing and fail safe immediately.
                self.smoothed_temp = None;
                self.ref_temp = 100.0;
                let target = cfg.interpolate_duty(100.0).max(floor);
                let changed = self
                    .last_duty
                    .is_none_or(|prev| prev.abs_diff(target) >= deadband_pct);
                return changed.then_some(target);
            }
        };

        let smooth_t = if cfg.smoothing_secs == 0 || !dt_secs.is_finite() || dt_secs <= 0.0 {
            raw_t
        } else {
            match self.smoothed_temp {
                Some(prev) => {
                    let tau = cfg.smoothing_secs as f32;
                    let alpha = (dt_secs / (tau + dt_secs)).clamp(0.05, 1.0);
                    prev + alpha * (raw_t - prev)
                }
                None => raw_t,
            }
        };
        self.smoothed_temp = Some(smooth_t);

        let hyst = cfg.hysteresis_c as f32;
        // Rising: follow immediately. Falling: wait until below the hysteresis band.
        let eff_t = if smooth_t >= self.ref_temp || smooth_t <= self.ref_temp - hyst {
            self.ref_temp = smooth_t;
            smooth_t
        } else {
            self.ref_temp
        };

        let target = cfg.interpolate_duty(eff_t).max(floor);
        let changed = self
            .last_duty
            .is_none_or(|prev| prev.abs_diff(target) >= deadband_pct);
        changed.then_some(target)
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;

    fn cfg_with(header: FanHeaderKind, pts: [(u8, u8); 8]) -> FanChannelConfig {
        let mut c = FanChannelConfig::default_for(header);
        for (i, (t, d)) in pts.into_iter().enumerate() {
            c.curve[i] = CurvePoint {
                temp_c: t,
                duty_pct: d,
            };
        }
        c
    }

    #[test]
    fn pump_curve_never_drops_below_floor() {
        let c = cfg_with(FanHeaderKind::AioPump, [(30, 0); 8]);
        assert!(c.sanitized_curve().iter().all(|p| p.duty_pct >= 45));
        assert!(c.interpolate_duty(10.0) >= 45);
        assert!(c.interpolate_duty(55.0) >= 45);
    }

    #[test]
    fn interpolate_enforces_floor_and_nan_failsafe() {
        let mut c = cfg_with(FanHeaderKind::CpuFan, [(30, 0); 8]);
        c.curve[0] = CurvePoint {
            temp_c: 30,
            duty_pct: 5,
        };
        assert!(c.interpolate_duty(25.0) >= 20);
        assert_eq!(c.interpolate_duty(f32::NAN), 100);
        assert_eq!(c.interpolate_duty(f32::INFINITY), 100);
    }

    #[test]
    fn cpu_fan_floor_and_chassis_fan_stop_allowed() {
        let cpu = cfg_with(FanHeaderKind::CpuFan, [(30, 0); 8]);
        assert!(cpu.sanitized_curve().iter().all(|p| p.duty_pct >= 20));
        let cha = cfg_with(FanHeaderKind::ChassisFan1, [(30, 0); 8]);
        assert_eq!(cha.sanitized_curve()[0].duty_pct, 0);
    }

    #[test]
    fn curve_is_sorted_strictly_increasing_even_at_100c() {
        let c = cfg_with(
            FanHeaderKind::ChassisFan1,
            [
                (100, 10),
                (100, 20),
                (100, 30),
                (100, 40),
                (100, 50),
                (100, 60),
                (100, 70),
                (100, 80),
            ],
        );
        let s = c.sanitized_curve();
        for w in s.windows(2) {
            assert!(
                w[1].temp_c > w[0].temp_c,
                "temps must strictly increase: {s:?}"
            );
            assert!(
                w[1].duty_pct >= w[0].duty_pct,
                "duty must not decrease: {s:?}"
            );
        }
        assert_eq!(s[7].temp_c, 100);
    }

    #[test]
    fn curve_is_sorted_strictly_increasing_and_monotonic() {
        let c = cfg_with(
            FanHeaderKind::ChassisFan1,
            [
                (90, 100),
                (30, 40),
                (60, 20),
                (60, 70),
                (10, 10),
                (200, 50),
                (45, 30),
                (75, 90),
            ],
        );
        let s = c.sanitized_curve();
        for w in s.windows(2) {
            assert!(
                w[1].temp_c > w[0].temp_c,
                "temps must strictly increase: {s:?}"
            );
            assert!(
                w[1].duty_pct >= w[0].duty_pct,
                "duty must not decrease: {s:?}"
            );
        }
        assert!(
            s.iter()
                .all(|p| (20..=100).contains(&p.temp_c) && p.duty_pct <= 100)
        );
    }

    #[test]
    fn tracker_smooths_spikes_and_respects_hysteresis_and_failsafe() {
        let mut cfg = FanChannelConfig::default_for(FanHeaderKind::CpuFan);
        cfg.hysteresis_c = 4;
        cfg.smoothing_secs = 9;
        let mut tr = FanCurveTracker::new();

        // First sample seeds the filter at 40 C.
        let d0 = tr.evaluate(&cfg, Some(40.0), 3.0, 3).unwrap();
        tr.mark_applied(d0);
        // Short 80 C spike is dampened by EMA (tau=9s, dt=3s -> alpha=0.25 -> 50 C, not 80 C).
        let d_spike = tr.evaluate(&cfg, Some(80.0), 3.0, 3).unwrap();
        assert!(
            d_spike < cfg.interpolate_duty(80.0),
            "spike should be smoothed: got {d_spike}"
        );
        tr.mark_applied(d_spike);

        // Small drop inside hysteresis band keeps reference temp held.
        let d_drop = tr.evaluate(&cfg, Some(48.0), 3.0, 1);
        assert_eq!(
            d_drop, None,
            "drop inside hysteresis band must not change duty"
        );

        // Missing sensor fails safe to 100 % immediately, bypassing smoothing.
        let d_fail = tr.evaluate(&cfg, None, 3.0, 1).unwrap();
        assert_eq!(d_fail, 100);
    }

    #[test]
    fn composed_temp_sources_compute_max_avg_delta_and_nvme() {
        let r = SensorReadings {
            cpu: Some(62.0),
            gpu_edge: Some(50.0),
            gpu_hotspot: Some(68.0),
            nvme: Some(44.0),
            motherboard: Some(36.0),
        };
        assert_eq!(
            resolve_temp_reading(FanTempSource::Nvme, None, &r),
            Some(44.0)
        );
        assert_eq!(
            resolve_temp_reading(FanTempSource::AvgCpuGpu, None, &r),
            Some(56.0)
        );
        assert_eq!(
            resolve_temp_reading(FanTempSource::MaxAll, None, &r),
            Some(62.0)
        );
        let delta = TempMixConfig {
            op: TempMixOp::Delta,
            a: SensorKind::Cpu,
            b: SensorKind::Motherboard,
        };
        assert_eq!(
            resolve_temp_reading(FanTempSource::CustomMix, Some(&delta), &r),
            Some(26.0)
        );
        let missing_delta = SensorReadings {
            cpu: Some(60.0),
            ..Default::default()
        };
        assert_eq!(
            resolve_temp_reading(FanTempSource::CustomMix, Some(&delta), &missing_delta),
            None
        );
    }
}
