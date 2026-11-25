//! ASUS motherboard fan control through the `ASUSManagement` WMI interface (BIOS Q-Fan curves).

use anyhow::Result;
use opengear_core::cooling::{CurvePoint, FanChannelConfig, FanHeaderKind, FanPolicyMode};
use opengear_platform::is_elevated;

/// ASUS `ASUSManagement` WMI FanType IDs (verified on ASUS ROG desktop boards).
fn asus_fan_type(header: FanHeaderKind) -> u8 {
    match header {
        FanHeaderKind::CpuFan => 0,
        FanHeaderKind::ChassisFan1 => 1,
        FanHeaderKind::ChassisFan2 => 2,
        FanHeaderKind::ChassisFan3 => 3,
        FanHeaderKind::AioPump => 4,
    }
}

/// ASUS `ASUSManagement` fan profile names.
fn asus_profile(policy: FanPolicyMode) -> &'static str {
    match policy {
        FanPolicyMode::Manual => "Manual",
        FanPolicyMode::Silent => "Silent",
        FanPolicyMode::Standard => "Standard",
        FanPolicyMode::Turbo => "Turbo",
        FanPolicyMode::FullSpeed => "Full Speed",
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

        let ft = asus_fan_type(cfg.header);
        let prof = asus_profile(cfg.policy).to_string();
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
        let ft = asus_fan_type(header);

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
mod tests {
    use super::*;

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
