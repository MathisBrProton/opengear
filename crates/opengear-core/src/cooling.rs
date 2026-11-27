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
    /// Driven dynamically by the live CPU temperature sensor (for software curves).
    Cpu,
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
            Self::Cpu => "cpu",
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
            "cpu" | "Cpu" => Self::Cpu,
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
        FanTempSource::Cpu => r.cpu,
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
        self.evaluate_with_floor(
            cfg,
            cfg.header.min_safe_duty_pct(),
            measured,
            dt_secs,
            deadband_pct,
        )
    }

    /// Evaluate the next target duty (%) using an explicit safety floor `floor`.
    pub fn evaluate_with_floor(
        &mut self,
        cfg: &FanChannelConfig,
        floor: u8,
        measured: Option<f32>,
        dt_secs: f32,
        deadband_pct: u8,
    ) -> Option<u8> {
        let floor = floor.min(100);
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

/// Stable storage key for a specific fan on a specific device (`"{device_id}::{fan_id}"`).
pub fn fan_rule_key(device_id: &str, fan_id: &str) -> String {
    format!("{device_id}::{fan_id}")
}

/// Control mode for a universal software-driven fan channel (`Cooling` capability).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareFanMode {
    /// Firmware / BIOS manages this fan automatically.
    #[default]
    Auto,
    /// Fixed manual duty cycle (%).
    Fixed,
    /// Single 8-point software curve driven by any sensor or composed formula.
    Curve,
    /// Dual-curve Mix (e.g. max(CPU curve, GPU curve), like FanControl).
    Mix,
}

/// How two independent curves are combined in [`SoftwareFanMode::Mix`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CurveMixOp {
    #[default]
    Max,
    Avg,
}

fn default_fixed_pct() -> u8 {
    60
}

fn default_software_curve() -> [CurvePoint; 8] {
    FanChannelConfig::default_for(FanHeaderKind::ChassisFan1).curve
}

fn default_software_source() -> FanTempSource {
    FanTempSource::Cpu
}

fn default_mix_source() -> FanTempSource {
    FanTempSource::GpuEdge
}

/// Universal per-fan software cooling configuration stored per profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareFanRule {
    #[serde(default)]
    pub mode: SoftwareFanMode,
    #[serde(default = "default_fixed_pct")]
    pub fixed_pct: u8,
    #[serde(default = "default_software_source")]
    pub source: FanTempSource,
    #[serde(default)]
    pub custom_mix: Option<TempMixConfig>,
    #[serde(default = "default_software_curve")]
    pub curve: [CurvePoint; 8],
    #[serde(default = "default_hysteresis")]
    pub hysteresis_c: u8,
    #[serde(default)]
    pub smoothing_secs: u8,
    /// Maximum duty slew rate in %/s (0 = instant).
    #[serde(default)]
    pub ramp_pct_per_sec: u8,
    #[serde(default = "default_mix_source")]
    pub mix_source: FanTempSource,
    #[serde(default)]
    pub mix_custom: Option<TempMixConfig>,
    #[serde(default = "default_software_curve")]
    pub mix_curve: [CurvePoint; 8],
    #[serde(default)]
    pub mix_op: CurveMixOp,
}

impl Default for SoftwareFanRule {
    fn default() -> Self {
        Self {
            mode: SoftwareFanMode::Auto,
            fixed_pct: default_fixed_pct(),
            source: FanTempSource::Cpu,
            custom_mix: None,
            curve: default_software_curve(),
            hysteresis_c: default_hysteresis(),
            smoothing_secs: 2,
            ramp_pct_per_sec: 0,
            mix_source: FanTempSource::GpuEdge,
            mix_custom: None,
            mix_curve: default_software_curve(),
            mix_op: CurveMixOp::Max,
        }
    }
}

impl SoftwareFanRule {
    /// True when the background software cooling loop must evaluate and drive this fan.
    pub fn is_software_curve(&self) -> bool {
        matches!(self.mode, SoftwareFanMode::Curve | SoftwareFanMode::Mix)
    }

    fn channel_cfg(
        source: FanTempSource,
        custom_mix: Option<TempMixConfig>,
        curve: [CurvePoint; 8],
        hysteresis_c: u8,
        smoothing_secs: u8,
    ) -> FanChannelConfig {
        FanChannelConfig {
            header: FanHeaderKind::ChassisFan1,
            policy: FanPolicyMode::Manual,
            curve,
            source,
            hysteresis_c,
            smoothing_secs,
            custom_mix,
        }
    }
}

/// Measured PWM <-> RPM calibration profile for one physical fan header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FanCalibration {
    /// Lowest tested PWM % where RPM > 0 (fan actually spinning).
    #[serde(default)]
    pub min_spinning_pct: Option<u8>,
    /// True if at least one tested low PWM % produced 0 RPM while higher PWM % spun.
    #[serde(default)]
    pub zero_rpm_supported: bool,
    #[serde(default)]
    pub max_rpm: Option<u32>,
    /// `(duty_pct, measured_rpm)` samples ordered ascending by duty.
    #[serde(default)]
    pub samples: Vec<(u8, u32)>,
}

impl FanCalibration {
    pub fn from_samples(mut samples: Vec<(u8, u32)>) -> Self {
        samples.sort_by_key(|(d, _)| *d);
        samples.dedup_by_key(|(d, _)| *d);
        let min_spinning_pct = samples
            .iter()
            .filter(|(_, rpm)| *rpm > 0)
            .map(|(d, _)| *d)
            .min();
        let has_zero = samples.iter().any(|(_, rpm)| *rpm == 0);
        let max_rpm = samples.iter().map(|(_, rpm)| *rpm).max().filter(|&r| r > 0);
        Self {
            min_spinning_pct,
            zero_rpm_supported: has_zero && min_spinning_pct.is_some(),
            max_rpm,
            samples,
        }
    }
}

/// Stateful evaluator for [`SoftwareFanRule`] supporting single curves, dual-curve Mix, ramp rate
/// limiting, hardware safety floors, and immediate failsafe on missing sensors.
#[derive(Debug, Clone, Copy, Default)]
pub struct SoftwareFanTracker {
    primary: FanCurveTracker,
    secondary: FanCurveTracker,
    ramp_duty: Option<f32>,
    last_applied: Option<u8>,
}

impl SoftwareFanTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.primary.reset();
        self.secondary.reset();
        self.ramp_duty = None;
        self.last_applied = None;
    }

    pub fn last_applied(&self) -> Option<u8> {
        self.last_applied
    }

    pub fn mark_applied(&mut self, duty: u8) {
        self.last_applied = Some(duty);
        self.ramp_duty = Some(duty as f32);
        self.primary.mark_applied(duty);
        self.secondary.mark_applied(duty);
    }

    fn resolve_for_software(
        source: FanTempSource,
        custom_mix: Option<&TempMixConfig>,
        readings: &SensorReadings,
    ) -> Option<f32> {
        match source {
            // In software curve mode, `CpuBios` acts as live CPU temperature fallback.
            FanTempSource::CpuBios => readings.cpu,
            other => resolve_temp_reading(other, custom_mix, readings),
        }
    }

    /// Evaluate the next PWM duty (%) to send for `rule` respecting `min_floor`.
    /// Returns `Some(duty)` only when a hardware write is needed.
    pub fn evaluate(
        &mut self,
        rule: &SoftwareFanRule,
        min_floor: u8,
        readings: &SensorReadings,
        dt_secs: f32,
        deadband_pct: u8,
    ) -> Option<u8> {
        let floor = min_floor.min(100);
        let (desired, bypass_ramp) = match rule.mode {
            SoftwareFanMode::Auto => return None,
            SoftwareFanMode::Fixed => (rule.fixed_pct.clamp(floor, 100), true),
            SoftwareFanMode::Curve => {
                let cfg = SoftwareFanRule::channel_cfg(
                    rule.source,
                    rule.custom_mix,
                    rule.curve,
                    rule.hysteresis_c,
                    rule.smoothing_secs,
                );
                let measured =
                    Self::resolve_for_software(rule.source, rule.custom_mix.as_ref(), readings);
                if !measured.is_some_and(f32::is_finite) {
                    self.primary.reset();
                    (100, true)
                } else {
                    let next = self
                        .primary
                        .evaluate_with_floor(&cfg, floor, measured, dt_secs, 0)
                        .unwrap_or_else(|| self.primary.last_duty().unwrap_or(floor));
                    self.primary.mark_applied(next);
                    (next, false)
                }
            }
            SoftwareFanMode::Mix => {
                let cfg_a = SoftwareFanRule::channel_cfg(
                    rule.source,
                    rule.custom_mix,
                    rule.curve,
                    rule.hysteresis_c,
                    rule.smoothing_secs,
                );
                let cfg_b = SoftwareFanRule::channel_cfg(
                    rule.mix_source,
                    rule.mix_custom,
                    rule.mix_curve,
                    rule.hysteresis_c,
                    rule.smoothing_secs,
                );
                let ma =
                    Self::resolve_for_software(rule.source, rule.custom_mix.as_ref(), readings);
                let mb =
                    Self::resolve_for_software(rule.mix_source, rule.mix_custom.as_ref(), readings);
                if !ma.is_some_and(f32::is_finite) || !mb.is_some_and(f32::is_finite) {
                    self.primary.reset();
                    self.secondary.reset();
                    (100, true)
                } else {
                    let da = self
                        .primary
                        .evaluate_with_floor(&cfg_a, floor, ma, dt_secs, 0)
                        .unwrap_or_else(|| self.primary.last_duty().unwrap_or(floor));
                    let db = self
                        .secondary
                        .evaluate_with_floor(&cfg_b, floor, mb, dt_secs, 0)
                        .unwrap_or_else(|| self.secondary.last_duty().unwrap_or(floor));
                    self.primary.mark_applied(da);
                    self.secondary.mark_applied(db);
                    let combined = match rule.mix_op {
                        CurveMixOp::Max => da.max(db),
                        CurveMixOp::Avg => ((u16::from(da) + u16::from(db)) / 2) as u8,
                    };
                    (combined.clamp(floor, 100), false)
                }
            }
        };

        let target = if bypass_ramp || rule.ramp_pct_per_sec == 0 || dt_secs <= 0.0 {
            self.ramp_duty = Some(desired as f32);
            desired
        } else {
            let max_step = (rule.ramp_pct_per_sec as f32) * dt_secs;
            let cur = self.ramp_duty.unwrap_or(desired as f32);
            let next_f = if (desired as f32) > cur {
                (cur + max_step).min(desired as f32)
            } else {
                (cur - max_step).max(desired as f32)
            };
            self.ramp_duty = Some(next_f);
            (next_f.round() as i32).clamp(floor as i32, 100) as u8
        };

        let changed = self
            .last_applied
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

    #[test]
    fn software_fan_mix_takes_highest_curve_duty_not_temp() {
        let mut rule = SoftwareFanRule {
            mode: SoftwareFanMode::Mix,
            source: FanTempSource::Cpu,
            mix_source: FanTempSource::GpuEdge,
            mix_op: CurveMixOp::Max,
            smoothing_secs: 0,
            hysteresis_c: 0,
            ramp_pct_per_sec: 0,
            ..Default::default()
        };
        // CPU curve stays gentle at 70 C (40 %), GPU curve ramps hard at 65 C (85 %).
        for pt in &mut rule.curve {
            pt.duty_pct = 40;
        }
        for pt in &mut rule.mix_curve {
            pt.duty_pct = 85;
        }
        let r = SensorReadings {
            cpu: Some(70.0),
            gpu_edge: Some(65.0),
            ..Default::default()
        };
        let mut tr = SoftwareFanTracker::new();
        assert_eq!(tr.evaluate(&rule, 20, &r, 2.0, 1), Some(85));
    }

    #[test]
    fn software_fan_tracker_enforces_floor_ramp_and_failsafe() {
        let mut rule = SoftwareFanRule {
            mode: SoftwareFanMode::Curve,
            source: FanTempSource::Cpu,
            smoothing_secs: 0,
            hysteresis_c: 0,
            ramp_pct_per_sec: 10,
            ..Default::default()
        };
        for pt in &mut rule.curve {
            pt.duty_pct = 10;
        }
        let mut tr = SoftwareFanTracker::new();
        // Hardware floor 30 % clamps the 10 % curve point.
        let d0 = tr
            .evaluate(
                &rule,
                30,
                &SensorReadings {
                    cpu: Some(40.0),
                    ..Default::default()
                },
                2.0,
                1,
            )
            .unwrap();
        assert_eq!(d0, 30);
        tr.mark_applied(d0);

        // Curve jumps to 90 %, but ramp = 10 %/s * 2 s -> capped at 30 + 20 = 50 %.
        for pt in &mut rule.curve {
            pt.duty_pct = 90;
        }
        let d1 = tr
            .evaluate(
                &rule,
                30,
                &SensorReadings {
                    cpu: Some(80.0),
                    ..Default::default()
                },
                2.0,
                1,
            )
            .unwrap();
        assert_eq!(d1, 50);
        tr.mark_applied(d1);

        // Missing sensor bypasses ramp and fails safe to 100 % immediately.
        assert_eq!(
            tr.evaluate(&rule, 30, &SensorReadings::default(), 2.0, 1),
            Some(100)
        );
    }

    #[test]
    fn fan_calibration_computes_min_spinning_and_zero_rpm() {
        let cal = FanCalibration::from_samples(vec![(100, 1800), (60, 1050), (35, 620), (20, 0)]);
        assert_eq!(cal.min_spinning_pct, Some(35));
        assert!(cal.zero_rpm_supported);
        assert_eq!(cal.max_rpm, Some(1800));
        assert_eq!(cal.samples.first(), Some(&(20, 0)));
    }
}
