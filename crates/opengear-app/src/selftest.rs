//! Automated, precise hardware self-test suite with real register/protocol read-back verification.
//! Run via CLI (`opengear selftest`), UI button ("Auto-Test Matériel"), or `cargo test`.
//! Automatically saves a detailed report to `%TEMP%\opengear_selftest.txt`.

use crate::audio_ext::read_audio_state;
use opengear_core::driver::{Control, DeviceKind, Value as DriverValue};
use opengear_core::lighting::{Placement, frame_len, render_frame};
use opengear_core::{AppConfig, RgbEffectConfig};
use opengear_mainboard::{diagnose_smbus_readonly, read_native_cpu_temp, read_superio_snapshot};
use opengear_platform::pawnio::pawnio_availability;
use opengear_platform::{
    TelemetryCollector, is_elevated, is_laptop_chassis, read_display_info, read_laptop_battery,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info, warn};

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
    let cfg_path = AppConfig::default_config_path();
    let (cfg, _) = AppConfig::load_or_recover(&cfg_path);
    let unified_drivers = crate::drivers::device_drivers();
    let mut discovered =
        crate::ui::devices::discover_all(&unified_drivers, &cfg.experimental_drivers);

    // 1. Unified Drivers & Per-Device Capabilities Audit
    if discovered.is_empty() {
        checks.push(SelfTestCheck {
            name: "Appareils pilotés (OpenGear Drivers)".into(),
            status: "WARN".into(),
            detail: "Aucun périphérique ou contrôleur compatible détecté.".into(),
        });
    } else {
        let mut drv_warns = Vec::new();
        let mut drv_summaries = Vec::new();
        for dev in &mut discovered {
            let info_d = dev.info();
            let st_d = dev.status();
            let settings = dev.settings();
            for s in &settings {
                match (&s.control, &s.value) {
                    (Control::Slider { min, max, .. }, DriverValue::Number(v))
                        if *v < *min || *v > *max =>
                    {
                        drv_warns
                            .push(format!("{}.{}: {} hors [{min}, {max}]", info_d.id, s.id, v));
                    }
                    (Control::Choice { options }, DriverValue::Text(v))
                        if !options.iter().any(|o| &o.id == v) =>
                    {
                        drv_warns.push(format!("{}.{}: choix inconnu {:?}", info_d.id, s.id, v));
                    }
                    _ => {}
                }
            }
            let zone_count = if let Some(l) = dev.lighting() {
                let zones = l.zones();
                let expected = frame_len(&zones);
                let rendered = render_frame(
                    &zones,
                    &Placement::default(),
                    &RgbEffectConfig::default(),
                    0.25,
                );
                if rendered.len() != expected {
                    drv_warns.push(format!(
                        "{}: render_frame={} != frame_len={}",
                        info_d.id,
                        rendered.len(),
                        expected
                    ));
                }
                zones.len()
            } else {
                0
            };
            let fan_count = dev.cooling().map(|c| c.fans().len()).unwrap_or(0);
            let mut caps = Vec::new();
            if let Some(pct) = st_d.battery_pct {
                caps.push(format!("Batterie={pct}%"));
            }
            if let Some(ptr) = dev.pointer().and_then(|p| p.pointer_info()) {
                caps.push(format!("DPI={}x{}", ptr.dpi_x, ptr.dpi_y));
            }
            if let Some(eq) = dev.equalizer().and_then(|e| e.eq_info()) {
                caps.push(format!("EQ={} bandes", eq.band_count));
            }
            if let Some(ob) = dev.onboard() {
                let profs = ob.onboard_profiles();
                if !profs.is_empty() {
                    let crc_bad = profs.iter().filter(|p| p.crc_valid == Some(false)).count();
                    caps.push(format!("Profils={} (CRC err={crc_bad})", profs.len()));
                }
            }
            if let Some(fc) = dev.fan_curves() {
                let (avail, msg) = fc.cooling_diagnostic();
                caps.push(format!("Courbes={} ({msg})", avail));
            }
            if zone_count > 0 {
                caps.push(format!("RGB={zone_count} zones"));
            }
            if fan_count > 0 {
                caps.push(format!("Ventilateurs={fan_count}"));
            }
            let summary_extra = if caps.is_empty() {
                format!("{} réglages", settings.len())
            } else {
                caps.join(" | ")
            };
            let check_status = if info_d.kind == DeviceKind::Motherboard
                && !admin
                && summary_extra.contains("Administrateur")
            {
                "SKIP_ADMIN"
            } else {
                "PASS"
            };
            checks.push(SelfTestCheck {
                name: format!("{} {}", info_d.vendor, info_d.model),
                status: check_status.into(),
                detail: format!(
                    "ID={} | FW={} | {summary_extra}",
                    info_d.id,
                    info_d.firmware.as_deref().unwrap_or("—")
                ),
            });
            drv_summaries.push(format!(
                "{}(settings={}, zones={}, fans={})",
                info_d.id,
                settings.len(),
                zone_count,
                fan_count
            ));
        }
        checks.push(SelfTestCheck {
            name: "Audit Drivers Unifiés & Capacités".into(),
            status: if drv_warns.is_empty() { "PASS" } else { "WARN" }.into(),
            detail: if drv_warns.is_empty() {
                format!(
                    "{} appareil(s) validé(s) : {}",
                    discovered.len(),
                    drv_summaries.join(", ")
                )
            } else {
                format!(
                    "Anomalies : {} | {}",
                    drv_warns.join(" ; "),
                    drv_summaries.join(", ")
                )
            },
        });
    }

    // 2. GPU (NVML / ADL2) & Windows WASAPI Audio
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

    // 2b. Batterie & Écran Portable (si châssis portable / batterie présente)
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

        // 8. Hardware Firmware Curves Read-Back via CurveCooling trait
        let curve_summary: Vec<String> = discovered
            .iter_mut()
            .filter_map(|d| {
                let fc = d.fan_curves()?;
                let channels = fc.curve_channels();
                if channels.is_empty() {
                    None
                } else {
                    Some(
                        channels
                            .into_iter()
                            .map(|cfg| format!("{:?}={:?}", cfg.header, cfg.policy))
                            .collect::<Vec<_>>()
                            .join(", "),
                    )
                }
            })
            .collect();
        checks.push(SelfTestCheck {
            name: "Courbes Firmware Carte Mère (CurveCooling)".into(),
            status: if !curve_summary.is_empty() {
                "PASS"
            } else {
                "WARN"
            }
            .into(),
            detail: if curve_summary.is_empty() {
                "Aucune courbe BIOS/WMI lue (SuperIO PawnIO disponible pour le contrôle live)."
                    .into()
            } else {
                curve_summary.join(" ; ")
            },
        });
    }

    // 9. Exhaustive IPC Endpoint Audit (sandboxed headless BackendEngine in %TEMP%)
    let ep_results = crate::ui::endpoint_tests::run_all_endpoint_checks();
    let ep_total = ep_results.len();
    let ep_failed: Vec<_> = ep_results.iter().filter(|r| !r.passed).collect();
    let ep_ms: u128 = ep_results.iter().map(|r| r.elapsed_ms).sum();
    if ep_failed.is_empty() {
        checks.push(SelfTestCheck {
            name: "Endpoints IPC OpenGear".into(),
            status: "PASS".into(),
            detail: format!("{ep_total}/{ep_total} suites d'endpoints vérifiées en {ep_ms} ms"),
        });
    } else {
        for f in &ep_failed {
            checks.push(SelfTestCheck {
                name: format!("Endpoint FAIL : {}", f.name),
                status: "FAIL".into(),
                detail: f.detail.clone(),
            });
        }
    }

    for c in &checks {
        match c.status.as_str() {
            "FAIL" => error!(check = %c.name, detail = %c.detail, "Auto-test FAIL"),
            "WARN" => warn!(check = %c.name, detail = %c.detail, "Auto-test WARN"),
            _ => info!(check = %c.name, status = %c.status, detail = %c.detail, "Auto-test"),
        }
    }

    let passed = checks.iter().filter(|c| c.status == "PASS").count();
    let warned = checks.iter().filter(|c| c.status == "WARN").count();
    let failed = checks.iter().filter(|c| c.status == "FAIL").count();
    let skipped_admin = checks.iter().filter(|c| c.status == "SKIP_ADMIN").count();

    let report_path = save_report_file(admin, &checks, &ep_results);

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

fn save_report_file(
    is_admin: bool,
    checks: &[SelfTestCheck],
    ep_results: &[crate::ui::endpoint_tests::EndpointCheckResult],
) -> PathBuf {
    let path = std::env::temp_dir().join("opengear_selftest.txt");
    let mut txt =
        format!("=== OpenGear Hardware & Endpoint Self-Test Report (Admin={is_admin}) ===\n");
    for c in checks {
        txt.push_str(&format!("[{:^10}] {} => {}\n", c.status, c.name, c.detail));
    }
    txt.push_str("\n=== Detail Endpoints IPC ===\n");
    for ep in ep_results {
        let tag = if ep.passed { "PASS" } else { "FAIL" };
        txt.push_str(&format!(
            "[{:^4}] {:<45} ({:>3} ms) => {}\n",
            tag, ep.name, ep.elapsed_ms, ep.detail
        ));
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
