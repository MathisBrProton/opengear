use super::backend::{
    BackendEngine, apply_validated_hw_effect, migrate_legacy_profiles,
    seed_and_resize_device_lighting,
};
#[cfg(test)]
use super::backend::{battery_alert_step, changed_vendors};
use super::{BackendMsg, MINIMIZE_TO_TRAY, UserEvent};
use crate::audio_ext::{set_endpoint_mute, set_endpoint_volume};
use crate::selftest::run_full_selftest;
use crate::system_ext::idle_duration;
use crate::tray_icon::{
    create_windows_shortcuts, is_autostart_enabled, release_single_instance, set_autostart_enabled,
};
use opengear_asus::{apply_asus_wmi_curve, read_asus_wmi_channel};
use opengear_core::cooling::{CurvePoint, FanPolicyMode, FanTempSource};
use opengear_core::driver::{Value as DriverValue, setting_ids};
use opengear_core::fans::FanTarget;
use opengear_core::lighting::{EffectRequest, LightingOverride, Placement};
use opengear_core::{
    EffectKind, KeyAction, KeyboardProfile, MacroStep, Rgb, RgbEffectConfig, SocdPair,
    scan_hid_devices,
};
use opengear_corsair::proto::{
    PROP_BRIGHTNESS_INDICATOR_COLOR, PROP_ESPORTS_BACKLIGHT_COLOR, PROP_PROFILE_INDICATOR_COLOR,
    PROP_WINLOCK_OFF_COLOR, PROP_WINLOCK_ON_COLOR,
};
use opengear_corsair::{
    CorsairDevice, key_code_for_led_index, list_corsair_backups, restore_corsair_backup,
};
use opengear_input::movement_keys_effective;
use opengear_logitech::{
    LodLevel, LogitechHeadset, LogitechMouse, MouseButtonAction, OnboardMacroStep, backup_path_for,
    list_sector_backups,
};
use opengear_mainboard::{read_superio_snapshot, restore_superio_fan_auto, set_superio_fan_pwm};
use opengear_platform::pawnio::install_official_pawnio;
use opengear_platform::{is_elevated, list_power_plans, relaunch_as_admin, set_active_power_plan};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use tao::event_loop::EventLoopProxy;

const MAX_PROFILE_IMPORT_BYTES: usize = 2 * 1024 * 1024;
const UNDO_DEPTH: usize = 20;
const POWER_PLAN_BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
const POWER_PLAN_HIGH_PERF: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const POWER_PLAN_ULTIMATE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

fn undo_label_for(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "set_rgb_config" => "Réglage de l'éclairage",
        "set_per_key_color" => "Couleur de touche",
        "clear_per_key" => "Effacement des couleurs par touche",
        "lighting_sync_all" => "Synchronisation de l'éclairage",
        "lighting_set_device" => "Éclairage par appareil",
        "lighting_resize_zone" => "Nombre de LED",
        "lighting_set_placement" => "Placement sur le bureau",
        "switch_profile" => "Changement de profil",
        "create_profile" => "Création de profil",
        "set_keyboard_layout" => "Format du clavier",
        "input_set_winlock" => "Verrou de la touche Windows",
        "input_set_binding" => "Assignation de touche",
        "input_remove_binding" => "Suppression d'assignation",
        "input_set_esports" => "Réglages e-sport",
        "profile_set_linked_apps" => "Applications liées au profil",
        "profile_import_json" => "Import de profil",
        "apply_preset" => "Préréglage",
        _ => return None,
    })
}

pub(crate) fn parse_temp_source(s: &str) -> FanTempSource {
    match s {
        "GpuLive" | "GpuEdge" | "gpu_edge" => FanTempSource::GpuEdge,
        "GpuHotspot" | "gpu_hotspot" => FanTempSource::GpuHotspot,
        "MixCpuGpu" | "MaxCpuGpu" | "max_cpu_gpu" => FanTempSource::MaxCpuGpu,
        _ => FanTempSource::CpuBios,
    }
}

pub(crate) fn is_valid_power_guid(s: &str) -> bool {
    let trimmed = s.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<&str> = trimmed.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected = [8usize, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected)
        .all(|(p, len)| p.len() == len && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(crate) fn check_import_size(raw: &str) -> std::result::Result<(), String> {
    if raw.len() > MAX_PROFILE_IMPORT_BYTES {
        return Err("Fichier profil trop volumineux (limite 2 Mo)".into());
    }
    Ok(())
}

impl BackendEngine {
    pub(crate) fn spawn_async_job(&self, id: u64, cmd: &str, tx: mpsc::Sender<BackendMsg>) {
        let cmd_owned = cmd.to_string();
        std::thread::Builder::new()
            .name(format!("opengear-async-{cmd}"))
            .spawn(move || {
                let msg = match cmd_owned.as_str() {
                    "pawnio_install" => match install_official_pawnio() {
                        Ok(ok_msg) => {
                            std::thread::sleep(std::time::Duration::from_millis(800));
                            BackendMsg::AsyncWorkerDone {
                                id,
                                action_msg: Some(ok_msg),
                                selftest: None,
                                refresh_hw: true,
                                error: None,
                            }
                        }
                        Err(e) => BackendMsg::AsyncWorkerDone {
                            id,
                            action_msg: None,
                            selftest: None,
                            refresh_hw: false,
                            error: Some(e.to_string()),
                        },
                    },
                    "run_selftest" => {
                        let rep = run_full_selftest();
                        BackendMsg::AsyncWorkerDone {
                            id,
                            action_msg: None,
                            selftest: Some(rep),
                            refresh_hw: true,
                            error: None,
                        }
                    }
                    _ => BackendMsg::AsyncWorkerDone {
                        id,
                        action_msg: None,
                        selftest: None,
                        refresh_hw: false,
                        error: Some(format!("Tâche asynchrone inconnue : {cmd_owned}")),
                    },
                };
                let _ = tx.send(msg);
            })
            .ok();
    }

    pub(crate) fn tray_toggle_socd(&mut self) {
        let new_val = {
            let prof = self.config.active_profile_mut();
            prof.socd.enabled = !prof.socd.enabled;
            prof.socd.enabled
        };
        if new_val && !self.input_engine.is_running() {
            let _ = self.input_engine.start();
        }
        self.sync_input_rules();
        self.persist_config();
    }

    pub(crate) fn tray_toggle_sync(&mut self) {
        let on = {
            let prof = self.config.active_profile_mut();
            let next = !prof.enable_rgb;
            prof.enable_rgb = next;
            for cfg in prof.device_lighting.values_mut() {
                cfg.enabled = next;
                if next {
                    cfg.mode = None;
                }
            }
            next
        };
        if !on {
            for dev in &mut self.driver_devices {
                if let Some(lighting) = dev.lighting() {
                    let _ = lighting.release();
                }
            }
            self.live_active_devices.clear();
        }
        self.last_device_frame_at.clear();
        self.persist_config();
    }

    fn corsair_mut(&mut self, p: &Value) -> std::result::Result<&mut CorsairDevice, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.typed_device_mut::<CorsairDevice>(idx)
            .ok_or_else(|| "Périphérique Corsair non connecté".to_string())
    }

    fn logi_mouse_mut(&mut self, p: &Value) -> std::result::Result<&mut LogitechMouse, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.typed_device_mut::<LogitechMouse>(idx)
            .ok_or_else(|| "Souris Logitech non connectée".to_string())
    }

    fn logi_headset_mut(&mut self, p: &Value) -> std::result::Result<&mut LogitechHeadset, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.typed_device_mut::<LogitechHeadset>(idx)
            .ok_or_else(|| "Casque Logitech non connecté".to_string())
    }

    pub(crate) fn handle_cmd(
        &mut self,
        cmd: &str,
        p: &Value,
        proxy: &EventLoopProxy<UserEvent>,
    ) -> std::result::Result<Value, String> {
        let record_undo = match cmd {
            "set_rgb_config" => p["commit"].as_bool().unwrap_or(true),
            _ => true,
        };
        let Some(label) = (if record_undo {
            undo_label_for(cmd)
        } else {
            None
        }) else {
            return self.handle_cmd_inner(cmd, p, proxy);
        };
        let before = self.config.clone();
        let before_json = serde_json::to_string(&before).unwrap_or_default();
        let mut res = self.handle_cmd_inner(cmd, p, proxy)?;
        let after_json = serde_json::to_string(&self.config).unwrap_or_default();
        if before_json != after_json {
            self.undo_stack.push((label.to_string(), before));
            if self.undo_stack.len() > UNDO_DEPTH {
                self.undo_stack.remove(0);
            }
            if let Some(ui) = res.get_mut("ui") {
                ui["can_undo"] = json!(true);
                ui["undo_label"] = json!(label);
            } else if let Some(ui) = res.get_mut("state").and_then(|s| s.get_mut("ui")) {
                ui["can_undo"] = json!(true);
                ui["undo_label"] = json!(label);
            }
        }
        Ok(res)
    }

    fn apply_preset(&mut self, preset: &str) -> std::result::Result<Vec<String>, String> {
        let (kind, primary, brightness, fan_policy, plans): (
            EffectKind,
            Rgb,
            f32,
            Option<FanPolicyMode>,
            &[&str],
        ) = match preset {
            "silent" => (
                EffectKind::Breathing,
                Rgb::new(40, 90, 255),
                0.35,
                Some(FanPolicyMode::Silent),
                &[POWER_PLAN_BALANCED],
            ),
            "gaming" => (
                EffectKind::RainbowWave,
                Rgb::CYAN,
                1.0,
                Some(FanPolicyMode::Turbo),
                &[POWER_PLAN_HIGH_PERF, POWER_PLAN_ULTIMATE],
            ),
            "office" => (
                EffectKind::Static,
                Rgb::new(235, 240, 255),
                0.6,
                Some(FanPolicyMode::Standard),
                &[POWER_PLAN_BALANCED],
            ),
            "lights_off" => (EffectKind::Off, Rgb::BLACK, 0.0, None, &[]),
            _ => return Err(format!("Préréglage inconnu : {preset}")),
        };
        let mut summary = Vec::new();

        {
            let p = self.config.active_profile_mut();
            p.enable_rgb = true;
            p.rgb.kind = kind;
            p.rgb.primary = primary;
            p.rgb.brightness = brightness;
            for cfg in p.device_lighting.values_mut() {
                cfg.enabled = true;
                cfg.mode = None;
                cfg.zones.clear();
            }
        }
        self.last_device_frame_at.clear();
        summary.push(format!("Éclairage : {}", kind.label()));

        // Devices with a performance profile (laptop BIOS modes…) follow the preset.
        let profile_devices: Vec<(String, String, String)> = self
            .driver_devices
            .iter()
            .filter_map(|d| {
                let setting = d
                    .settings()
                    .into_iter()
                    .find(|s| s.id == setting_ids::PERFORMANCE_PROFILE)?;
                Some((d.info().id, d.info().model, setting.label))
            })
            .collect();
        if let Some(policy) = fan_policy {
            if !profile_devices.is_empty() {
                let target = match preset {
                    "silent" => setting_ids::PROFILE_QUIET,
                    "gaming" => setting_ids::PROFILE_PERFORMANCE,
                    _ => setting_ids::PROFILE_BALANCED,
                };
                for (id, model, label) in profile_devices {
                    match self.set_device_setting(
                        &id,
                        setting_ids::PERFORMANCE_PROFILE,
                        &DriverValue::Text(target.into()),
                    ) {
                        Ok(_) => summary.push(format!("{label} ({model}) : {target}")),
                        Err(e) => summary.push(format!("{label} ({model}) inchangé ({e})")),
                    }
                }
            } else if self.cooling_status.wmi_available && is_elevated() {
                let mut applied = 0usize;
                let mut failed = 0usize;
                for ch in self.cooling_channels.iter_mut() {
                    if ch.source != FanTempSource::CpuBios {
                        continue;
                    }
                    let mut cfg = ch.clone();
                    cfg.policy = policy;
                    match apply_asus_wmi_curve(&cfg) {
                        Ok(_) => {
                            ch.policy = policy;
                            applied += 1;
                        }
                        Err(_) => failed += 1,
                    }
                }
                summary.push(format!(
                    "Ventilateurs : profil {} sur {applied} canal(aux){}",
                    policy.label(),
                    if failed > 0 {
                        format!(", {failed} refusé(s) par le BIOS")
                    } else {
                        String::new()
                    }
                ));
            } else {
                summary.push(
                    "Ventilateurs inchangés (droits administrateur + ASUS WMI requis)".into(),
                );
            }
        }

        if !plans.is_empty() {
            let target = plans.iter().find_map(|g| {
                self.power_plans
                    .iter()
                    .find(|p| p.guid.eq_ignore_ascii_case(g))
                    .map(|p| (p.guid.clone(), p.name.clone()))
            });
            match target {
                Some((guid, name)) => match set_active_power_plan(&guid) {
                    Ok(_) => {
                        self.power_plans = list_power_plans();
                        summary.push(format!("Alimentation : {name}"));
                    }
                    Err(e) => summary.push(format!("Alimentation inchangée ({e})")),
                },
                None => summary.push("Alimentation inchangée (plan absent de ce PC)".into()),
            }
        }
        if preset == "lights_off" {
            let lit: Vec<String> = self
                .driver_devices
                .iter()
                .filter(|d| {
                    d.settings().iter().any(|s| {
                        s.id == setting_ids::BACKLIGHT && s.value == DriverValue::Bool(true)
                    })
                })
                .map(|d| d.info().id)
                .collect();
            for id in lit {
                let _ =
                    self.set_device_setting(&id, setting_ids::BACKLIGHT, &DriverValue::Bool(false));
            }
        }
        self.persist_config();
        Ok(summary)
    }

    fn diagnostics_json(&mut self) -> Value {
        let mut checks: Vec<Value> = Vec::new();
        let mut push = |id: &str, label: &str, status: &str, detail: String, action: Value| {
            checks.push(json!({ "id": id, "label": label, "status": status, "detail": detail, "action": action }));
        };

        let admin = is_elevated();
        push(
            "admin",
            "Droits administrateur",
            if admin { "ok" } else { "warn" },
            if admin {
                "OpenGear tourne en administrateur : ventilateurs, RGB de la RAM et capteurs avancés sont disponibles.".into()
            } else {
                "Sans droits administrateur, le contrôle des ventilateurs, le RGB de la RAM et certains capteurs sont désactivés.".into()
            },
            if admin {
                Value::Null
            } else {
                json!({ "label": "Relancer en administrateur", "cmd": "cooling_relaunch_admin", "payload": {} })
            },
        );

        let (st, detail, action) = match opengear_platform::pawnio::pawnio_availability() {
            opengear_platform::PawnIoAvailability::Ready => ("ok", "Pilote signé PawnIO actif (capteurs CPU, SMBus, SuperIO).".to_string(), Value::Null),
            opengear_platform::PawnIoAvailability::NeedsAdmin => (
                "warn",
                "Le pilote PawnIO est installé mais nécessite les droits administrateur.".to_string(),
                Value::Null,
            ),
            opengear_platform::PawnIoAvailability::NotInstalled => (
                "warn",
                "Le pilote signé PawnIO n'est pas installé : température CPU native, RAM RGB et ventilateurs directs indisponibles.".to_string(),
                json!({ "label": "Installer PawnIO", "cmd": "pawnio_install", "payload": {} }),
            ),
        };
        push("pawnio", "Pilote matériel PawnIO", st, detail, action);

        if let Some(model) = super::backend::device_cooling_owner(&mut self.driver_devices) {
            push(
                "device_cooling",
                "Ventilateurs de l'appareil",
                "ok",
                format!("Ventilateurs pilotés par « {model} »."),
                Value::Null,
            );
        } else {
            push(
                "asus_wmi",
                "Interface ventilateurs ASUS (WMI)",
                if self.cooling_status.wmi_available {
                    "ok"
                } else {
                    "warn"
                },
                self.cooling_status.message.clone(),
                Value::Null,
            );
        }

        let sensors = self.latest_sensors();
        push(
            "sensors",
            "Capteurs de température",
            if sensors.available { "ok" } else { "warn" },
            if sensors.available {
                format!("Source : {}", sensors.source)
            } else {
                "Aucune source de capteurs : installe LibreHardwareMonitor ou PawnIO pour les températures.".into()
            },
            Value::Null,
        );

        let n_dev = self.driver_devices.len();
        push(
            "devices",
            "Appareils compatibles",
            if n_dev > 0 { "ok" } else { "warn" },
            if n_dev > 0 {
                format!("{n_dev} appareil(s) piloté(s).")
            } else {
                "Aucun appareil compatible détecté. Vérifie les branchements et ferme iCUE / G HUB / Armoury Crate.".into()
            },
            json!({ "label": "Relancer la détection", "cmd": "reconnect_all", "payload": {} }),
        );

        if !self.is_laptop
            && self
                .typed_devices::<opengear_asus::AuraMainboardController>()
                .is_empty()
        {
            push(
                "aura",
                "Contrôleur ASUS Aura",
                "warn",
                "Aucun contrôleur ASUS Aura USB détecté (ou utilisé par Armoury Crate).".into(),
                Value::Null,
            );
        }

        let needs_engine = {
            let p = self.config.active_profile_mut();
            p.socd.enabled
                || p.key_chatter_ms > 0
                || p.mouse_debounce_ms > 0
                || p.win_lock
                || !p.bindings.is_empty()
        };
        let engine_ok = self.input_engine.is_running();
        push(
            "input",
            "Moteur clavier (remaps, macros, SOCD)",
            if !needs_engine || engine_ok {
                "ok"
            } else {
                "error"
            },
            if engine_ok {
                "Actif.".into()
            } else if needs_engine {
                "Des assignations sont configurées mais le moteur clavier ne tourne pas.".into()
            } else {
                "Inactif (aucune assignation configurée).".into()
            },
            if needs_engine && !engine_ok {
                json!({ "label": "Démarrer le moteur", "cmd": "input_set_enabled", "payload": { "enabled": true } })
            } else {
                Value::Null
            },
        );

        match &self.config_warning {
            Some(w) => push(
                "config",
                "Fichier de profils",
                "error",
                w.clone(),
                Value::Null,
            ),
            None => push(
                "config",
                "Fichier de profils",
                "ok",
                format!("Profils enregistrés dans {}", self.config_path.display()),
                Value::Null,
            ),
        }

        json!({ "checks": checks })
    }

    fn diagnostic_report(&mut self) -> String {
        use std::fmt::Write as _;
        let mut r = String::new();
        let s = &self.telemetry.static_info;
        let _ = writeln!(
            r,
            "=== Rapport de diagnostic OpenGear {} ===",
            env!("CARGO_PKG_VERSION")
        );
        let _ = writeln!(
            r,
            "Carte mère : {} (BIOS {})",
            s.motherboard, s.bios_version
        );
        let _ = writeln!(
            r,
            "CPU : {} | GPU : {} | RAM : {:.0} Go",
            s.cpu_name, s.gpu_name, s.ram_total_gb
        );
        let _ = writeln!(
            r,
            "Administrateur : {} | Mode UI : {}",
            is_elevated(),
            self.config.ui_mode
        );
        let _ = writeln!(r, "\n--- Appareils pilotés ---");
        for dev in &self.driver_devices {
            let info = dev.info();
            let st = dev.status();
            let _ = writeln!(
                r,
                "{} {} ({}, {:?}, {:?}) FW {}",
                info.vendor,
                info.model,
                info.id,
                info.kind,
                info.connection,
                info.firmware.as_deref().unwrap_or("—"),
            );
            if let Some(pct) = st.battery_pct {
                let _ = writeln!(r, "  Batterie : {pct}% (en charge={:?})", st.charging);
            }
            if let Some(c) = dev.as_any().and_then(|a| a.downcast_ref::<CorsairDevice>()) {
                let _ = writeln!(
                    r,
                    "  Corsair Bragi : report {} o | FlashTap 0xF5 = {}",
                    c.info.out_report_len, c.info.flashtap_probe
                );
            }
            for line in dev.diagnostic_lines() {
                let _ = writeln!(r, "  {line}");
            }
        }
        if self.laptop_battery.present {
            let b = &self.laptop_battery;
            let _ = writeln!(
                r,
                "Portable : Bat={:?}% (Santé={:?}%, Cycles={:?}, Secteur={})",
                b.percent, b.health_pct, b.cycle_count, b.ac_online,
            );
        }
        let (socd_n, inj_ok, inj_fail) = self.input_engine.injection_stats();
        let _ = writeln!(
            r,
            "Moteur clavier : actif={} | Snap Tap résolutions={} | injections OK={} refusées={}",
            self.input_engine.is_running(),
            socd_n,
            inj_ok,
            inj_fail
        );
        let _ = writeln!(r, "\n--- Diagnostic ---");
        if let Some(checks) = self.diagnostics_json()["checks"].as_array() {
            for c in checks {
                let _ = writeln!(
                    r,
                    "[{}] {} — {}",
                    c["status"].as_str().unwrap_or("?").to_uppercase(),
                    c["label"].as_str().unwrap_or(""),
                    c["detail"].as_str().unwrap_or("")
                );
            }
        }
        let _ = writeln!(
            r,
            "\n--- Fin du journal ({}) ---",
            crate::app_log::log_path().display()
        );
        let _ = writeln!(r, "{}", crate::app_log::tail(60));
        mask_user_profile(&r)
    }

    fn handle_cmd_inner(
        &mut self,
        cmd: &str,
        p: &Value,
        proxy: &EventLoopProxy<UserEvent>,
    ) -> std::result::Result<Value, String> {
        match cmd {
            "get_state" => Ok(self.full_state_json()),
            "get_catalog" => Ok(Self::catalog_json()),
            "get_telemetry" => Ok(self.telemetry_json()),
            "get_diagnostics" => Ok(self.diagnostics_json()),
            "get_diagnostic_report" => Ok(json!({ "text": self.diagnostic_report() })),
            "open_test_window" => {
                #[cfg(windows)]
                {
                    let sys_root =
                        std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
                    let notepad = PathBuf::from(sys_root).join("System32").join("notepad.exe");
                    let path = std::env::temp_dir().join("opengear_test.txt");
                    let _ = std::fs::write(
                        &path,
                        "OpenGear — fenêtre de test\r\n\r\nClique ici pour taper, puis regarde les pastilles dans OpenGear.\r\n",
                    );
                    std::process::Command::new(notepad)
                        .arg(&path)
                        .spawn()
                        .map_err(|e| format!("Impossible d'ouvrir le Bloc-notes : {e}"))?;
                    Ok(json!({}))
                }
                #[cfg(not(windows))]
                Err("Windows uniquement".into())
            }
            "input_socd_live" => {
                let [left, right, forward, back] = movement_keys_effective();
                Ok(json!({
                    "engine_running": self.input_engine.is_running(),
                    "left": left,
                    "right": right,
                    "forward": forward,
                    "back": back,
                    "idle_ms": idle_duration().map(|d| d.as_millis() as u64),
                }))
            }
            "set_ui_prefs" => {
                if let Some(m) = p["ui_mode"].as_str()
                    && (m == "simple" || m == "expert")
                {
                    self.config.ui_mode = m.to_string();
                }
                if let Some(done) = p["onboarding_done"].as_bool() {
                    self.config.onboarding_done = done;
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "apply_preset" => {
                let preset = p["preset"].as_str().ok_or("Préréglage manquant")?;
                let summary = self.apply_preset(preset)?;
                let mut st = self.full_state_json();
                st["last_action_msg"] =
                    json!(format!("Préréglage appliqué — {}", summary.join(" · ")));
                Ok(st)
            }
            "undo_last" => {
                let Some((label, prev)) = self.undo_stack.pop() else {
                    return Err("Rien à annuler".into());
                };
                self.config = prev;
                self.apply_restored_config();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(format!("Annulé : {label}"));
                Ok(st)
            }
            "list_backups" => Ok(json!({ "backups": list_sector_backups() })),
            "logi_restore_backup" => {
                let file = p["file"].as_str().ok_or("Fichier de sauvegarde manquant")?;
                let path = backup_path_for(file).ok_or("Sauvegarde introuvable ou nom invalide")?;
                let m = self.logi_mouse_mut(p)?;
                m.restore_onboard_backup(&path).map_err(|e| e.to_string())?;
                m.refresh_telemetry();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(format!("Sauvegarde {file} restaurée et vérifiée"));
                Ok(st)
            }
            "corsair_list_backups" => Ok(json!({ "backups": list_corsair_backups() })),
            "corsair_restore_backup" => {
                let file = p["file"].as_str().ok_or("Fichier de sauvegarde manquant")?;
                let dev = self.corsair_mut(p)?;
                let file_id = dev
                    .with_flash_session(|t| restore_corsair_backup(t, file))
                    .map_err(|e| e.to_string())?;
                let _ = dev.reload_hwp_profiles();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(format!(
                    "Sauvegarde Corsair {file} restaurée (fichier HWP 0x{file_id:04X})"
                ));
                Ok(st)
            }
            "device_set" => {
                let device = p["device"].as_str().ok_or("device manquant")?;
                let setting = p["setting"].as_str().ok_or("setting manquant")?;
                let value: opengear_core::driver::Value =
                    serde_json::from_value(p["value"].clone()).map_err(|_| "valeur invalide")?;
                let msg = self.set_device_setting(device, setting, &value)?;
                let mut st = self.full_state_json();
                if let Some(m) = msg {
                    st["last_action_msg"] = json!(m);
                }
                Ok(st)
            }
            "set_experimental_driver" => {
                let name = p["name"].as_str().ok_or("name manquant")?;
                let enabled = p["enabled"].as_bool().ok_or("enabled manquant")?;
                self.set_experimental_driver(name, enabled)?;
                Ok(self.full_state_json())
            }
            "reconnect_all" => {
                self.reconnect_all();
                Ok(self.full_state_json())
            }
            "scan_hid" => {
                let all = p["all"].as_bool().unwrap_or(false);
                self.scanned_devices = scan_hid_devices(all).unwrap_or_default();
                Ok(self.full_state_json())
            }
            "save_config" => {
                self.config
                    .save(&self.config_path)
                    .map_err(|e| e.to_string())?;
                Ok(
                    json!({ "message": format!("Profils sauvegardés dans {}", self.config_path.display()) }),
                )
            }
            "switch_profile" => {
                let name = p["name"].as_str().ok_or("Nom de profil manquant")?;
                if self.config.profiles.iter().any(|pr| pr.name == name) {
                    self.config.active_profile = name.to_string();
                    self.manual_profile_name = name.to_string();
                    self.auto_switched = false;
                    self.sync_input_rules();
                    self.persist_config();
                }
                Ok(self.full_state_json())
            }
            "create_profile" => {
                let name = p["name"].as_str().unwrap_or("").trim();
                if name.is_empty() {
                    return Err("Nom de profil vide".into());
                }
                let mut cloned = self.config.active_profile_mut().clone();
                cloned.name = name.to_string();
                self.config.profiles.push(cloned);
                self.config.active_profile = name.to_string();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "set_keyboard_layout" => {
                if let Some(f) = p["form_factor"].as_str() {
                    self.config.keyboard_form_factor = opengear_core::FormFactor::from_id(f);
                }
                if let Some(s) = p["standard"].as_str() {
                    self.config.keyboard_standard = super::keyboard_view::parse_standard(s);
                }
                self.invalidate_keyboard_view();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "set_power_plan" => {
                let guid = p["guid"].as_str().ok_or("GUID manquant")?;
                if !is_valid_power_guid(guid) {
                    return Err(format!("GUID de plan d'alimentation invalide : {guid}"));
                }
                set_active_power_plan(guid).map_err(|e| e.to_string())?;
                self.power_plans = list_power_plans();
                Ok(self.full_state_json())
            }
            "lighting_sync_all" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                {
                    let prof = self.config.active_profile_mut();
                    prof.enable_rgb = on;
                    for d in &self.driver_devices {
                        let id = d.info().id;
                        let entry = prof.device_lighting.entry(id).or_default();
                        entry.enabled = on;
                        if on {
                            entry.mode = None;
                        }
                    }
                }
                if !on {
                    for d in &mut self.driver_devices {
                        if let Some(lighting) = d.lighting() {
                            let _ = lighting.release();
                        }
                    }
                    self.live_active_devices.clear();
                }
                self.last_device_frame_at.clear();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "lighting_set_device" => {
                let dev_id = p["device_id"].as_str().ok_or("device_id manquant")?;
                let zone_id = p["zone_id"].as_str().map(str::to_string);
                let enabled_arg = p["enabled"].as_bool();
                let parsed_override = if p.get("override").is_some_and(|v| !v.is_null()) {
                    Some(parse_lighting_override(&p["override"])?)
                } else {
                    None
                };

                if let Some(LightingOverride::Hardware { ref request }) = parsed_override {
                    let mut req = request.clone();
                    if req.zone.is_none() {
                        req.zone = zone_id.clone();
                    }
                    let dev = self
                        .device_mut(dev_id)
                        .ok_or_else(|| format!("Appareil introuvable : {dev_id}"))?;
                    let lighting = dev
                        .lighting()
                        .ok_or_else(|| format!("Éclairage non supporté sur : {dev_id}"))?;
                    apply_validated_hw_effect(lighting, &req).map_err(|e| e.to_string())?;
                } else if matches!(parsed_override, Some(LightingOverride::Off)) {
                    let dev = self
                        .device_mut(dev_id)
                        .ok_or_else(|| format!("Appareil introuvable : {dev_id}"))?;
                    if zone_id.is_none()
                        && let Some(lighting) = dev.lighting()
                    {
                        let len = opengear_core::lighting::frame_len(&lighting.zones());
                        let _ = lighting.set_frame(&vec![Rgb::BLACK; len]);
                    }
                } else if enabled_arg == Some(false)
                    && let Some(dev) = self.device_mut(dev_id)
                    && let Some(lighting) = dev.lighting()
                {
                    let _ = lighting.release();
                }

                {
                    let prof = self.config.active_profile_mut();
                    let entry = prof.device_lighting.entry(dev_id.to_string()).or_default();
                    if let Some(en) = enabled_arg {
                        entry.enabled = en;
                    } else if parsed_override.is_some()
                        && zone_id.is_none()
                        && !matches!(parsed_override, Some(LightingOverride::Hardware { .. }))
                    {
                        entry.enabled = true;
                    }
                    match (zone_id.as_deref(), parsed_override) {
                        (Some(zid), Some(ov)) => {
                            if matches!(ov, LightingOverride::Global) {
                                entry.zones.remove(zid);
                            } else {
                                entry.zones.insert(zid.to_string(), ov);
                            }
                            if !entry.zones.is_empty() && enabled_arg.is_none() {
                                entry.enabled = true;
                            }
                        }
                        (Some(zid), None) if p.get("override").is_some() => {
                            entry.zones.remove(zid);
                        }
                        (None, Some(ov)) => {
                            if matches!(ov, LightingOverride::Hardware { .. })
                                && enabled_arg.is_none()
                            {
                                entry.enabled = false;
                            }
                            if matches!(ov, LightingOverride::Global) {
                                entry.mode = None;
                            } else {
                                entry.mode = Some(ov);
                            }
                        }
                        (None, None) if p.get("override").is_some() => {
                            entry.mode = None;
                        }
                        _ => {}
                    }
                }
                self.last_device_frame_at.remove(dev_id);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "lighting_resize_zone" => {
                let dev_id = p["device_id"].as_str().ok_or("device_id manquant")?;
                let zone_id = p["zone_id"].as_str().ok_or("zone_id manquant")?;
                let count = (p["led_count"].as_u64().ok_or("led_count manquant")?)
                    .min(u16::MAX as u64) as u16;
                let clamped = {
                    let dev = self
                        .device_mut(dev_id)
                        .ok_or_else(|| format!("Appareil introuvable : {dev_id}"))?;
                    let lighting = dev
                        .lighting()
                        .ok_or_else(|| format!("Éclairage non supporté sur : {dev_id}"))?;
                    let range = lighting
                        .zones()
                        .into_iter()
                        .find(|z| z.id == zone_id)
                        .and_then(|z| z.resizable)
                        .ok_or_else(|| format!("Zone non redimensionnable : {zone_id}"))?;
                    let target = count.clamp(range.0, range.1);
                    lighting
                        .resize_zone(zone_id, target)
                        .map_err(|e| e.to_string())?;
                    target
                };
                self.config
                    .active_profile_mut()
                    .device_lighting
                    .entry(dev_id.to_string())
                    .or_default()
                    .zone_led_counts
                    .insert(zone_id.to_string(), clamped);
                self.last_device_frame_at.remove(dev_id);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "lighting_set_placement" => {
                let dev_id = p["device_id"].as_str().ok_or("device_id manquant")?;
                let x = p["x"].as_f64().unwrap_or(0.0) as f32;
                let y = p["y"].as_f64().unwrap_or(0.0) as f32;
                let w = p["width"].as_f64().unwrap_or(1.0) as f32;
                let h = p["height"].as_f64().unwrap_or(1.0) as f32;
                let xc = x.clamp(0.0, 0.95);
                let yc = y.clamp(0.0, 0.95);
                let wc = w.clamp(0.05, 1.0 - xc);
                let hc = h.clamp(0.05, 1.0 - yc);
                self.config.set_placement(
                    dev_id,
                    Placement {
                        x: xc,
                        y: yc,
                        width: wc,
                        height: hc,
                    },
                );
                self.last_device_frame_at.remove(dev_id);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "set_rgb_config" => {
                let commit = p["commit"].as_bool().unwrap_or(true);
                let en_now = {
                    let prof = self.config.active_profile_mut();
                    if let Some(en) = p["enable_rgb"].as_bool() {
                        prof.enable_rgb = en;
                    }
                    if let Some(k) = p["kind"].as_str()
                        && let Some(ek) = parse_effect_kind(k)
                    {
                        prof.rgb.kind = ek;
                    }
                    if let Some(hex) = p["primary"].as_str()
                        && let Some(c) = Rgb::from_hex(hex)
                    {
                        prof.rgb.primary = c;
                    }
                    if let Some(hex) = p["secondary"].as_str()
                        && let Some(c) = Rgb::from_hex(hex)
                    {
                        prof.rgb.secondary = c;
                    }
                    if let Some(sp) = p["speed"].as_f64() {
                        prof.rgb.speed = (sp as f32).clamp(0.1, 5.0);
                    }
                    if let Some(br) = p["brightness"].as_f64() {
                        prof.rgb.brightness = (br as f32).clamp(0.0, 1.0);
                    }
                    if let Some(rev) = p["reverse"].as_bool() {
                        prof.rgb.reverse = rev;
                    }
                    prof.enable_rgb
                };
                if !en_now {
                    for dev in &mut self.driver_devices {
                        if let Some(lighting) = dev.lighting() {
                            let _ = lighting.release();
                        }
                    }
                    self.live_active_devices.clear();
                }
                self.last_device_frame_at.clear();
                if commit {
                    self.persist_config();
                }
                Ok(self.full_state_json())
            }
            "set_per_key_color" => {
                let key_code = if let Some(code) = p["key_code"].as_str() {
                    code.to_string()
                } else if let Some(idx) = p["led_index"].as_u64() {
                    key_code_for_led_index(0x1B1C, 0x1B73, idx as usize)
                        .ok_or("Index LED inconnu sans code de touche")?
                        .to_string()
                } else {
                    return Err("key_code manquant".into());
                };
                let hex = p["color"].as_str().ok_or("color manquant")?;
                let col = Rgb::from_hex(hex).ok_or("Couleur hex invalide")?;
                let prof = self.config.active_profile_mut();
                prof.rgb.kind = EffectKind::PerKey;
                prof.rgb.per_key.insert(key_code, col);
                self.last_device_frame_at.clear();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "clear_per_key" => {
                self.config.active_profile_mut().rgb.per_key.clear();
                self.last_device_frame_at.clear();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "corsair_set_indicator_color" => {
                let which = p["indicator"].as_str().unwrap_or("esports");
                let hex = p["color"].as_str().ok_or("Couleur manquante")?;
                let col = Rgb::from_hex(hex).ok_or("Couleur hex invalide")?;
                let prop = match which {
                    "winlock_on" => PROP_WINLOCK_ON_COLOR,
                    "winlock_off" => PROP_WINLOCK_OFF_COLOR,
                    "profile" => PROP_PROFILE_INDICATOR_COLOR,
                    "brightness" => PROP_BRIGHTNESS_INDICATOR_COLOR,
                    _ => PROP_ESPORTS_BACKLIGHT_COLOR,
                };
                let dev = self.corsair_mut(p)?;
                dev.set_indicator_color(prop, col)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_mouse_set_dpi_stage" => {
                let stage = p["stage"].as_u64().unwrap_or(0) as u8;
                let dpi = p["dpi"].as_u64().unwrap_or(800) as u16;
                let dev = self.corsair_mut(p)?;
                dev.set_mouse_dpi_stage(stage, dpi)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_mouse_set_active_stage" => {
                let stage = p["stage"].as_u64().unwrap_or(0) as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_mouse_active_stage(stage)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_set_dpi" => {
                let x = p["x"].as_u64().ok_or("DPI X manquant")? as u16;
                let y = p["y"].as_u64().map(|v| v as u16);
                let lod = p["lod"].as_str().map(|s| match s {
                    "Low" => LodLevel::Low,
                    "High" => LodLevel::High,
                    _ => LodLevel::Medium,
                });
                let m = self.logi_mouse_mut(p)?;
                m.set_dpi(x, y, lod).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_set_active_profile" => {
                let slot = p["slot"].as_u64().ok_or("slot manquant")? as u16;
                let m = self.logi_mouse_mut(p)?;
                m.set_active_profile(slot).map_err(|e| e.to_string())?;
                m.refresh_telemetry();
                Ok(self.full_state_json())
            }
            "logi_set_dpi_stage" => {
                let stage = p["stage"].as_u64().ok_or("stage manquant")? as u8;
                let m = self.logi_mouse_mut(p)?;
                m.set_active_dpi_stage(stage).map_err(|e| e.to_string())?;
                m.refresh_telemetry();
                Ok(self.full_state_json())
            }
            "logi_reload_profiles" => {
                let m = self.logi_mouse_mut(p)?;
                m.reload_onboard_profiles().map_err(|e| e.to_string())?;
                m.refresh_telemetry();
                Ok(self.full_state_json())
            }
            "logi_flash_profile" => {
                let idx = p["index"].as_u64().ok_or("index manquant")? as usize;
                let m = self.logi_mouse_mut(p)?;
                let prof = m
                    .info
                    .onboard_profiles
                    .get_mut(idx)
                    .ok_or("Index profil invalide")?;

                if let Some(hz) = p["report_rate_hz"].as_u64() {
                    prof.report_rate_hz = hz as u16;
                }
                if let Some(def) = p["default_dpi_idx"].as_u64() {
                    prof.default_dpi_idx = (def as u8).min(4);
                }
                if let Some(arr) = p["dpi_stages"].as_array() {
                    for (i, v) in arr.iter().take(5).enumerate() {
                        if let Some(dpi) = v.as_u64() {
                            prof.dpi_stages[i] = (dpi as u16).clamp(100, 32000);
                        }
                    }
                }
                if let Some(btns) = p["buttons"].as_array() {
                    for (i, b) in btns.iter().enumerate() {
                        if i < prof.buttons.len()
                            && let Some(hex) = b.as_str()
                            && let Some(raw) = parse_4byte_hex(hex)
                        {
                            prof.buttons[i] = MouseButtonAction::decode(raw);
                        }
                    }
                }
                m.flash_onboard_profile(idx).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_flash_macro" => {
                let prof_idx = p["profile_index"].as_u64().unwrap_or(0) as usize;
                let btn_idx = p["button_index"].as_u64().unwrap_or(3) as usize;
                let steps_arr = p["steps"].as_array().ok_or("steps manquants")?;
                let mut steps = Vec::new();
                for s in steps_arr {
                    let kind = s["kind"].as_str().unwrap_or("");
                    let val = s["value"].as_u64().unwrap_or(0) as u16;
                    match kind {
                        "key_down" => steps.push(OnboardMacroStep::KeyPress(val as u8)),
                        "key_up" => steps.push(OnboardMacroStep::KeyRelease(val as u8)),
                        "mouse_down" => steps.push(OnboardMacroStep::MousePress(val as u8)),
                        "mouse_up" => steps.push(OnboardMacroStep::MouseRelease(val as u8)),
                        "delay" => steps.push(OnboardMacroStep::DelayMs(val)),
                        _ => {}
                    }
                }
                let m = self.logi_mouse_mut(p)?;
                m.flash_onboard_macro(prof_idx, btn_idx, &steps)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_set_eq" => {
                let arr = p["gains"].as_array().ok_or("gains manquants")?;
                let gains: Vec<i8> = arr
                    .iter()
                    .map(|v| v.as_i64().unwrap_or(0).clamp(-12, 12) as i8)
                    .collect();
                let h = self.logi_headset_mut(p)?;
                h.set_equalizer_gains(&gains).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "cooling_read_bios_curves" => {
                if !self.cooling_status.wmi_available {
                    return Err(
                        "Droits Admin + ASUSManagement WMI requis pour relire les courbes BIOS"
                            .into(),
                    );
                }
                self.cooling_channels = opengear_core::cooling::FanHeaderKind::ALL
                    .iter()
                    .map(|&h| {
                        read_asus_wmi_channel(h).unwrap_or_else(|| {
                            opengear_core::cooling::FanChannelConfig::default_for(h)
                        })
                    })
                    .collect();
                Ok(self.full_state_json())
            }
            "cooling_update_channel" => {
                let idx = p["index"].as_u64().ok_or("index manquant")? as usize;
                let apply = p["apply"].as_bool().unwrap_or(false);
                let ch = self
                    .cooling_channels
                    .get_mut(idx)
                    .ok_or("Canal PWM invalide")?;

                if let Some(pol) = p["policy"].as_str() {
                    ch.policy = match pol {
                        "Silent" => FanPolicyMode::Silent,
                        "Standard" => FanPolicyMode::Standard,
                        "Turbo" => FanPolicyMode::Turbo,
                        "FullSpeed" => FanPolicyMode::FullSpeed,
                        _ => FanPolicyMode::Manual,
                    };
                }
                if let Some(pts) = p["curve"].as_array() {
                    for (i, item) in pts.iter().take(8).enumerate() {
                        let t = item["temp_c"].as_u64().unwrap_or(50).clamp(20, 100) as u8;
                        let d = item["duty_pct"].as_u64().unwrap_or(50).clamp(0, 100) as u8;
                        ch.curve[i] = CurvePoint {
                            temp_c: t,
                            duty_pct: d,
                        };
                    }
                }
                if let Some(src) = p["source"].as_str() {
                    ch.source = parse_temp_source(src);
                    if idx < 5 {
                        self.smart_fan_last_duty[idx] = None;
                    }
                }
                if let Some(h) = p["hysteresis_c"].as_u64() {
                    ch.hysteresis_c = (h as u8).min(12);
                }
                if apply && ch.source == FanTempSource::CpuBios {
                    apply_asus_wmi_curve(ch).map_err(|e| e.to_string())?;
                }
                Ok(self.full_state_json())
            }
            "cooling_sl3_preset" => {
                let preset = p["preset"].as_str().unwrap_or("default");
                if let Some(ch) = self.cooling_channels.get_mut(0) {
                    ch.policy = FanPolicyMode::Manual;
                    ch.curve =
                        opengear_core::cooling::FanChannelConfig::silent_loop3_preset(preset);
                    if self.cooling_status.is_admin {
                        let _ = apply_asus_wmi_curve(ch);
                    }
                }
                Ok(self.full_state_json())
            }
            "cooling_relaunch_admin" => {
                release_single_instance();
                relaunch_as_admin().map_err(|e| e.to_string())?;
                let _ = proxy.send_event(UserEvent::Quit);
                Ok(json!({ "relaunching": true }))
            }
            "input_set_enabled" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                if on {
                    self.input_engine.start().map_err(|e| e.to_string())?;
                    self.sync_input_rules();
                } else {
                    self.input_engine.stop();
                }
                Ok(self.full_state_json())
            }
            "input_set_winlock" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                self.config.active_profile_mut().win_lock = on;
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "input_set_binding" => {
                let key = p["key"].as_str().ok_or("key manquante")?.to_string();
                let kind = p["kind"].as_str().ok_or("kind manquant")?;
                let action = match kind {
                    "remap" => KeyAction::Remap {
                        target: p["target"].as_str().unwrap_or("F13").to_string(),
                    },
                    "disable" => KeyAction::Disable,
                    "text" => KeyAction::Text {
                        text: p["text"].as_str().unwrap_or("").to_string(),
                        delay_ms: p["delay_ms"].as_u64().unwrap_or(15) as u32,
                    },
                    "macro" => {
                        let name = p["name"].as_str().unwrap_or("Macro").to_string();
                        let steps: Vec<MacroStep> = serde_json::from_value(p["steps"].clone())
                            .map_err(|e| e.to_string())?;
                        KeyAction::Macro { name, steps }
                    }
                    _ => return Err("Type d'action inconnu".into()),
                };
                self.config
                    .active_profile_mut()
                    .bindings
                    .insert(key, action);
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "input_remove_binding" => {
                let key = p["key"].as_str().ok_or("key manquante")?;
                self.config.active_profile_mut().bindings.remove(key);
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "input_set_esports" => {
                {
                    let prof = self.config.active_profile_mut();
                    if let Some(en) = p["socd_enabled"].as_bool() {
                        prof.socd.enabled = en;
                    }
                    if let Some(preset) = p["socd_preset"].as_str() {
                        prof.socd.pairs = match preset {
                            "qwerty" => vec![
                                SocdPair {
                                    key_a: "A".into(),
                                    key_b: "D".into(),
                                },
                                SocdPair {
                                    key_a: "W".into(),
                                    key_b: "S".into(),
                                },
                            ],
                            "qd_only" => vec![SocdPair {
                                key_a: "Q".into(),
                                key_b: "D".into(),
                            }],
                            "ad_only" => vec![SocdPair {
                                key_a: "A".into(),
                                key_b: "D".into(),
                            }],
                            _ => vec![
                                SocdPair {
                                    key_a: "Q".into(),
                                    key_b: "D".into(),
                                },
                                SocdPair {
                                    key_a: "Z".into(),
                                    key_b: "S".into(),
                                },
                            ],
                        };
                    }
                    if let Some(blocked) = p["blocked_apps"].as_array() {
                        prof.socd.blocked_apps = blocked
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.trim().to_ascii_lowercase()))
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    if let Some(kc) = p["key_chatter_ms"].as_u64() {
                        prof.key_chatter_ms = (kc as u16).min(80);
                    }
                    if let Some(md) = p["mouse_debounce_ms"].as_u64() {
                        prof.mouse_debounce_ms = (md as u16).min(100);
                    }
                }
                let need_engine = {
                    let pr = self.config.active_profile_mut();
                    pr.socd.enabled || pr.key_chatter_ms > 0 || pr.mouse_debounce_ms > 0
                };
                if need_engine && !self.input_engine.is_running() {
                    let _ = self.input_engine.start();
                }
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "input_macro_record_start" => {
                self.input_engine
                    .start_macro_recording()
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "input_macro_record_stop" => {
                let steps = self.input_engine.stop_macro_recording();
                Ok(json!({
                    "state": self.full_state_json(),
                    "recorded_steps": steps,
                }))
            }
            "sys_set_prefs" => {
                if let Some(v) = p["lights_off_on_lock"].as_bool() {
                    self.config.lights_off_on_lock = v;
                    if !v {
                        self.blackout_lock = false;
                    }
                }
                if let Some(v) = p["idle_lights_off_min"].as_u64() {
                    self.config.idle_lights_off_min = v.min(240) as u16;
                }
                if let Some(v) = p["battery_alerts"].as_bool() {
                    self.config.battery_alerts = v;
                }
                if let Some(v) = p["minimize_to_tray"].as_bool() {
                    self.config.minimize_to_tray = v;
                    MINIMIZE_TO_TRAY.store(v, Ordering::Relaxed);
                }
                if let Some(v) = p["start_minimized"].as_bool() {
                    self.config.start_minimized = v;
                    if is_autostart_enabled() {
                        let _ = set_autostart_enabled(true, v);
                    }
                }
                if let Some(v) = p["auto_profile_switch"].as_bool() {
                    self.config.auto_profile_switch = v;
                }
                if let Some(v) = p["autostart"].as_bool() {
                    set_autostart_enabled(v, self.config.start_minimized)
                        .map_err(|e| e.to_string())?;
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "sys_create_shortcuts" => {
                let msg = create_windows_shortcuts().map_err(|e| e.to_string())?;
                Ok(json!({ "message": msg }))
            }
            "profile_set_linked_apps" => {
                let apps: Vec<String> = p["apps"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.trim().to_ascii_lowercase()))
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                self.config.active_profile_mut().linked_apps = apps;
                self.persist_config();
                Ok(self.full_state_json())
            }
            "superio_refresh" => {
                self.superio = read_superio_snapshot();
                Ok(self.full_state_json())
            }
            "superio_set_pwm" => {
                let ch = p["channel"].as_u64().ok_or("channel manquant")? as usize;
                let duty = p["duty_pct"].as_u64().ok_or("duty_pct manquant")?.min(100) as u8;
                let msg = set_superio_fan_pwm(ch, duty).map_err(|e| e.to_string())?;
                self.sync_fan_marker();
                self.superio = read_superio_snapshot();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(msg);
                Ok(st)
            }
            "superio_restore_auto" => {
                let ch = p["channel"].as_u64().ok_or("channel manquant")? as usize;
                let msg = restore_superio_fan_auto(ch).map_err(|e| e.to_string())?;
                self.sync_fan_marker();
                self.superio = read_superio_snapshot();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(msg);
                Ok(st)
            }
            "audio_set_volume" => {
                let is_mic = p["is_mic"].as_bool().unwrap_or(false);
                let pct = p["percent"].as_u64().unwrap_or(50).min(100) as u8;
                set_endpoint_volume(is_mic, pct).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "audio_set_mute" => {
                let is_mic = p["is_mic"].as_bool().unwrap_or(false);
                let muted = p["muted"].as_bool().unwrap_or(false);
                set_endpoint_mute(is_mic, muted).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "profile_export_json" => {
                let prof = self.config.active_profile_mut().clone();
                let pretty = serde_json::to_string_pretty(&prof).map_err(|e| e.to_string())?;
                Ok(json!({ "name": prof.name, "json": pretty }))
            }
            "profile_import_json" => {
                let raw = p["json"].as_str().ok_or("JSON profil manquant")?;
                check_import_size(raw)?;
                let prof: KeyboardProfile =
                    serde_json::from_str(raw).map_err(|e| format!("JSON invalide : {e}"))?;
                let name = prof.name.clone();
                if let Some(existing) = self.config.profiles.iter_mut().find(|pr| pr.name == name) {
                    *existing = prof;
                } else {
                    self.config.profiles.push(prof);
                }
                self.config.active_profile = name.clone();
                self.manual_profile_name = name;
                self.auto_switched = false;
                migrate_legacy_profiles(&mut self.config);
                seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "device_fan_set" => {
                let device = p["device"].as_str().ok_or("device manquant")?;
                let fan = p["fan"].as_str();
                let target: FanTarget =
                    serde_json::from_value(p["target"].clone()).map_err(|e| e.to_string())?;
                self.set_device_fan(device, fan, target)?;
                Ok(self.full_state_json())
            }
            "laptop_set_auto_battery" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                self.config.laptop_auto_battery_saver = on;
                self.persist_config();
                Ok(self.full_state_json())
            }
            _ => Err(format!("Commande IPC inconnue : {cmd}")),
        }
    }
}

fn parse_effect_kind(s: &str) -> Option<EffectKind> {
    match s {
        "Static" => Some(EffectKind::Static),
        "PerKey" => Some(EffectKind::PerKey),
        "RainbowWave" => Some(EffectKind::RainbowWave),
        "SpiralRainbow" => Some(EffectKind::SpiralRainbow),
        "SpectrumCycle" => Some(EffectKind::SpectrumCycle),
        "Breathing" => Some(EffectKind::Breathing),
        "ColorWave" => Some(EffectKind::ColorWave),
        "Visor" => Some(EffectKind::Visor),
        "Rain" => Some(EffectKind::Rain),
        "Gradient" => Some(EffectKind::Gradient),
        "Ambilight" => Some(EffectKind::Ambilight),
        "ReactiveTyping" => Some(EffectKind::ReactiveTyping),
        "AudioVisualizer" => Some(EffectKind::AudioVisualizer),
        "Thermal" => Some(EffectKind::Thermal),
        "Off" => Some(EffectKind::Off),
        _ => None,
    }
}

fn parse_rgb_value(v: &Value) -> Option<Rgb> {
    if let Some(hex) = v.as_str() {
        return Rgb::from_hex(hex);
    }
    let r = v["r"].as_u64()? as u8;
    let g = v["g"].as_u64()? as u8;
    let b = v["b"].as_u64()? as u8;
    Some(Rgb::new(r, g, b))
}

fn parse_lighting_override(v: &Value) -> std::result::Result<LightingOverride, String> {
    if let Some(s) = v.as_str() {
        return match s {
            "global" => Ok(LightingOverride::Global),
            "off" => Ok(LightingOverride::Off),
            other => Err(format!("Mode d'éclairage inconnu : {other}")),
        };
    }
    match v["mode"].as_str().unwrap_or("") {
        "global" => Ok(LightingOverride::Global),
        "off" => Ok(LightingOverride::Off),
        "software" => {
            let e = &v["effect"];
            let mut eff = RgbEffectConfig::default();
            if let Some(k) = e["kind"].as_str().and_then(parse_effect_kind) {
                eff.kind = k;
            }
            if let Some(c) = parse_rgb_value(&e["primary"]) {
                eff.primary = c;
            }
            if let Some(c) = parse_rgb_value(&e["secondary"]) {
                eff.secondary = c;
            }
            if let Some(sp) = e["speed"].as_f64() {
                eff.speed = (sp as f32).clamp(0.1, 5.0);
            }
            if let Some(br) = e["brightness"].as_f64() {
                eff.brightness = (br as f32).clamp(0.0, 1.0);
            }
            if let Some(rev) = e["reverse"].as_bool() {
                eff.reverse = rev;
            }
            Ok(LightingOverride::Software { effect: eff })
        }
        "hardware" => {
            let r = &v["request"];
            let effect = r["effect"]
                .as_str()
                .ok_or("Nom d'effet matériel manquant")?
                .to_string();
            let zone = r["zone"].as_str().map(str::to_string);
            let colors: Vec<Rgb> = r["colors"]
                .as_array()
                .map(|arr| arr.iter().filter_map(parse_rgb_value).collect())
                .unwrap_or_default();
            let speed = r["speed"].as_u64().map(|n| n as u8);
            let brightness = r["brightness"].as_u64().map(|n| n as u8);
            let reverse = r["reverse"].as_bool().unwrap_or(false);
            let save = r["save"].as_bool().unwrap_or(false);
            Ok(LightingOverride::Hardware {
                request: EffectRequest {
                    effect,
                    zone,
                    colors,
                    speed,
                    brightness,
                    reverse,
                    save,
                },
            })
        }
        other => Err(format!("Surcharge d'éclairage invalide : {other}")),
    }
}

fn parse_4byte_hex(hex: &str) -> Option<[u8; 4]> {
    if hex.len() != 8 || !hex.is_ascii() {
        return None;
    }
    Some([
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
        u8::from_str_radix(&hex[6..8], 16).ok()?,
    ])
}

fn mask_user_profile(text: &str) -> String {
    match std::env::var("USERPROFILE") {
        Ok(home) if home.len() > 3 => {
            let masked = text.replace(&home, "%USERPROFILE%");
            masked.replace(&home.replace('\\', "/"), "%USERPROFILE%")
        }
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{is_allowed_origin, resolve_static_asset};
    use super::{
        battery_alert_step, changed_vendors, check_import_size, is_valid_power_guid,
        mask_user_profile, parse_temp_source,
    };
    use opengear_core::cooling::FanTempSource;

    #[test]
    fn battery_alerts_fire_once_at_15_then_once_at_5() {
        let mut last = None;
        let mut fired = Vec::new();
        for pct in [40, 21, 16, 15, 14, 12, 6, 5, 4, 3] {
            let (next, notify) = battery_alert_step(last, pct, false);
            last = next;
            if let Some(level) = notify {
                fired.push((pct, level));
            }
        }
        assert_eq!(fired, vec![(15, 15), (5, 5)]);
    }

    #[test]
    fn battery_alert_starts_at_5_when_first_seen_very_low() {
        assert_eq!(battery_alert_step(None, 3, false), (Some(5), Some(5)));
        assert_eq!(battery_alert_step(Some(5), 3, false), (Some(5), None));
    }

    #[test]
    fn charging_or_recharged_rearms_alerts() {
        assert_eq!(battery_alert_step(Some(5), 4, true), (None, None));
        assert_eq!(battery_alert_step(Some(15), 25, false), (None, None));
        assert_eq!(battery_alert_step(None, 14, false), (Some(15), Some(15)));
    }

    #[test]
    fn user_profile_is_masked_in_reports() {
        if let Ok(home) = std::env::var("USERPROFILE") {
            let text = format!(r"cfg: {home}\AppData\Roaming\OpenGear\x.toml");
            let masked = mask_user_profile(&text);
            assert!(!masked.contains(&home));
            assert!(masked.contains("%USERPROFILE%"));
        }
    }

    #[test]
    fn only_the_brand_that_changed_is_reconnected() {
        let before = [
            (0x046D, 0xC547, 0xFF00),
            (0x0B05, 0x19AF, 0xFF72),
            (0x1B1C, 0x1B73, 0xFF42),
        ];
        let after = [(0x0B05, 0x19AF, 0xFF72), (0x1B1C, 0x1B73, 0xFF42)];
        assert_eq!(changed_vendors(&before, &after), vec![0x046D]);
        assert_eq!(changed_vendors(&after[..1], &after), vec![0x1B1C]);
        assert!(changed_vendors(&before, &before).is_empty());
    }

    #[test]
    fn static_routes_allow_known_modules_and_reject_unknown_or_external_origins() {
        assert!(resolve_static_asset("/").is_some());
        assert!(resolve_static_asset("/js/main.js").is_some());
        assert!(resolve_static_asset("/assets/mouse.svg").is_some());
        assert!(resolve_static_asset("/../src/main.rs").is_none());
        assert!(resolve_static_asset("/app.js").is_none());

        assert!(is_allowed_origin("http://opengear.localhost/"));
        assert!(is_allowed_origin("https://opengear.localhost/index.html"));
        assert!(!is_allowed_origin("https://evil.example/"));
    }

    #[test]
    fn legacy_temp_sources_map_cleanly_and_power_guids_are_validated() {
        assert_eq!(parse_temp_source("GpuLive"), FanTempSource::GpuEdge);
        assert_eq!(parse_temp_source("MixCpuGpu"), FanTempSource::MaxCpuGpu);
        assert_eq!(parse_temp_source("GpuHotspot"), FanTempSource::GpuHotspot);

        assert!(is_valid_power_guid("381b4222-f694-41f0-9685-ff5bb260df2e"));
        assert!(!is_valid_power_guid("not-a-guid"));
        assert!(check_import_size("ok").is_ok());
        assert!(check_import_size(&"x".repeat(2 * 1024 * 1024 + 1)).is_err());
    }
}
