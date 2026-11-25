//! Automated, precise hardware self-test suite with real register/protocol read-back verification.
//! Run via CLI (`opengear selftest`), UI button ("Auto-Test Matériel"), or `cargo test`.
//! Automatically saves a detailed report to `%TEMP%\opengear_selftest.txt`.

use crate::audio_ext::read_audio_state;
use opengear_asus::{AuraMainboardController, read_asus_wmi_channel};
use opengear_core::cooling::FanHeaderKind;
use opengear_corsair::CorsairDevice;
use opengear_hp::probe_hp_omen;
use opengear_logitech::{LogitechHeadset, LogitechMouse};
use opengear_mainboard::{diagnose_smbus_readonly, read_native_cpu_temp, read_superio_snapshot};
use opengear_platform::pawnio::pawnio_availability;
use opengear_platform::{
    TelemetryCollector, is_elevated, is_laptop_chassis, read_display_info, read_laptop_battery,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfTestCheck {
    pub name: String,
    /// "PASS" | "WARN" | "FAIL" | "SKIP_ADMIN"
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfTestReport {
    pub is_admin: bool,
    pub passed: usize,
    pub warned: usize,
    pub failed: usize,
    pub skipped_admin: usize,
    pub report_path: String,
    pub checks: Vec<SelfTestCheck>,
}

pub fn run_full_selftest() -> SelfTestReport {
    let admin = is_elevated();
    let mut checks = Vec::new();

    // 1. Corsair Bragi v2 (Keyboard / Mouse / HWP Flash / 8000 Hz)
    let corsair = CorsairDevice::connect_all();
    if corsair.is_empty() {
        checks.push(SelfTestCheck {
            name: "Corsair Bragi v2".into(),
            status: "WARN".into(),
            detail: "Aucun périphérique Corsair Bragi détecté.".into(),
        });
    } else {
        for d in &corsair {
            let hz = d.info.pollrate.map(|p| p.hz()).unwrap_or(0);
            let deb = d
                .info
                .debounce_timeout_ms
                .map(|m| format!("{m}ms"))
                .unwrap_or_else(|| "—".into());
            let hwp_n = d.info.hwp_profiles.len();
            checks.push(SelfTestCheck {
                name: format!(
                    "Corsair {} ({:04X}:{:04X})",
                    d.info.product_name, d.info.vid, d.info.pid
                ),
                status: "PASS".into(),
                detail: format!(
                    "FW {} | Polling={} Hz | Debounce={} | Profils Flash HWP={}",
                    d.info.firmware_version, hz, deb, hwp_n
                ),
            });
        }
    }

    // 2. Logitech G HID++ 2.0 (Mouse + 5-profile CRC-16 + Headset EQ/Sidetone)
    let mice = LogitechMouse::connect_all();
    if mice.is_empty() {
        checks.push(SelfTestCheck {
            name: "Logitech G Souris (HID++ 2.0)".into(),
            status: "WARN".into(),
            detail: "Aucune souris Logitech active (en veille ?).".into(),
        });
    } else {
        for m in &mice {
            let crc_ok = m
                .info
                .onboard_profiles
                .iter()
                .filter(|p| p.crc_valid)
                .count();
            let total = m.info.onboard_profiles.len();
            let bat = m
                .info
                .battery
                .as_ref()
                .and_then(|b| b.percent)
                .map(|p| format!("{p}%"))
                .unwrap_or_else(|| "—".into());
            let dpi = m.info.dpi.as_ref().map(|d| d.dpi_x).unwrap_or(0);
            checks.push(SelfTestCheck {
                name: format!(
                    "Logitech {} ({:04X}:{:04X})",
                    m.info.name, m.info.vid, m.info.pid
                ),
                status: if total == 0 || crc_ok == total {
                    "PASS"
                } else {
                    "WARN"
                }
                .into(),
                detail: format!(
                    "Batterie={} | DPI={} | Flash Onboard CRC-16 OK={}/{} | {} features",
                    bat,
                    dpi,
                    crc_ok,
                    total,
                    m.info.features.len()
                ),
            });
        }
    }

    let headsets = LogitechHeadset::connect_all();
    for h in &headsets {
        let eq_n = h.info.equalizer.as_ref().map(|e| e.band_count).unwrap_or(0);
        checks.push(SelfTestCheck {
            name: format!(
                "Logitech Casque {} ({:04X}:{:04X})",
                h.info.name, h.info.vid, h.info.pid
            ),
            status: "PASS".into(),
            detail: format!("Sidetone={}% | EQ DSP={} bandes", h.info.sidetone_pct, eq_n),
        });
    }

    // 3. ASUS Aura USB (0B05:19AF)
    match AuraMainboardController::connect() {
        Ok(a) => checks.push(SelfTestCheck {
            name: format!("ASUS Aura USB ({:04X}:{:04X})", a.info.vid, a.info.pid),
            status: "PASS".into(),
            detail: format!(
                "FW {} | {} LEDs CM | {}x 12V RGB | {}x 5V ARGB Gen2",
                a.info.firmware_version,
                a.info.mainboard_fixed_leds,
                a.info.rgb_12v_headers,
                a.info.argb_5v_headers
            ),
        }),
        Err(e) => checks.push(SelfTestCheck {
            name: "ASUS Aura USB".into(),
            status: "WARN".into(),
            detail: format!("Non connecté : {e}"),
        }),
    }

    // 4. GPU (NVML / ADL2) & Windows WASAPI Audio
    let is_laptop = is_laptop_chassis();
    let mut tel = TelemetryCollector::new();
    let sample = tel.sample();
    let gpu_backend = if sample.gpu.backend.is_empty() {
        "N/A"
    } else {
        sample.gpu.backend.as_str()
    };
    checks.push(SelfTestCheck {
        name: "GPU & Télémétrie".into(),
        status: if sample.gpu.available { "PASS" } else { "WARN" }.into(),
        detail: format!(
            "CPU {:.0}% | RAM {:.1}/{:.1} Go | {} ({}) : {:.0?}°C, {:.1?}W",
            sample.cpu_usage_pct,
            sample.ram_used_gb,
            sample.ram_total_gb,
            tel.static_info.gpu_name,
            gpu_backend,
            sample.gpu.edge_temp_c,
            sample.gpu.power_watts
        ),
    });

    // 4b. Batterie & Écran Portable (si châssis portable / batterie présente)
    let bat = read_laptop_battery();
    if is_laptop || bat.present {
        let disp = read_display_info();
        checks.push(SelfTestCheck {
            name: "Batterie & Écran Portable".into(),
            status: "PASS".into(),
            detail: format!(
                "Batterie {}% (Santé {}%, {} cycles, {}) | Écran {}x{} @ {} Hz ({:?}) Luminosité {}%",
                bat.percent.map(|p| p.to_string()).unwrap_or_else(|| "—".into()),
                bat.health_pct.map(|h| format!("{h:.0}")).unwrap_or_else(|| "—".into()),
                bat.cycle_count.map(|c| c.to_string()).unwrap_or_else(|| "—".into()),
                if bat.ac_online { "Secteur" } else { "Batterie" },
                disp.width,
                disp.height,
                disp.current_hz.map(|h| h.to_string()).unwrap_or_else(|| "—".into()),
                disp.supported_hz,
                disp.brightness_pct.map(|b| b.to_string()).unwrap_or_else(|| "—".into()),
            ),
        });
    }

    // 4c. HP OMEN BIOS WMI (hpqBIntM)
    let omen = probe_hp_omen();
    if omen.detected {
        let (st_str, detail) = if !admin {
            (
                "SKIP_ADMIN",
                format!(
                    "{} ({}) détecté — relance en Administrateur pour interroger hpqBIntM (ventilateurs, RGB 4 zones, TGP GPU).",
                    omen.model, omen.board_id
                ),
            )
        } else if omen.wmi_accessible {
            (
                "PASS",
                format!(
                    "{} | Ventilateurs={} ({:?} tr/min) | Clavier={} | Rétroéclairage={}",
                    omen.diagnostic,
                    omen.fan_count,
                    omen.fan_rpms,
                    omen.keyboard_label,
                    if omen.backlight_on { "ON" } else { "OFF" }
                ),
            )
        } else {
            ("WARN", omen.diagnostic.clone())
        };
        checks.push(SelfTestCheck {
            name: "HP OMEN BIOS WMI (hpqBIntM)".into(),
            status: st_str.into(),
            detail,
        });
    }

    let au = read_audio_state();
    checks.push(SelfTestCheck {
        name: "Windows Core Audio (WASAPI)".into(),
        status: if au.available { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "Sortie={}% (muet={}) | Micro={} ({}% muet={})",
            au.output_volume_pct,
            au.output_muted,
            if au.mic_available { "OK" } else { "absent" },
            au.mic_volume_pct,
            au.mic_muted
        ),
    });

    // 5. PawnIO Driver + Native CPU Temp (AMD Ryzen / Intel Core)
    let pio_avail = pawnio_availability();
    if !admin {
        checks.push(SelfTestCheck {
            name: "PawnIO CPU Temp + SuperIO PWM + SMBus RAM".into(),
            status: "SKIP_ADMIN".into(),
            detail: format!(
                "Statut PawnIO={pio_avail:?} — Relance en Administrateur (UAC) pour exécuter les tests matériels SuperIO / SMBus / CPU Temp."
            ),
        });
    } else {
        match read_native_cpu_temp() {
            Some((t, src)) => checks.push(SelfTestCheck {
                name: "PawnIO Température CPU Native".into(),
                status: "PASS".into(),
                detail: format!("{t:.1} °C ({src})"),
            }),
            None => checks.push(SelfTestCheck {
                name: "PawnIO Température CPU Native".into(),
                status: "FAIL".into(),
                detail: format!("Lecture CPU Temp échouée (PawnIO={pio_avail:?})"),
            }),
        }

        // 6. PawnIO SuperIO (Nuvoton NCT679xD live RPM & PWM)
        let sio = read_superio_snapshot();
        if sio.detected {
            let fan_summary: Vec<String> = sio
                .fans
                .iter()
                .map(|f| {
                    format!(
                        "Ch{}={}RPM/{}%({})",
                        f.index + 1,
                        f.rpm,
                        f.pwm_pct,
                        if f.is_manual { "MAN" } else { "BIOS" }
                    )
                })
                .collect();
            checks.push(SelfTestCheck {
                name: format!("PawnIO SuperIO ({})", sio.chip_name),
                status: "PASS".into(),
                detail: format!("{} | {}", sio.diagnostic, fan_summary.join(", ")),
            });
        } else {
            checks.push(SelfTestCheck {
                name: "PawnIO SuperIO (LpcIO)".into(),
                status: if is_laptop { "WARN" } else { "FAIL" }.into(),
                detail: if is_laptop {
                    format!(
                        "{} — non applicable sur PC portable (contrôle via EC / WMI constructeur).",
                        sio.diagnostic
                    )
                } else {
                    sio.diagnostic
                },
            });
        }

        // 7. PawnIO SMBus Full Read-Only Discovery + RAM RGB
        let smbus_diag = diagnose_smbus_readonly();
        let ram = crate::drivers::ram_rgb().probe();
        checks.push(SelfTestCheck {
            name: "PawnIO SMBus & RAM DDR RGB".into(),
            status: if !ram.ene_modules.is_empty() {
                "PASS"
            } else {
                "WARN"
            }
            .into(),
            detail: format!("{} || SMBus: {}", ram.message, smbus_diag.join(" ; ")),
        });

        // 8. ASUSManagement WMI Fan Policy Read-Back
        let mut wmi_fans = Vec::new();
        for &h in FanHeaderKind::ALL {
            if let Some(cfg) = read_asus_wmi_channel(h) {
                wmi_fans.push(format!("{:?}={:?}", h, cfg.policy));
            }
        }
        checks.push(SelfTestCheck {
            name: "ASUSManagement WMI Q-Fan".into(),
            status: if !wmi_fans.is_empty() { "PASS" } else { "WARN" }.into(),
            detail: if wmi_fans.is_empty() {
                "Aucune politique relue depuis ASUSManagement WMI (utilise SuperIO PawnIO pour le contrôle live !)".into()
            } else {
                wmi_fans.join(", ")
            },
        });
    }

    let passed = checks.iter().filter(|c| c.status == "PASS").count();
    let warned = checks.iter().filter(|c| c.status == "WARN").count();
    let failed = checks.iter().filter(|c| c.status == "FAIL").count();
    let skipped_admin = checks.iter().filter(|c| c.status == "SKIP_ADMIN").count();

    let report_path = save_report_file(admin, &checks);

    SelfTestReport {
        is_admin: admin,
        passed,
        warned,
        failed,
        skipped_admin,
        report_path: report_path.display().to_string(),
        checks,
    }
}

fn save_report_file(is_admin: bool, checks: &[SelfTestCheck]) -> PathBuf {
    let path = std::env::temp_dir().join("opengear_selftest.txt");
    let mut txt = format!("=== OpenGear Hardware Self-Test Report (Admin={is_admin}) ===\n");
    for c in checks {
        txt.push_str(&format!("[{:^10}] {} => {}\n", c.status, c.name, c.detail));
    }
    let _ = std::fs::write(&path, txt);
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use opengear_core::cooling::{FanChannelConfig, FanHeaderKind};

    #[test]
    fn test_fan_curve_interpolation_and_pump_safety() {
        let cfg = FanChannelConfig::default_for(FanHeaderKind::AioPump);
        assert_eq!(cfg.interpolate_duty(25.0), 55);
        assert_eq!(cfg.interpolate_duty(30.0), 55);
        assert_eq!(cfg.interpolate_duty(95.0), 100);
        // Midpoint between (50°C -> 64%) and (60°C -> 72%) must be 68%
        assert_eq!(cfg.interpolate_duty(55.0), 68);
        // Pump preset must never drop below 45%
        for pt in FanChannelConfig::silent_loop3_preset("ultra_silent") {
            assert!(pt.duty_pct >= 45);
        }
    }

    #[test]
    #[ignore = "needs real hardware (audio endpoint, HID, WMI); run with `cargo test -- --ignored`"]
    fn test_live_non_admin_hardware_selftest() {
        let rep = run_full_selftest();
        assert!(!rep.checks.is_empty());
        // WASAPI & Telemetry must always pass on Windows
        assert!(
            rep.checks
                .iter()
                .any(|c| c.name.contains("WASAPI") && c.status == "PASS"),
            "WASAPI check failed: {:?}",
            rep.checks
        );
    }
}
