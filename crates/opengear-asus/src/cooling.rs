use anyhow::Result;
use opengear_platform::is_elevated;
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

    /// ASUS `ASUSManagement` WMI FanType IDs (verified on ASUS ROG desktop boards).
    pub fn asus_wmi_fan_type(self) -> u8 {
        match self {
            Self::CpuFan => 0,
            Self::ChassisFan1 => 1,
            Self::ChassisFan2 => 2,
            Self::ChassisFan3 => 3,
            Self::AioPump => 4,
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
    pub fn asus_profile_str(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Silent => "Silent",
            Self::Standard => "Standard",
            Self::Turbo => "Turbo",
            Self::FullSpeed => "Full Speed",
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
}

impl FanTempSource {
    pub fn id(self) -> &'static str {
        match self {
            Self::CpuBios => "cpu_bios",
            Self::GpuEdge => "gpu_edge",
            Self::GpuHotspot => "gpu_hotspot",
            Self::MaxCpuGpu => "max_cpu_gpu",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "gpu_edge" => Self::GpuEdge,
            "gpu_hotspot" => Self::GpuHotspot,
            "max_cpu_gpu" => Self::MaxCpuGpu,
            _ => Self::CpuBios,
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

#[derive(Debug, Clone)]
pub struct AsusWmiCoolingStatus {
    pub is_admin: bool,
    pub wmi_available: bool,
    pub message: String,
}

/// Probe `root\wmi\ASUSManagement` availability via native COM WMI (requires Admin rights).
pub fn probe_asus_wmi_cooling() -> AsusWmiCoolingStatus {
    let admin = is_elevated();
    if !admin {
        return AsusWmiCoolingStatus {
            is_admin: false,
            wmi_available: false,
            message: "Droits Administrateur requis pour accéder à l'interface BIOS/EC ASUSManagement WMI.".into(),
        };
    }

    #[cfg(windows)]
    if let Ok(conn) = opengear_platform::wmi::WmiConn::connect(r"root\wmi")
        && matches!(conn.first_instance_path("ASUSManagement"), Ok(Some(_)))
    {
        return AsusWmiCoolingStatus {
            is_admin: true,
            wmi_available: true,
            message: "Interface BIOS/EC ASUSManagement WMI active (prête pour les courbes PWM)."
                .into(),
        };
    }

    AsusWmiCoolingStatus {
        is_admin: true,
        wmi_available: false,
        message: "Administrateur actif, mais aucune instance ASUSManagement active (vérifie le pilote ASUS System Control Interface / BIOS Q-Fan).".into(),
    }
}

/// Validate explicit `0` ErrorCode values returned by `ASUSManagement` WMI methods.
pub(crate) fn check_wmi_error_codes(stdout: &str, expect_curve: bool) -> Result<()> {
    let mut expected = vec!["PolicyErr="];
    if expect_curve {
        expected.push("CurveErr=");
    }
    for prefix in expected {
        let code = stdout
            .split_whitespace()
            .find_map(|tok| tok.strip_prefix(prefix))
            .and_then(|s| s.trim().parse::<u32>().ok());
        match code {
            Some(0) => {}
            Some(_) => anyhow::bail!(
                "ASUSManagement BIOS a refusé la commande ({stdout}) — utilise le contrôle direct SuperIO PawnIO !"
            ),
            None => anyhow::bail!(
                "ASUSManagement n'a pas renvoyé de code de retour valide ({stdout:?}) — commande non confirmée"
            ),
        }
    }
    Ok(())
}

/// Apply an 8-point PWM fan/pump curve to the ASUS motherboard via native COM `ASUSManagement`.
pub fn apply_asus_wmi_curve(cfg: &FanChannelConfig) -> Result<String> {
    if !is_elevated() {
        anyhow::bail!(
            "Lance OpenGear en Administrateur pour appliquer les courbes PWM sur la carte mère ASUS."
        );
    }

    #[cfg(windows)]
    {
        use anyhow::Context;
        use opengear_platform::wmi::{WmiConn, WmiVal};

        let conn = WmiConn::connect(r"root\wmi")?;
        let obj_path = conn
            .first_instance_path("ASUSManagement")?
            .context("Instance ASUSManagement WMI introuvable")?;

        let ft = cfg.header.asus_wmi_fan_type();
        let prof = cfg.policy.asus_profile_str().to_string();
        let low_limit = (cfg.header.min_safe_duty_pct().max(20)) as u32;
        let safe_curve = cfg.sanitized_curve();

        let r1 = conn.exec_method(
            &obj_path,
            "ASUSManagement",
            "SetFanPolicy",
            &[
                ("FanType", WmiVal::U8(ft)),
                ("Mode", WmiVal::String("PWM".into())),
                ("Profile", WmiVal::String(prof)),
                ("Source", WmiVal::String("CPU".into())),
                ("LowLimit", WmiVal::U32(low_limit)),
            ],
        )?;
        let policy_err = r1.get("ErrorCode").and_then(|v| v.as_u64());

        if cfg.policy == FanPolicyMode::Manual {
            let p = &safe_curve;
            let curve_res = conn.exec_method(
                &obj_path,
                "ASUSManagement",
                "SetManualFanCurvePro",
                &[
                    ("FanType", WmiVal::U8(ft)),
                    ("Mode", WmiVal::String("PWM".into())),
                    ("Point1Temp", WmiVal::U8(p[0].temp_c)),
                    ("Point1Duty", WmiVal::U8(p[0].duty_pct)),
                    ("Point2Temp", WmiVal::U8(p[1].temp_c)),
                    ("Point2Duty", WmiVal::U8(p[1].duty_pct)),
                    ("Point3Temp", WmiVal::U8(p[2].temp_c)),
                    ("Point3Duty", WmiVal::U8(p[2].duty_pct)),
                    ("Point4Temp", WmiVal::U8(p[3].temp_c)),
                    ("Point4Duty", WmiVal::U8(p[3].duty_pct)),
                    ("Point5Temp", WmiVal::U8(p[4].temp_c)),
                    ("Point5Duty", WmiVal::U8(p[4].duty_pct)),
                    ("Point6Temp", WmiVal::U8(p[5].temp_c)),
                    ("Point6Duty", WmiVal::U8(p[5].duty_pct)),
                    ("Point7Temp", WmiVal::U8(p[6].temp_c)),
                    ("Point7Duty", WmiVal::U8(p[6].duty_pct)),
                    ("Point8Temp", WmiVal::U8(p[7].temp_c)),
                    ("Point8Duty", WmiVal::U8(p[7].duty_pct)),
                ],
            );
            let curve_err = match curve_res {
                Ok(ref r2) => r2.get("ErrorCode").and_then(|v| v.as_u64()),
                Err(e) => {
                    // Roll back Manual policy to BIOS Standard if curve programming failed.
                    let _ = conn.exec_method(
                        &obj_path,
                        "ASUSManagement",
                        "SetFanPolicy",
                        &[
                            ("FanType", WmiVal::U8(ft)),
                            ("Mode", WmiVal::String("PWM".into())),
                            ("Profile", WmiVal::String("Standard".into())),
                            ("Source", WmiVal::String("CPU".into())),
                            ("LowLimit", WmiVal::U32(low_limit)),
                        ],
                    );
                    return Err(e);
                }
            };
            let summary = format!(
                "PolicyErr={} CurveErr={}",
                policy_err.map(|v| v.to_string()).unwrap_or_default(),
                curve_err.map(|v| v.to_string()).unwrap_or_default(),
            );
            if check_wmi_error_codes(&summary, true).is_err() {
                let _ = conn.exec_method(
                    &obj_path,
                    "ASUSManagement",
                    "SetFanPolicy",
                    &[
                        ("FanType", WmiVal::U8(ft)),
                        ("Mode", WmiVal::String("PWM".into())),
                        ("Profile", WmiVal::String("Standard".into())),
                        ("Source", WmiVal::String("CPU".into())),
                        ("LowLimit", WmiVal::U32(low_limit)),
                    ],
                );
                check_wmi_error_codes(&summary, true)?;
            }
            Ok(summary)
        } else {
            let summary = format!(
                "PolicyErr={}",
                policy_err.map(|v| v.to_string()).unwrap_or_default()
            );
            check_wmi_error_codes(&summary, false)?;
            Ok(summary)
        }
    }

    #[cfg(not(windows))]
    {
        let _ = cfg;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Read back the current BIOS/EC policy and 8-point PWM curve from `root\wmi\ASUSManagement`
/// via native COM WMI (`GetFanPolicy` + `GetManualFanCurvePro`).
pub fn read_asus_wmi_channel(header: FanHeaderKind) -> Option<FanChannelConfig> {
    if !is_elevated() {
        return None;
    }
    #[cfg(windows)]
    {
        use opengear_platform::wmi::{WmiConn, WmiVal};

        let conn = WmiConn::connect(r"root\wmi").ok()?;
        let obj_path = conn.first_instance_path("ASUSManagement").ok()??;
        let ft = header.asus_wmi_fan_type();

        let p = conn
            .exec_method(
                &obj_path,
                "ASUSManagement",
                "GetFanPolicy",
                &[("FanType", WmiVal::U8(ft))],
            )
            .ok();
        let c = conn
            .exec_method(
                &obj_path,
                "ASUSManagement",
                "GetManualFanCurvePro",
                &[
                    ("FanType", WmiVal::U8(ft)),
                    ("Mode", WmiVal::String("PWM".into())),
                ],
            )
            .ok();

        let pol_str = p
            .as_ref()
            .and_then(|m| m.get("Profile"))
            .and_then(|v| v.as_str())
            .unwrap_or("Manual");
        let mut cfg = FanChannelConfig::default_for(header);
        cfg.policy = match pol_str.trim() {
            "Silent" => FanPolicyMode::Silent,
            "Standard" => FanPolicyMode::Standard,
            "Turbo" => FanPolicyMode::Turbo,
            "Full Speed" | "FullSpeed" => FanPolicyMode::FullSpeed,
            _ => FanPolicyMode::Manual,
        };

        if let Some(cm) = c {
            let mut parsed = Vec::with_capacity(8);
            for i in 1..=8 {
                let tk = format!("Point{i}Temp");
                let dk = format!("Point{i}Duty");
                let tv = cm.get(&tk).and_then(|v| v.as_u64()).unwrap_or(0);
                let dv = cm.get(&dk).and_then(|v| v.as_u64()).unwrap_or(255);
                if (20..=105).contains(&tv) && dv <= 100 {
                    parsed.push(CurvePoint {
                        temp_c: (tv as u8).min(100),
                        duty_pct: dv as u8,
                    });
                }
            }
            if parsed.len() == 8 {
                cfg.curve.copy_from_slice(&parsed);
            }
        }
        Some(cfg)
    }
    #[cfg(not(windows))]
    {
        let _ = header;
        None
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
    fn wmi_error_codes_require_explicit_zero() {
        assert!(check_wmi_error_codes("PolicyErr=0 CurveErr=0", true).is_ok());
        assert!(check_wmi_error_codes("PolicyErr=0", false).is_ok());
        assert!(check_wmi_error_codes("PolicyErr=1 CurveErr=0", true).is_err());
        assert!(check_wmi_error_codes("PolicyErr= CurveErr=", true).is_err());
        assert!(check_wmi_error_codes("PolicyErr=0", true).is_err());
        assert!(check_wmi_error_codes("", false).is_err());
    }
}
