use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub temp_c: u8,
    pub duty_pct: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanHeaderKind {
    /// CPU_FAN (Radiator fans on be quiet! Silent Loop 3)
    CpuFan,
    /// AIO_PUMP (be quiet! Silent Loop 3 — 6-pole 3-phase PWM pump, up to 3500 RPM)
    AioPump,
    /// CHA_FAN1 (Chassis / Intake fans)
    ChassisFan1,
    /// CHA_FAN2 (Chassis / Exhaust fans)
    ChassisFan2,
    /// CHA_FAN3
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

    /// ASUSManagement WMI `FanType` numeric ID mapping on desktop ROG boards.
    pub fn asus_wmi_fan_type(self) -> u8 {
        match self {
            Self::CpuFan => 0,
            Self::ChassisFan1 => 1,
            Self::ChassisFan2 => 2,
            Self::ChassisFan3 => 3,
            Self::AioPump => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AioPump => "Pompe AIO — be quiet! Silent Loop 3 (PWM 3500 RPM)",
            Self::CpuFan => "Ventilateurs Radiateur (CPU_FAN)",
            Self::ChassisFan1 => "Ventilateurs Boîtier #1 (CHA_FAN1)",
            Self::ChassisFan2 => "Ventilateurs Boîtier #2 (CHA_FAN2)",
            Self::ChassisFan3 => "Ventilateurs Boîtier #3 (CHA_FAN3)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::AioPump => "AIO Pump (Silent Loop 3)",
            Self::CpuFan => "CPU_FAN (Radiateur)",
            Self::ChassisFan1 => "CHA_FAN1",
            Self::ChassisFan2 => "CHA_FAN2",
            Self::ChassisFan3 => "CHA_FAN3",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanPolicyMode {
    Silent,
    Standard,
    Turbo,
    FullSpeed,
    Manual,
}

impl FanPolicyMode {
    pub const ALL: &'static [Self] = &[
        Self::Silent,
        Self::Standard,
        Self::Turbo,
        Self::FullSpeed,
        Self::Manual,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Silent => "Silencieux",
            Self::Standard => "Standard",
            Self::Turbo => "Turbo",
            Self::FullSpeed => "Pleine vitesse (100%)",
            Self::Manual => "Courbe manuelle (8 pts)",
        }
    }

    pub fn asus_profile_str(self) -> &'static str {
        match self {
            Self::Silent => "Silent",
            Self::Standard => "Standard",
            Self::Turbo => "Turbo",
            Self::FullSpeed => "Full Speed",
            Self::Manual => "Manual",
        }
    }
}

/// Temperature source inspired by FanControl: BIOS CPU, Live GPU (AMD ADL2), or Mix max(CPU, GPU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FanTempSource {
    #[default]
    CpuBios,
    GpuLive,
    MixCpuGpu,
}

impl FanTempSource {
    pub fn from_str_id(s: &str) -> Self {
        match s {
            "GpuLive" => Self::GpuLive,
            "MixCpuGpu" => Self::MixCpuGpu,
            _ => Self::CpuBios,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanChannelConfig {
    pub header: FanHeaderKind,
    pub policy: FanPolicyMode,
    /// 8-point temperature/duty curve (matches ASUSManagement `SetManualFanCurvePro`)
    pub curve: [CurvePoint; 8],
    /// FanControl-style smart temperature source (BIOS CPU / Live GPU / Mix max(CPU,GPU))
    #[serde(default)]
    pub source: FanTempSource,
    /// Hysteresis in °C before ramping fans down (prevents Ryzen/GPU fan yoyo!)
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
    /// - duty clamped to `min_safe_duty_pct..=100` and monotonically non-decreasing
    ///   (a hotter point can never ask for less airflow than a cooler one).
    pub fn sanitized_curve(&self) -> [CurvePoint; 8] {
        let min_duty = self.header.min_safe_duty_pct();
        let mut pts = self.curve;
        pts.sort_by_key(|p| p.temp_c);
        let mut prev_temp: Option<u8> = None;
        let mut prev_duty = min_duty;
        for p in pts.iter_mut() {
            let mut t = p.temp_c.clamp(20, 100);
            if let Some(pt) = prev_temp {
                if t <= pt {
                    t = (pt + 1).min(100);
                }
            }
            p.temp_c = t;
            prev_temp = Some(t);
            p.duty_pct = p.duty_pct.clamp(min_duty, 100).max(prev_duty);
            prev_duty = p.duty_pct;
        }
        pts
    }

    /// Linearly interpolate target PWM duty (%) from the 8-point curve for a given temperature (°C).
    pub fn interpolate_duty(&self, temp_c: f32) -> u8 {
        let mut pts = self.curve;
        pts.sort_by_key(|p| p.temp_c);
        if temp_c <= pts[0].temp_c as f32 {
            return pts[0].duty_pct;
        }
        for w in pts.windows(2) {
            let t0 = w[0].temp_c as f32;
            let t1 = w[1].temp_c as f32;
            if temp_c <= t1 && t1 > t0 {
                let f = ((temp_c - t0) / (t1 - t0)).clamp(0.0, 1.0);
                let d = w[0].duty_pct as f32 + (w[1].duty_pct as f32 - w[0].duty_pct as f32) * f;
                return d.round().clamp(0.0, 100.0) as u8;
            }
        }
        pts[7].duty_pct
    }

    pub fn default_for(header: FanHeaderKind) -> Self {
        let curve = match header {
            // be quiet! Silent Loop 3: keep minimum >= 45% for steady coolant flow & zero airlocks
            FanHeaderKind::AioPump => [
                CurvePoint { temp_c: 30, duty_pct: 55 },
                CurvePoint { temp_c: 40, duty_pct: 58 },
                CurvePoint { temp_c: 50, duty_pct: 64 },
                CurvePoint { temp_c: 60, duty_pct: 72 },
                CurvePoint { temp_c: 68, duty_pct: 82 },
                CurvePoint { temp_c: 75, duty_pct: 92 },
                CurvePoint { temp_c: 82, duty_pct: 100 },
                CurvePoint { temp_c: 90, duty_pct: 100 },
            ],
            FanHeaderKind::CpuFan => [
                CurvePoint { temp_c: 30, duty_pct: 25 },
                CurvePoint { temp_c: 40, duty_pct: 32 },
                CurvePoint { temp_c: 50, duty_pct: 42 },
                CurvePoint { temp_c: 60, duty_pct: 55 },
                CurvePoint { temp_c: 70, duty_pct: 72 },
                CurvePoint { temp_c: 78, duty_pct: 88 },
                CurvePoint { temp_c: 85, duty_pct: 100 },
                CurvePoint { temp_c: 92, duty_pct: 100 },
            ],
            _ => [
                CurvePoint { temp_c: 30, duty_pct: 25 },
                CurvePoint { temp_c: 40, duty_pct: 30 },
                CurvePoint { temp_c: 50, duty_pct: 40 },
                CurvePoint { temp_c: 60, duty_pct: 50 },
                CurvePoint { temp_c: 70, duty_pct: 65 },
                CurvePoint { temp_c: 78, duty_pct: 80 },
                CurvePoint { temp_c: 85, duty_pct: 95 },
                CurvePoint { temp_c: 92, duty_pct: 100 },
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

    /// Preset curves specifically tuned for be quiet! Silent Loop 3 pump (4-pin PWM, 3500 RPM max).
    pub fn silent_loop3_preset(preset: &str) -> [CurvePoint; 8] {
        match preset {
            "ultra_silent" => [
                CurvePoint { temp_c: 30, duty_pct: 45 },
                CurvePoint { temp_c: 40, duty_pct: 45 },
                CurvePoint { temp_c: 50, duty_pct: 50 },
                CurvePoint { temp_c: 60, duty_pct: 58 },
                CurvePoint { temp_c: 70, duty_pct: 70 },
                CurvePoint { temp_c: 78, duty_pct: 82 },
                CurvePoint { temp_c: 85, duty_pct: 95 },
                CurvePoint { temp_c: 92, duty_pct: 100 },
            ],
            "max_perf" => [
                CurvePoint { temp_c: 30, duty_pct: 75 },
                CurvePoint { temp_c: 40, duty_pct: 80 },
                CurvePoint { temp_c: 50, duty_pct: 85 },
                CurvePoint { temp_c: 60, duty_pct: 92 },
                CurvePoint { temp_c: 68, duty_pct: 100 },
                CurvePoint { temp_c: 75, duty_pct: 100 },
                CurvePoint { temp_c: 82, duty_pct: 100 },
                CurvePoint { temp_c: 90, duty_pct: 100 },
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

/// Check whether current process runs with Administrator privileges (native Win32 TokenElevation — 0 subprocesses).
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 || token.is_null() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret_len: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut TOKEN_ELEVATION as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elev.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Relaunch OpenGear elevated via Windows UAC (`ShellExecuteW("runas")`) to unlock ASUS WMI fan control.
#[cfg(windows)]
pub fn relaunch_as_admin() -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().context("Chemin exécutable introuvable")?;
    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let res = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        if (res as isize) <= 32 {
            anyhow::bail!("Élévation UAC refusée ou échouée (code {})", res as isize);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn relaunch_as_admin() -> Result<()> {
    anyhow::bail!("Non supporté hors Windows")
}

/// Probe `root\wmi\ASUSManagement` availability (requires Admin rights).
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
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "$i = Get-CimInstance -Namespace root\\wmi -ClassName ASUSManagement -ErrorAction SilentlyContinue; if ($i) { 'OK' } else { 'NO_INSTANCE' }",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        if let Ok(o) = out {
            let txt = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if txt == "OK" {
                return AsusWmiCoolingStatus {
                    is_admin: true,
                    wmi_available: true,
                    message: "Interface BIOS/EC ASUSManagement WMI active (prête pour les courbes PWM).".into(),
                };
            }
        }
    }

    AsusWmiCoolingStatus {
        is_admin: true,
        wmi_available: false,
        message: "Administrateur actif, mais aucune instance ASUSManagement active (vérifie le pilote ASUS System Control Interface / BIOS Q-Fan).".into(),
    }
}

/// Parse the `PolicyErr=<n> [CurveErr=<n>]` status line printed by the WMI script.
/// Succeeds only if every expected code is present, numeric and equal to 0.
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
                "ASUSManagement BIOS a refusé la commande ({stdout}) — utilise le contrôle direct SuperIO PawnIO (live RPM/PWM) juste en dessous !"
            ),
            None => anyhow::bail!(
                "ASUSManagement n'a pas renvoyé de code de retour valide ({stdout:?}) — commande non confirmée"
            ),
        }
    }
    Ok(())
}

/// Apply an 8-point PWM fan/pump curve to the ASUS ROG motherboard via `ASUSManagement::SetManualFanCurvePro` + `SetFanPolicy`.
pub fn apply_asus_wmi_curve(cfg: &FanChannelConfig) -> Result<String> {
    if !is_elevated() {
        anyhow::bail!("Lance OpenGear en Administrateur pour appliquer les courbes PWM sur la carte mère ASUS.");
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let ft = cfg.header.asus_wmi_fan_type();
        let prof = cfg.policy.asus_profile_str();
        // Never send a raw UI curve to the BIOS: enforce ordering, monotonic duty and header floors.
        let safe_curve = cfg.sanitized_curve();
        let p = &safe_curve;

        let script = if cfg.policy == FanPolicyMode::Manual {
            format!(
                "$i = Get-CimInstance -Namespace root\\wmi -ClassName ASUSManagement | Select-Object -First 1; \
                 if (-not $i) {{ throw 'ASUSManagement WMI instance introuvable' }}; \
                 $r1 = Invoke-CimMethod -InputObject $i -MethodName SetFanPolicy -Arguments @{{ FanType=[byte]{ft}; Mode='PWM'; Profile='Manual'; Source='CPU'; LowLimit=[uint32]20 }}; \
                 $r2 = Invoke-CimMethod -InputObject $i -MethodName SetManualFanCurvePro -Arguments @{{ \
                   FanType=[byte]{ft}; Mode='PWM'; \
                   Point1Temp=[byte]{}; Point1Duty=[byte]{}; \
                   Point2Temp=[byte]{}; Point2Duty=[byte]{}; \
                   Point3Temp=[byte]{}; Point3Duty=[byte]{}; \
                   Point4Temp=[byte]{}; Point4Duty=[byte]{}; \
                   Point5Temp=[byte]{}; Point5Duty=[byte]{}; \
                   Point6Temp=[byte]{}; Point6Duty=[byte]{}; \
                   Point7Temp=[byte]{}; Point7Duty=[byte]{}; \
                   Point8Temp=[byte]{}; Point8Duty=[byte]{} \
                 }}; \
                 \"PolicyErr=$($r1.ErrorCode) CurveErr=$($r2.ErrorCode)\"",
                p[0].temp_c, p[0].duty_pct,
                p[1].temp_c, p[1].duty_pct,
                p[2].temp_c, p[2].duty_pct,
                p[3].temp_c, p[3].duty_pct,
                p[4].temp_c, p[4].duty_pct,
                p[5].temp_c, p[5].duty_pct,
                p[6].temp_c, p[6].duty_pct,
                p[7].temp_c, p[7].duty_pct,
            )
        } else {
            format!(
                "$i = Get-CimInstance -Namespace root\\wmi -ClassName ASUSManagement | Select-Object -First 1; \
                 if (-not $i) {{ throw 'ASUSManagement WMI instance introuvable' }}; \
                 $r1 = Invoke-CimMethod -InputObject $i -MethodName SetFanPolicy -Arguments @{{ FanType=[byte]{ft}; Mode='PWM'; Profile='{prof}'; Source='CPU'; LowLimit=[uint32]20 }}; \
                 \"PolicyErr=$($r1.ErrorCode)\""
            )
        };

        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("Échec exécution PowerShell WMI")?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("Erreur ASUSManagement WMI : {}", err.trim());
        }
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        // STRICT ERROR CHECK: never claim success unless the ASUS BIOS explicitly returned 0
        // for every call (an empty / unparsable ErrorCode means the method did not run).
        check_wmi_error_codes(&stdout, cfg.policy == FanPolicyMode::Manual)?;
        Ok(stdout)
    }

    #[cfg(not(windows))]
    {
        let _ = cfg;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Read back the current BIOS/EC policy and 8-point PWM curve from `root\wmi\ASUSManagement`
/// (`GetFanPolicy` + `GetManualFanCurvePro`) when running elevated as Admin.
pub fn read_asus_wmi_channel(header: FanHeaderKind) -> Option<FanChannelConfig> {
    if !is_elevated() {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let ft = header.asus_wmi_fan_type();
        let script = format!(
            "$i = Get-CimInstance -Namespace root\\wmi -ClassName ASUSManagement -ErrorAction SilentlyContinue | Select-Object -First 1; \
             if (-not $i) {{ exit 1 }}; \
             $p = Invoke-CimMethod -InputObject $i -MethodName GetFanPolicy -Arguments @{{ FanType=[byte]{ft} }} -ErrorAction SilentlyContinue; \
             $c = Invoke-CimMethod -InputObject $i -MethodName GetManualFanCurvePro -Arguments @{{ FanType=[byte]{ft}; Mode='PWM' }} -ErrorAction SilentlyContinue; \
             $pol = if ($p -and $p.Profile) {{ $p.Profile }} else {{ 'Manual' }}; \
             if ($c) {{ \
               \"$pol|$($c.Point1Temp),$($c.Point1Duty);$($c.Point2Temp),$($c.Point2Duty);$($c.Point3Temp),$($c.Point3Duty);$($c.Point4Temp),$($c.Point4Duty);$($c.Point5Temp),$($c.Point5Duty);$($c.Point6Temp),$($c.Point6Duty);$($c.Point7Temp),$($c.Point7Duty);$($c.Point8Temp),$($c.Point8Duty)\" \
             }} else {{ \"$pol|\" }}"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let (pol_str, pts_str) = line.split_once('|')?;
        let mut cfg = FanChannelConfig::default_for(header);
        cfg.policy = match pol_str.trim() {
            "Silent" => FanPolicyMode::Silent,
            "Standard" => FanPolicyMode::Standard,
            "Turbo" => FanPolicyMode::Turbo,
            "Full Speed" | "FullSpeed" => FanPolicyMode::FullSpeed,
            _ => FanPolicyMode::Manual,
        };
        let mut parsed_pts = Vec::new();
        for tok in pts_str.split(';') {
            if let Some((t, d)) = tok.split_once(',') {
                if let (Ok(tv), Ok(dv)) = (t.trim().parse::<u8>(), d.trim().parse::<u8>()) {
                    if tv >= 20 && tv <= 105 && dv <= 100 {
                        parsed_pts.push(CurvePoint {
                            temp_c: tv.min(100),
                            duty_pct: dv,
                        });
                    }
                }
            }
        }
        if parsed_pts.len() == 8 {
            cfg.curve.copy_from_slice(&parsed_pts);
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
            c.curve[i] = CurvePoint { temp_c: t, duty_pct: d };
        }
        c
    }

    #[test]
    fn pump_curve_never_drops_below_floor() {
        let c = cfg_with(FanHeaderKind::AioPump, [(30, 0); 8]);
        assert!(c.sanitized_curve().iter().all(|p| p.duty_pct >= 45));
    }

    #[test]
    fn cpu_fan_floor_and_chassis_fan_stop_allowed() {
        let cpu = cfg_with(FanHeaderKind::CpuFan, [(30, 0); 8]);
        assert!(cpu.sanitized_curve().iter().all(|p| p.duty_pct >= 20));
        let cha = cfg_with(FanHeaderKind::ChassisFan1, [(30, 0); 8]);
        assert_eq!(cha.sanitized_curve()[0].duty_pct, 0);
    }

    #[test]
    fn curve_is_sorted_strictly_increasing_and_monotonic() {
        let c = cfg_with(
            FanHeaderKind::ChassisFan1,
            [(90, 100), (30, 40), (60, 20), (60, 70), (10, 10), (200, 50), (45, 30), (75, 90)],
        );
        let s = c.sanitized_curve();
        for w in s.windows(2) {
            assert!(w[1].temp_c > w[0].temp_c, "temps must strictly increase: {s:?}");
            assert!(w[1].duty_pct >= w[0].duty_pct, "duty must not decrease: {s:?}");
        }
        assert!(s.iter().all(|p| (20..=100).contains(&p.temp_c) && p.duty_pct <= 100));
    }

    #[test]
    fn wmi_error_codes_require_explicit_zero() {
        assert!(check_wmi_error_codes("PolicyErr=0 CurveErr=0", true).is_ok());
        assert!(check_wmi_error_codes("PolicyErr=0", false).is_ok());
        assert!(check_wmi_error_codes("PolicyErr=1 CurveErr=0", true).is_err());
        // Empty / missing codes used to be reported as success
        assert!(check_wmi_error_codes("PolicyErr= CurveErr=", true).is_err());
        assert!(check_wmi_error_codes("PolicyErr=0", true).is_err());
        assert!(check_wmi_error_codes("", false).is_err());
    }
}
