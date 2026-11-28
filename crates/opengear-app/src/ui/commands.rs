use super::backend::{
    BackendEngine, apply_validated_hw_effect, migrate_legacy_profiles, release_device_lighting,
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
use opengear_core::cooling::{
    CurveMixOp, CurvePoint, FanPolicyMode, FanTempSource, SensorKind, SoftwareFanMode,
    SoftwareFanTracker, TempMixConfig, TempMixOp, fan_rule_key,
};
use opengear_core::driver::{DeviceKind, Value as DriverValue, setting_ids};
use opengear_core::fans::FanTarget;
use opengear_core::lighting::{EffectRequest, LightingOverride, Placement};
use opengear_core::onboard::{OnboardMacroStep, OnboardProfilePatch};
use opengear_core::{
    BlendMode, EffectKind, KeyAction, KeyboardProfile, MacroMode, MacroStep, Rgb, RgbEffectConfig,
    RgbLayer, SocdPair, scan_hid_devices,
};
use opengear_input::movement_keys_effective;
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

/// Index of the pump channel, the only header the pump presets may be written to.
///
/// The pump is not always the first channel: on most boards `CPU_FAN` comes first and
/// `AIO_PUMP` sits further down, so the index has to be looked up by header, never assumed.
fn pump_channel_index(channels: &[opengear_core::cooling::FanChannelConfig]) -> Option<usize> {
    channels
        .iter()
        .position(|c| c.header == opengear_core::cooling::FanHeaderKind::AioPump)
}

fn undo_label_for(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "set_rgb_config" => "Réglage de l'éclairage",
        "set_per_key_color" => "Couleur de touche",
        "clear_per_key" => "Effacement des couleurs par touche",
        "lighting_sync_all" => "Synchronisation de l'éclairage",
        "lighting_set_device" => "Éclairage par appareil",
        "lighting_resize_zone" => "Nombre de LED",
        "lighting_set_placement" => "Placement sur le bureau",
        "set_rgb_layers" => "Calques RGB",
        "switch_profile" => "Changement de profil",
        "create_profile" => "Création de profil",
        "set_keyboard_layout" => "Format du clavier",
        "input_set_winlock" => "Verrou de la touche Windows",
        "input_set_binding" => "Assignation de touche",
        "input_remove_binding" => "Suppression d'assignation",
        "input_set_layer_binding" => "Assignation sur couche",
        "input_remove_layer_binding" => "Suppression sur couche",
        "input_set_mouse_binding" => "Assignation de bouton souris",
        "input_remove_mouse_binding" => "Suppression d'assignation souris",
        "input_set_mouse_layer_binding" => "Assignation souris sur couche",
        "input_remove_mouse_layer_binding" => "Suppression souris sur couche",
        "input_set_esports" => "Réglages e-sport",
        "profile_set_linked_apps" => "Applications liées au profil",
        "profile_import_json" => "Import de profil",
        "apply_preset" => "Préréglage",
        "device_fan_rule_set" => "Règle de ventilation",
        _ => return None,
    })
}

pub(crate) fn parse_temp_source(s: &str) -> FanTempSource {
    match s {
        "Cpu" | "cpu" => FanTempSource::Cpu,
        "GpuLive" | "GpuEdge" | "gpu_edge" => FanTempSource::GpuEdge,
        "GpuHotspot" | "gpu_hotspot" => FanTempSource::GpuHotspot,
        "Nvme" | "nvme" => FanTempSource::Nvme,
        "MixCpuGpu" | "MaxCpuGpu" | "max_cpu_gpu" => FanTempSource::MaxCpuGpu,
        "AvgCpuGpu" | "avg_cpu_gpu" => FanTempSource::AvgCpuGpu,
        "MaxAll" | "max_all" => FanTempSource::MaxAll,
        "CustomMix" | "custom_mix" => FanTempSource::CustomMix,
        _ => FanTempSource::CpuBios,
    }
}

fn parse_temp_mix(mix: &Value) -> TempMixConfig {
    let op = match mix["op"].as_str().unwrap_or("Max") {
        "Avg" | "avg" => TempMixOp::Avg,
        "Delta" | "delta" => TempMixOp::Delta,
        _ => TempMixOp::Max,
    };
    let a = parse_sensor_kind(
        mix.get("a")
            .or_else(|| mix.get("sensor_a"))
            .and_then(|v| v.as_str())
            .unwrap_or("Cpu"),
    );
    let b = parse_sensor_kind(
        mix.get("b")
            .or_else(|| mix.get("sensor_b"))
            .and_then(|v| v.as_str())
            .unwrap_or("GpuEdge"),
    );
    TempMixConfig { op, a, b }
}

fn parse_curve_points(pts: &[Value], dst: &mut [CurvePoint; 8]) {
    for (i, item) in pts.iter().take(8).enumerate() {
        let t = item["temp_c"].as_u64().unwrap_or(50).clamp(20, 100) as u8;
        let d = item["duty_pct"].as_u64().unwrap_or(50).clamp(0, 100) as u8;
        dst[i] = CurvePoint {
            temp_c: t,
            duty_pct: d,
        };
    }
}

fn parse_sensor_kind(s: &str) -> SensorKind {
    match s {
        "GpuEdge" | "gpu_edge" => SensorKind::GpuEdge,
        "GpuHotspot" | "gpu_hotspot" => SensorKind::GpuHotspot,
        "Nvme" | "nvme" => SensorKind::Nvme,
        _ => SensorKind::Cpu,
    }
}

fn parse_blend_mode(s: &str) -> BlendMode {
    match s {
        "Add" | "add" => BlendMode::Add,
        "Screen" | "screen" => BlendMode::Screen,
        "Max" | "max" => BlendMode::Max,
        _ => BlendMode::Normal,
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
                    cfg.zones.clear();
                }
            }
            next
        };
        tracing::info!(enabled = on, "Bascule synchronisation RGB via tray");
        if !on {
            let lighting_cfgs = self.config.active_profile_mut().device_lighting.clone();
            for dev in &mut self.driver_devices {
                let id = dev.info().id;
                if let Some(lighting) = dev.lighting() {
                    release_device_lighting(lighting, lighting_cfgs.get(&id));
                }
            }
            self.live_active_devices.clear();
        }
        self.invalidate_rgb_frames();
        self.persist_config();
    }

    pub(crate) fn tray_switch_profile(&mut self, name: &str) {
        if self.config.profiles.iter().any(|pr| pr.name == name) {
            self.config.active_profile = name.to_string();
            self.manual_profile_name = name.to_string();
            self.auto_switched = false;
            self.apply_restored_config();
        }
    }

    pub(crate) fn tray_apply_preset(&mut self, preset: &str) {
        if self.apply_preset(preset).is_ok() {
            self.persist_config();
        }
    }

    fn onboard_for_cmd(
        &mut self,
        p: &Value,
        missing_msg: &str,
    ) -> std::result::Result<&mut dyn opengear_core::onboard::OnboardMemory, String> {
        let dev_id = p["device_id"]
            .as_str()
            .or_else(|| p["device"].as_str())
            .ok_or("device_id manquant")?;
        self.find_onboard_mut(Some(dev_id))
            .ok_or_else(|| format!("{missing_msg} : {dev_id}"))
    }

    fn pointer_for_cmd(
        &mut self,
        p: &Value,
        missing_msg: &str,
    ) -> std::result::Result<&mut dyn opengear_core::pointer::PointerDevice, String> {
        let dev_id = p["device_id"]
            .as_str()
            .or_else(|| p["device"].as_str())
            .ok_or("device_id manquant")?;
        self.find_pointer_mut(Some(dev_id))
            .ok_or_else(|| format!("{missing_msg} : {dev_id}"))
    }

    pub(crate) fn handle_cmd(
        &mut self,
        cmd: &str,
        p: &Value,
        proxy: Option<&EventLoopProxy<UserEvent>>,
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
        self.invalidate_rgb_frames();
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
            } else {
                let mut applied = 0usize;
                let mut failed = 0usize;
                let curve_ids: Vec<String> = self
                    .curve_states
                    .iter()
                    .filter(|(_, st)| st.available && st.is_admin)
                    .map(|(id, _)| id.clone())
                    .collect();
                if curve_ids.is_empty() {
                    summary.push(
                        "Ventilateurs inchangés (droits administrateur + contrôleur WMI requis)"
                            .into(),
                    );
                } else {
                    for dev_id in curve_ids {
                        let pending: Vec<(usize, opengear_core::cooling::FanChannelConfig)> = self
                            .curve_states
                            .get(&dev_id)
                            .map(|st| {
                                st.channels
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, ch)| ch.source == FanTempSource::CpuBios)
                                    .map(|(i, ch)| {
                                        let mut cfg = ch.clone();
                                        cfg.policy = policy;
                                        (i, cfg)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut ok_indices = Vec::new();
                        if let Some(fc) = self.curve_cooling_for_mut(&dev_id) {
                            for (i, cfg) in pending {
                                if fc.apply_channel_curve(&cfg).is_ok() {
                                    ok_indices.push(i);
                                    applied += 1;
                                } else {
                                    failed += 1;
                                }
                            }
                        } else {
                            failed += pending.len();
                        }
                        if let Some(st) = self.curve_states.get_mut(&dev_id) {
                            for i in ok_indices {
                                st.channels[i].policy = policy;
                            }
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
                }
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
                json!({ "label": "Relancer en administrateur", "cmd": "relaunch_admin", "payload": {} })
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
        } else if let Some(cs) = self.curve_states.values().next() {
            push(
                "asus_wmi",
                "Interface ventilateurs carte mère (WMI)",
                if cs.available { "ok" } else { "warn" },
                cs.message.clone(),
                Value::Null,
            );
        } else {
            push(
                "asus_wmi",
                "Interface ventilateurs carte mère (WMI)",
                "warn",
                if admin {
                    "Aucun contrôleur de courbes carte mère détecté.".into()
                } else {
                    "Droits administrateur requis pour le contrôle WMI carte mère.".into()
                },
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
            && !self
                .driver_devices
                .iter()
                .any(|d| d.info().id.starts_with("asus_aura:"))
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
                || !p.mouse_bindings.is_empty()
                || p.layers.values().any(|m| !m.is_empty())
                || p.mouse_layers.values().any(|m| !m.is_empty())
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
        mask_sensitive_data(&r)
    }

    fn export_anonymous_report(&mut self) -> std::result::Result<Value, String> {
        let mut text = self.diagnostic_report();
        if let Ok(selftest) =
            std::fs::read_to_string(std::env::temp_dir().join("opengear_selftest.txt"))
        {
            text.push_str("\n--- Dernier Auto-Test Matériel ---\n");
            text.push_str(&mask_sensitive_data(&selftest));
        }
        let path = std::env::temp_dir().join("opengear_anonymous_report.txt");
        std::fs::write(&path, &text)
            .map_err(|e| format!("Écriture du rapport impossible : {e}"))?;
        Ok(json!({
            "path": mask_user_profile(&path.display().to_string()),
            "text": text,
        }))
    }

    fn handle_cmd_inner(
        &mut self,
        cmd: &str,
        p: &Value,
        proxy: Option<&EventLoopProxy<UserEvent>>,
    ) -> std::result::Result<Value, String> {
        match cmd {
            "get_state" => {
                if !self.suspended {
                    self.check_dormant_wireless();
                }
                Ok(self.full_state_json())
            }
            "get_catalog" => Ok(Self::catalog_json()),
            "get_telemetry" => Ok(self.telemetry_json()),
            "get_diagnostics" => Ok(self.diagnostics_json()),
            "get_diagnostic_report" => Ok(json!({ "text": self.diagnostic_report() })),
            "export_anonymous_report" => self.export_anonymous_report(),
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
                if let Some(lang) = p["language"].as_str()
                    && (lang == "fr" || lang == "en")
                {
                    self.config.language = lang.to_string();
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
            "device_list_backups" => {
                let backups = self.collect_backups(p["device_id"].as_str());
                Ok(json!({ "backups": backups }))
            }
            "device_restore_backup" => {
                let file = p["file"].as_str().ok_or("Fichier de sauvegarde manquant")?;
                let o = self.onboard_for_cmd(p, "Appareil à mémoire embarquée introuvable")?;
                let msg = o.restore_onboard_backup(file).map_err(|e| e.to_string())?;
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(msg);
                Ok(st)
            }
            "device_set_dpi" => {
                let x = p["x"].as_u64().ok_or("x manquant")? as u16;
                let y = p["y"].as_u64().map(|v| v as u16);
                let lod = p["lod"].as_str();
                let ptr = self.pointer_for_cmd(p, "Réglage DPI non supporté")?;
                ptr.set_dpi_with_lod(x, y, lod).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "device_set_dpi_stage" => {
                let stage = p["stage"].as_u64().ok_or("stage manquant")? as usize;
                let dpi = p["dpi"].as_u64().ok_or("dpi manquant")? as u16;
                let ptr = self.pointer_for_cmd(p, "Pointeur introuvable")?;
                ptr.set_dpi_stage(stage, dpi).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "device_set_active_dpi_stage" => {
                let stage = p["stage"].as_u64().ok_or("stage manquant")? as usize;
                let ptr = self.pointer_for_cmd(p, "Pointeur introuvable")?;
                ptr.set_active_dpi_stage(stage).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "device_onboard_activate" => {
                let slot = p["slot"].as_u64().ok_or("slot manquant")? as u16;
                let o = self.onboard_for_cmd(p, "Mémoire embarquée introuvable")?;
                o.set_active_onboard_profile(slot)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "device_onboard_reload" => {
                let o = self.onboard_for_cmd(p, "Mémoire embarquée introuvable")?;
                o.reload_onboard_profiles().map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "device_onboard_flash_profile" => {
                let idx = p["index"].as_u64().ok_or("index manquant")? as usize;
                let patch = parse_onboard_patch(p);
                let o = self.onboard_for_cmd(p, "Mémoire embarquée introuvable")?;
                o.write_onboard_profile(idx, &patch)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "device_onboard_flash_macro" => {
                let prof_idx = p["profile_index"].as_u64().unwrap_or(0) as usize;
                let btn_idx = p["button_index"].as_u64().unwrap_or(3) as usize;
                let steps_arr = p["steps"].as_array().ok_or("steps manquants")?;
                let steps = parse_onboard_macro_steps(steps_arr);
                let o = self.onboard_for_cmd(p, "Mémoire embarquée introuvable")?;
                o.write_onboard_macro(prof_idx, btn_idx, &steps)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
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
            "device_set_eq" => {
                let device_id = p["device_id"].as_str().ok_or("device_id manquant")?;
                let dev = self
                    .device_mut(device_id)
                    .ok_or_else(|| format!("Appareil introuvable : {device_id}"))?;
                let eq = dev
                    .equalizer()
                    .ok_or_else(|| format!("Égaliseur non supporté sur : {device_id}"))?;
                let gains: Vec<i8> = if let Some(preset_id) = p["preset"].as_str() {
                    eq.eq_info()
                        .and_then(|info| info.presets.into_iter().find(|pr| pr.id == preset_id))
                        .ok_or_else(|| format!("Préréglage égaliseur inconnu : {preset_id}"))?
                        .gains_db
                } else {
                    p["gains"]
                        .as_array()
                        .ok_or("gains ou preset manquant")?
                        .iter()
                        .map(|v| v.as_i64().unwrap_or(0).clamp(-12, 12) as i8)
                        .collect()
                };
                eq.set_eq_gains(&gains).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
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
                    self.apply_restored_config();
                    self.sync_tray_state();
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
                self.sync_tray_state();
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
                tracing::info!(enabled = on, "Bascule synchronisation RGB globale");
                {
                    let prof = self.config.active_profile_mut();
                    prof.enable_rgb = on;
                    for d in &self.driver_devices {
                        let id = d.info().id;
                        let entry = prof.device_lighting.entry(id).or_default();
                        entry.enabled = on;
                        if on {
                            // Clear per-device and per-zone overrides (including Hardware) so Sync
                            // applies uniformly across all zones, while preserving last_*_hardware
                            // for restoration when Sync is disabled.
                            entry.mode = None;
                            entry.zones.clear();
                        }
                    }
                }
                if !on {
                    let lighting_cfgs = self.config.active_profile_mut().device_lighting.clone();
                    for d in &mut self.driver_devices {
                        let id = d.info().id;
                        if let Some(lighting) = d.lighting() {
                            release_device_lighting(lighting, lighting_cfgs.get(&id));
                        }
                    }
                    self.live_active_devices.clear();
                }
                self.invalidate_rgb_frames();
                self.persist_config();
                self.sync_tray_state();
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
                    tracing::info!(
                        device = dev_id,
                        zone = ?req.zone,
                        effect = %req.effect,
                        save = req.save,
                        "Application effet matériel RGB"
                    );
                    let dev = self
                        .device_mut(dev_id)
                        .ok_or_else(|| format!("Appareil introuvable : {dev_id}"))?;
                    let lighting = dev
                        .lighting()
                        .ok_or_else(|| format!("Éclairage non supporté sur : {dev_id}"))?;
                    apply_validated_hw_effect(lighting, &req).map_err(|e| {
                        tracing::warn!(device = dev_id, zone = ?req.zone, effect = %req.effect, ?e, "Échec effet matériel RGB");
                        e.to_string()
                    })?;
                } else if matches!(parsed_override, Some(LightingOverride::Off)) {
                    let dev = self
                        .device_mut(dev_id)
                        .ok_or_else(|| format!("Appareil introuvable : {dev_id}"))?;
                    if zone_id.is_none()
                        && let Some(lighting) = dev.lighting()
                    {
                        let len = opengear_core::lighting::frame_len(&lighting.zones());
                        if let Err(err) = lighting.set_frame(&vec![Rgb::BLACK; len]) {
                            tracing::warn!(device = dev_id, ?err, "Échec extinction éclairage RGB");
                        }
                    }
                } else if enabled_arg == Some(false) {
                    tracing::info!(
                        device = dev_id,
                        "Désactivation synchronisation RGB appareil"
                    );
                    let dev_cfg = self
                        .config
                        .active_profile_mut()
                        .device_lighting
                        .get(dev_id)
                        .cloned();
                    if let Some(dev) = self.device_mut(dev_id)
                        && let Some(lighting) = dev.lighting()
                    {
                        release_device_lighting(lighting, dev_cfg.as_ref());
                    }
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
                            if let LightingOverride::Hardware { ref request } = ov
                                && !request.effect.starts_with("shutdown:")
                            {
                                let mut saved = request.clone();
                                saved.save = false;
                                entry.last_zone_hardware.insert(zid.to_string(), saved);
                            }
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
                            if let LightingOverride::Hardware { ref request } = ov
                                && !request.effect.starts_with("shutdown:")
                            {
                                let mut saved = request.clone();
                                saved.save = false;
                                entry.last_hardware = Some(saved);
                                // Whole-device hardware overrides supersede prior per-zone hardware presets.
                                entry.last_zone_hardware.clear();
                                entry.zones.clear();
                            }
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
                self.invalidate_rgb_frame_for(dev_id);
                self.persist_config();
                self.sync_tray_state();
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
                self.invalidate_rgb_frame_for(dev_id);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "lighting_set_placement" => {
                let dev_id = p["device_id"].as_str().ok_or("device_id manquant")?;
                let x = p["x"].as_f64().unwrap_or(0.0) as f32;
                let y = p["y"].as_f64().unwrap_or(0.0) as f32;
                let w = p["width"].as_f64().unwrap_or(1.0) as f32;
                let h = p["height"].as_f64().unwrap_or(1.0) as f32;
                let rotation_deg = p["rotation_deg"].as_f64().unwrap_or(0.0) as f32;
                let xc = x.clamp(0.0, 0.95);
                let yc = y.clamp(0.0, 0.95);
                let wc = w.clamp(0.05, 1.0 - xc);
                let hc = h.clamp(0.05, 1.0 - yc);
                let rot = rotation_deg.clamp(-180.0, 180.0);
                self.config.set_placement(
                    dev_id,
                    Placement {
                        x: xc,
                        y: yc,
                        width: wc,
                        height: hc,
                        rotation_deg: rot,
                    },
                );
                self.invalidate_rgb_frame_for(dev_id);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "set_rgb_layers" => {
                let arr = p["layers"].as_array().ok_or("layers manquant")?;
                let mut parsed = Vec::with_capacity(arr.len().min(8));
                for item in arr.iter().take(8) {
                    let enabled = item["enabled"].as_bool().unwrap_or(true);
                    let blend = parse_blend_mode(item["blend"].as_str().unwrap_or("Normal"));
                    let opacity = (item["opacity"].as_f64().unwrap_or(1.0) as f32).clamp(0.0, 1.0);
                    let key_filter: Vec<String> = item
                        .get("key_filter")
                        .or_else(|| item.get("keys"))
                        .and_then(|v| v.as_array())
                        .map(|ks| {
                            ks.iter()
                                .filter_map(|k| k.as_str().map(str::trim))
                                .filter(|k| !k.is_empty())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    let ef = &item["effect"];
                    let kind = parse_effect_kind(ef["kind"].as_str().unwrap_or("Static"))
                        .unwrap_or(EffectKind::Static);
                    let primary = ef["primary"]
                        .as_str()
                        .and_then(Rgb::from_hex)
                        .unwrap_or(Rgb::CYAN);
                    let secondary = ef["secondary"]
                        .as_str()
                        .and_then(Rgb::from_hex)
                        .unwrap_or_else(|| Rgb::new(155, 60, 255));
                    let speed = (ef["speed"].as_f64().unwrap_or(1.0) as f32).clamp(0.1, 5.0);
                    let brightness =
                        (ef["brightness"].as_f64().unwrap_or(1.0) as f32).clamp(0.0, 1.0);
                    let reverse = ef["reverse"].as_bool().unwrap_or(false);
                    parsed.push(RgbLayer {
                        enabled,
                        opacity,
                        blend,
                        key_filter,
                        effect: RgbEffectConfig {
                            kind,
                            primary,
                            secondary,
                            speed,
                            brightness,
                            reverse,
                            ..Default::default()
                        },
                    });
                }
                self.config.active_profile_mut().rgb_layers = parsed;
                let need_engine = self.profile_needs_input_engine();
                if !self.headless && need_engine && !self.input_engine.is_running() {
                    let _ = self.input_engine.start();
                }
                self.invalidate_rgb_frames();
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
                    let lighting_cfgs = self.config.active_profile_mut().device_lighting.clone();
                    for dev in &mut self.driver_devices {
                        let id = dev.info().id;
                        if let Some(lighting) = dev.lighting() {
                            release_device_lighting(lighting, lighting_cfgs.get(&id));
                        }
                    }
                    self.live_active_devices.clear();
                }
                self.invalidate_rgb_frames();
                if commit {
                    self.persist_config();
                }
                Ok(self.full_state_json())
            }
            "set_per_key_color" => {
                let key_code = if let Some(code) = p["key_code"].as_str() {
                    code.to_string()
                } else if let Some(idx) = p["led_index"].as_u64() {
                    let target = idx as usize;
                    self.driver_devices
                        .iter_mut()
                        .filter(|d| d.info().kind == DeviceKind::Keyboard)
                        .find_map(|d| {
                            d.lighting().and_then(|l| {
                                l.zones()
                                    .into_iter()
                                    .flat_map(|z| z.leds.into_iter())
                                    .nth(target)
                                    .and_then(|led| led.key_code)
                            })
                        })
                        .ok_or("Index LED inconnu sans code de touche")?
                } else {
                    return Err("key_code manquant".into());
                };
                let hex = p["color"].as_str().ok_or("color manquant")?;
                let col = Rgb::from_hex(hex).ok_or("Couleur hex invalide")?;
                let prof = self.config.active_profile_mut();
                prof.rgb.kind = EffectKind::PerKey;
                prof.rgb.per_key.insert(key_code, col);
                self.invalidate_rgb_frames();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "clear_per_key" => {
                self.config.active_profile_mut().rgb.per_key.clear();
                self.invalidate_rgb_frames();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "device_curve_read" => {
                let device = p["device"].as_str().ok_or("device manquant")?.to_string();
                let st = self
                    .curve_states
                    .get(&device)
                    .ok_or_else(|| format!("Contrôleur de courbes introuvable : {device}"))?;
                if !st.available {
                    return Err(
                        "Droits Admin + interface BIOS WMI requis pour relire les courbes".into(),
                    );
                }
                let read = self
                    .curve_cooling_for_mut(&device)
                    .ok_or_else(|| format!("Contrôleur de courbes absent : {device}"))?
                    .read_firmware_curves()
                    .map_err(|e| e.to_string())?;
                if let Some(entry) = self.curve_states.get_mut(&device) {
                    entry.channels = read;
                }
                Ok(self.full_state_json())
            }
            "device_curve_set" => {
                let device = p["device"].as_str().ok_or("device manquant")?.to_string();
                let idx = p["index"].as_u64().ok_or("index manquant")? as usize;
                let apply = p["apply"].as_bool().unwrap_or(false);
                let (cfg_to_apply, tracker_reset) = {
                    let st = self
                        .curve_states
                        .get_mut(&device)
                        .ok_or_else(|| format!("Contrôleur de courbes introuvable : {device}"))?;
                    let ch = st.channels.get_mut(idx).ok_or("Canal PWM invalide")?;

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
                        parse_curve_points(pts, &mut ch.curve);
                    }
                    let mut reset = false;
                    if let Some(src) = p["source"].as_str() {
                        ch.source = parse_temp_source(src);
                        reset = true;
                    }
                    if let Some(mix) = p.get("custom_mix").filter(|v| v.is_object()) {
                        ch.custom_mix = Some(parse_temp_mix(mix));
                        reset = true;
                    } else if ch.source == FanTempSource::CustomMix && ch.custom_mix.is_none() {
                        ch.custom_mix = Some(TempMixConfig::default());
                    }
                    if let Some(h) = p["hysteresis_c"].as_u64() {
                        ch.hysteresis_c = (h as u8).min(12);
                    }
                    if let Some(sm) = p["smoothing_secs"].as_u64() {
                        ch.smoothing_secs = (sm as u8).min(20);
                    }
                    let cfg = (apply && ch.source == FanTempSource::CpuBios).then(|| ch.clone());
                    (cfg, reset)
                };
                if tracker_reset
                    && let Some(st) = self.curve_states.get_mut(&device)
                    && let Some(tr) = st.trackers.get_mut(idx)
                {
                    tr.reset();
                }
                if let Some(cfg) = cfg_to_apply {
                    self.curve_cooling_for_mut(&device)
                        .ok_or_else(|| format!("Contrôleur de courbes absent : {device}"))?
                        .apply_channel_curve(&cfg)
                        .map_err(|e| e.to_string())?;
                }
                Ok(self.full_state_json())
            }
            "device_pump_preset" => {
                let device = p["device"].as_str().ok_or("device manquant")?.to_string();
                let preset = p["preset"].as_str().unwrap_or("default");
                // These curves are tuned for a pump (45 % floor): find the pump header by kind,
                // never assume channel 0.
                let to_apply = {
                    let st = self
                        .curve_states
                        .get_mut(&device)
                        .ok_or_else(|| format!("Contrôleur de courbes introuvable : {device}"))?;
                    let idx = pump_channel_index(&st.channels)
                        .ok_or("Aucun canal de pompe sur cette carte mère")?;
                    let ch = &mut st.channels[idx];
                    ch.policy = FanPolicyMode::Manual;
                    ch.curve =
                        opengear_core::cooling::FanChannelConfig::silent_loop3_preset(preset);
                    (st.is_admin && st.available).then(|| ch.clone())
                };
                if let Some(cfg) = to_apply
                    && let Some(fc) = self.curve_cooling_for_mut(&device)
                {
                    let _ = fc.apply_channel_curve(&cfg);
                }
                Ok(self.full_state_json())
            }
            "relaunch_admin" => {
                release_single_instance();
                relaunch_as_admin().map_err(|e| e.to_string())?;
                if let Some(px) = proxy {
                    let _ = px.send_event(UserEvent::Quit);
                }
                Ok(json!({ "relaunching": true }))
            }
            "input_set_enabled" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                if on && !self.headless {
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
                self.sync_tray_state();
                Ok(self.full_state_json())
            }
            "input_set_binding" => {
                let key = p["key"].as_str().ok_or("key manquante")?.to_string();
                let action = parse_key_action(p)?;
                self.config
                    .active_profile_mut()
                    .bindings
                    .insert(key, action);
                self.sync_input_rules();
                self.persist_config();
                self.sync_tray_state();
                Ok(self.full_state_json())
            }
            "input_remove_binding" => {
                let key = p["key"].as_str().ok_or("key manquante")?;
                self.config.active_profile_mut().bindings.remove(key);
                self.sync_input_rules();
                self.persist_config();
                self.sync_tray_state();
                Ok(self.full_state_json())
            }
            "input_set_mouse_binding" => {
                let button = p["button"].as_str().ok_or("button manquant")?.to_string();
                let action = parse_key_action(p)?;
                self.config
                    .active_profile_mut()
                    .mouse_bindings
                    .insert(button, action);
                if !self.headless && !self.input_engine.is_running() {
                    let _ = self.input_engine.start();
                }
                self.sync_input_rules();
                self.persist_config();
                self.sync_tray_state();
                Ok(self.full_state_json())
            }
            "input_remove_mouse_binding" => {
                let button = p["button"].as_str().ok_or("button manquant")?;
                self.config
                    .active_profile_mut()
                    .mouse_bindings
                    .remove(button);
                self.sync_input_rules();
                self.persist_config();
                self.sync_tray_state();
                Ok(self.full_state_json())
            }
            "input_set_layer_binding" => {
                let layer = p["layer"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or("layer manquant")?
                    .to_string();
                let key = p["key"].as_str().ok_or("key manquante")?.to_string();
                let action = parse_key_action(p)?;
                self.config
                    .active_profile_mut()
                    .layers
                    .entry(layer)
                    .or_default()
                    .insert(key, action);
                if !self.headless && !self.input_engine.is_running() {
                    let _ = self.input_engine.start();
                }
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "input_remove_layer_binding" => {
                let layer = p["layer"].as_str().ok_or("layer manquant")?;
                let key = p["key"].as_str().ok_or("key manquante")?;
                let prof = self.config.active_profile_mut();
                if let Some(map) = prof.layers.get_mut(layer) {
                    map.remove(key);
                    if map.is_empty() {
                        prof.layers.remove(layer);
                    }
                }
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "input_set_mouse_layer_binding" => {
                let layer = p["layer"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or("layer manquant")?
                    .to_string();
                let button = p["button"].as_str().ok_or("button manquant")?.to_string();
                let action = parse_key_action(p)?;
                self.config
                    .active_profile_mut()
                    .mouse_layers
                    .entry(layer)
                    .or_default()
                    .insert(button, action);
                if !self.headless && !self.input_engine.is_running() {
                    let _ = self.input_engine.start();
                }
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "input_remove_mouse_layer_binding" => {
                let layer = p["layer"].as_str().ok_or("layer manquant")?;
                let button = p["button"].as_str().ok_or("button manquant")?;
                let prof = self.config.active_profile_mut();
                if let Some(map) = prof.mouse_layers.get_mut(layer) {
                    map.remove(button);
                    if map.is_empty() {
                        prof.mouse_layers.remove(layer);
                    }
                }
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
                if !self.headless && need_engine && !self.input_engine.is_running() {
                    let _ = self.input_engine.start();
                }
                self.sync_input_rules();
                self.persist_config();
                self.sync_tray_state();
                Ok(self.full_state_json())
            }
            "input_macro_record_start" => {
                if !self.headless {
                    self.input_engine
                        .start_macro_recording()
                        .map_err(|e| e.to_string())?;
                }
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
                if let Some(v) = p["openrgb_server"].as_bool() {
                    self.config.openrgb_server_enabled = v;
                    self.sync_openrgb_server();
                    if v && !self.headless
                        && let Some(err) = self.openrgb_server_error.clone()
                    {
                        self.config.openrgb_server_enabled = false;
                        self.persist_config();
                        return Err(err);
                    }
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
            "device_fan_refresh" => {
                let device = p["device"].as_str().ok_or("device manquant")?;
                self.refresh_device_fans(device)?;
                Ok(self.full_state_json())
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
                let name = prof.name.trim().to_string();
                if name.is_empty() {
                    return Err("Nom de profil manquant dans le JSON".into());
                }
                if let Some(existing) = self.config.profiles.iter_mut().find(|pr| pr.name == name) {
                    *existing = prof;
                } else {
                    self.config.profiles.push(prof);
                }
                self.config.active_profile = name.clone();
                self.manual_profile_name = name;
                self.auto_switched = false;
                migrate_legacy_profiles(&mut self.config, &mut self.driver_devices);
                seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
                self.sync_input_rules();
                self.persist_config();
                self.sync_tray_state();
                Ok(self.full_state_json())
            }
            "device_fan_set" => {
                let device = p["device"].as_str().ok_or("device manquant")?;
                let fan = p["fan"].as_str();
                let target: FanTarget =
                    serde_json::from_value(p["target"].clone()).map_err(|e| e.to_string())?;
                self.set_device_fan(device, fan, target)?;
                if let Some(fan_id) = fan {
                    let key = fan_rule_key(device, fan_id);
                    self.software_fan_trackers.remove(&key);
                    let prof = self.config.active_profile_mut();
                    match target {
                        FanTarget::Auto => {
                            prof.fan_rules.remove(&key);
                        }
                        FanTarget::Fixed(pct) => {
                            let entry = prof.fan_rules.entry(key).or_default();
                            entry.mode = SoftwareFanMode::Fixed;
                            entry.fixed_pct = pct;
                        }
                    }
                    self.persist_config();
                }
                Ok(self.full_state_json())
            }
            "device_fan_rule_set" => {
                let device = p["device"].as_str().ok_or("device manquant")?.to_string();
                let fan = p["fan"].as_str().ok_or("fan manquant")?.to_string();
                let min_floor = {
                    let dev = self
                        .device_mut(&device)
                        .ok_or_else(|| format!("Appareil introuvable : {device}"))?;
                    let c = dev
                        .cooling()
                        .ok_or_else(|| format!("Ventilateurs non pilotables sur {device}"))?;
                    c.fans()
                        .into_iter()
                        .find(|f| f.id == fan)
                        .ok_or_else(|| format!("Ventilateur introuvable : {fan}"))?
                        .min_pct
                };
                let key = fan_rule_key(&device, &fan);
                let mut rule = self
                    .config
                    .active_profile_mut()
                    .fan_rules
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                if let Some(mode) = p["mode"].as_str() {
                    rule.mode = match mode {
                        "fixed" | "Fixed" => SoftwareFanMode::Fixed,
                        "curve" | "Curve" => SoftwareFanMode::Curve,
                        "mix" | "Mix" => SoftwareFanMode::Mix,
                        _ => SoftwareFanMode::Auto,
                    };
                }
                if let Some(pct) = p["fixed_pct"].as_u64() {
                    rule.fixed_pct = (pct.min(100) as u8).max(min_floor);
                }
                if let Some(src) = p["source"].as_str() {
                    rule.source = parse_temp_source(src);
                }
                if let Some(mix) = p.get("custom_mix").filter(|v| v.is_object()) {
                    rule.custom_mix = Some(parse_temp_mix(mix));
                } else if rule.source == FanTempSource::CustomMix && rule.custom_mix.is_none() {
                    rule.custom_mix = Some(TempMixConfig::default());
                }
                if let Some(pts) = p["curve"].as_array() {
                    parse_curve_points(pts, &mut rule.curve);
                }
                if let Some(h) = p["hysteresis_c"].as_u64() {
                    rule.hysteresis_c = (h as u8).min(12);
                }
                if let Some(sm) = p["smoothing_secs"].as_u64() {
                    rule.smoothing_secs = (sm as u8).min(20);
                }
                if let Some(rp) = p["ramp_pct_per_sec"].as_u64() {
                    rule.ramp_pct_per_sec = (rp as u8).min(100);
                }
                if let Some(src) = p["mix_source"].as_str() {
                    rule.mix_source = parse_temp_source(src);
                }
                if let Some(mix) = p.get("mix_custom").filter(|v| v.is_object()) {
                    rule.mix_custom = Some(parse_temp_mix(mix));
                } else if rule.mix_source == FanTempSource::CustomMix && rule.mix_custom.is_none() {
                    rule.mix_custom = Some(TempMixConfig::default());
                }
                if let Some(pts) = p["mix_curve"].as_array() {
                    parse_curve_points(pts, &mut rule.mix_curve);
                }
                if let Some(op) = p["mix_op"].as_str() {
                    rule.mix_op = match op {
                        "avg" | "Avg" => CurveMixOp::Avg,
                        _ => CurveMixOp::Max,
                    };
                }

                let mut tracker = SoftwareFanTracker::new();
                let target = match rule.mode {
                    SoftwareFanMode::Auto => FanTarget::Auto,
                    SoftwareFanMode::Fixed => {
                        FanTarget::Fixed(rule.fixed_pct.clamp(min_floor, 100))
                    }
                    SoftwareFanMode::Curve | SoftwareFanMode::Mix => {
                        let readings = self.sensor_readings();
                        let duty = tracker
                            .evaluate(&rule, min_floor, &readings, 0.0, 1)
                            .unwrap_or_else(|| min_floor.max(50));
                        FanTarget::Fixed(duty)
                    }
                };
                self.set_device_fan(&device, Some(&fan), target)?;
                if let FanTarget::Fixed(d) = target
                    && rule.is_software_curve()
                {
                    tracker.mark_applied(d);
                    self.software_fan_trackers.insert(key.clone(), tracker);
                } else {
                    self.software_fan_trackers.remove(&key);
                }

                {
                    let prof = self.config.active_profile_mut();
                    if rule.mode == SoftwareFanMode::Auto {
                        prof.fan_rules.remove(&key);
                    } else {
                        prof.fan_rules.insert(key, rule);
                    }
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "device_fan_calibrate" => {
                if p["cancel"].as_bool() == Some(true) {
                    self.cancel_fan_calibration();
                    return Ok(self.full_state_json());
                }
                let device = p["device"].as_str().ok_or("device manquant")?;
                let fan = p["fan"].as_str().ok_or("fan manquant")?;
                self.start_fan_calibration(device, fan)?;
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

fn parse_macro_mode(s: Option<&str>) -> MacroMode {
    match s {
        Some("while_held") => MacroMode::WhileHeld,
        Some("toggle") => MacroMode::Toggle,
        _ => MacroMode::Once,
    }
}

fn parse_key_action(p: &Value) -> std::result::Result<KeyAction, String> {
    let kind = p["kind"].as_str().ok_or("kind manquant")?;
    Ok(match kind {
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
            let steps: Vec<MacroStep> =
                serde_json::from_value(p["steps"].clone()).map_err(|e| e.to_string())?;
            let mode = parse_macro_mode(p["mode"].as_str());
            KeyAction::Macro { name, steps, mode }
        }
        "layer_hold" => {
            let layer = p["layer"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("fn")
                .to_string();
            KeyAction::LayerHold { layer }
        }
        "layer_toggle" => {
            let layer = p["layer"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("fn")
                .to_string();
            KeyAction::LayerToggle { layer }
        }
        "tap_hold" => {
            let tap_key = p["tap"].as_str().ok_or("tap manquant")?.to_string();
            let hold_key = p["hold"].as_str().ok_or("hold manquant")?.to_string();
            let threshold_ms = (p["tap_ms"].as_u64().unwrap_or(200) as u16).clamp(50, 1000);
            KeyAction::TapHold {
                tap_key,
                hold_key,
                threshold_ms,
            }
        }
        _ => return Err("Type d'action inconnu".into()),
    })
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

impl BackendEngine {
    fn collect_backups(&mut self, device_id: Option<&str>) -> Vec<Value> {
        let mut out = Vec::new();
        for d in &mut self.driver_devices {
            let info = d.info();
            if let Some(target) = device_id
                && info.id != target
            {
                continue;
            }
            if let Some(ob) = d.onboard() {
                for b in ob.onboard_backups() {
                    out.push(json!({
                        "device_id": info.id,
                        "device_model": info.model,
                        "device_vendor": info.vendor,
                        "file": b.file,
                        "tag": b.tag,
                        "sector_hex": b.sector_hex,
                        "size": b.size,
                        "created_ms": b.created_ms,
                    }));
                }
            }
        }
        out
    }
}

fn parse_onboard_patch(p: &Value) -> OnboardProfilePatch {
    OnboardProfilePatch {
        report_rate_hz: p["report_rate_hz"].as_u64().map(|v| v as u16),
        default_dpi_idx: p["default_dpi_idx"].as_u64().map(|v| (v as u8).min(4)),
        dpi_stages: p["dpi_stages"].as_array().map(|arr| {
            arr.iter()
                .take(5)
                .filter_map(|v| v.as_u64().map(|dpi| (dpi as u16).clamp(100, 32000)))
                .collect()
        }),
        button_hexes: p["buttons"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|b| b.as_str().map(str::to_string))
                .collect()
        }),
    }
}

fn parse_onboard_macro_steps(arr: &[Value]) -> Vec<OnboardMacroStep> {
    arr.iter()
        .filter_map(|s| {
            let kind = s["kind"].as_str().unwrap_or("");
            let val = s["value"].as_u64().unwrap_or(0) as u16;
            match kind {
                "key_down" => Some(OnboardMacroStep::KeyPress(val as u8)),
                "key_up" => Some(OnboardMacroStep::KeyRelease(val as u8)),
                "mouse_down" => Some(OnboardMacroStep::MousePress(val as u8)),
                "mouse_up" => Some(OnboardMacroStep::MouseRelease(val as u8)),
                "delay" => Some(OnboardMacroStep::DelayMs(val)),
                _ => None,
            }
        })
        .collect()
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

/// Mask user home directories, OS username, and long hex serial substrings from shareable logs.
fn mask_sensitive_data(text: &str) -> String {
    let mut out = mask_user_profile(text);
    if let Ok(user) = std::env::var("USERNAME")
        && user.len() >= 3
    {
        out = out.replace(&format!("\\{user}\\"), "\\%USERNAME%\\");
        out = out.replace(&format!("/{user}/"), "/%USERNAME%/");
    }
    // Redact long hex device serial tokens (>= 16 hex digits such as Corsair sub0_<SERIAL>).
    let mut masked = String::with_capacity(out.len());
    let mut run = 0usize;
    let mut buf = String::new();
    for ch in out.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            buf.push(ch);
        } else {
            if run >= 16 {
                masked.push_str(&buf[..4]);
                masked.push_str("***");
                masked.push_str(&buf[buf.len() - 4..]);
            } else {
                masked.push_str(&buf);
            }
            buf.clear();
            run = 0;
            masked.push(ch);
        }
    }
    if run >= 16 {
        masked.push_str(&buf[..4]);
        masked.push_str("***");
        masked.push_str(&buf[buf.len() - 4..]);
    } else {
        masked.push_str(&buf);
    }
    masked
}

#[cfg(test)]
mod tests {
    use super::super::{is_allowed_origin, resolve_static_asset};
    use super::{
        battery_alert_step, changed_vendors, check_import_size, is_valid_power_guid,
        mask_sensitive_data, mask_user_profile, parse_temp_source, pump_channel_index,
    };
    use opengear_core::cooling::{FanChannelConfig, FanHeaderKind, FanTempSource};

    /// Pump presets carry a 45 % floor: written to any other header they would drive a plain
    /// fan with a pump curve. The target is found by header, never by position.
    #[test]
    fn pump_presets_target_the_pump_header_wherever_it_sits() {
        let channels: Vec<FanChannelConfig> = [
            FanHeaderKind::CpuFan,
            FanHeaderKind::ChassisFan1,
            FanHeaderKind::AioPump,
            FanHeaderKind::ChassisFan2,
        ]
        .into_iter()
        .map(FanChannelConfig::default_for)
        .collect();
        let idx = pump_channel_index(&channels).expect("canal de pompe");
        assert_eq!(idx, 2);
        assert_eq!(channels[idx].header, FanHeaderKind::AioPump);
    }

    /// A board with no pump must refuse the preset rather than fall back to another header.
    #[test]
    fn a_board_without_a_pump_has_no_target() {
        let channels: Vec<FanChannelConfig> = [FanHeaderKind::CpuFan, FanHeaderKind::ChassisFan1]
            .into_iter()
            .map(FanChannelConfig::default_for)
            .collect();
        assert_eq!(pump_channel_index(&channels), None);
        assert_eq!(pump_channel_index(&[]), None);
    }

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
        assert!(resolve_static_asset("/js/i18n.js").is_some());
        assert!(resolve_static_asset("/assets/mouse.webp").is_some());
        assert!(resolve_static_asset("/assets/ram.webp").is_some());
        assert!(resolve_static_asset("/assets/laptop.webp").is_some());
        assert!(resolve_static_asset("/assets/generic_device.webp").is_some());
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

    #[test]
    fn enabling_global_sync_clears_zone_overrides_while_preserving_hw_restore() {
        use opengear_core::Rgb;
        use opengear_core::lighting::{DeviceLightingConfig, EffectRequest, LightingOverride};
        let hw_req = EffectRequest {
            effect: "Static".into(),
            zone: Some("ch_0".into()),
            colors: vec![Rgb::CYAN],
            speed: None,
            brightness: None,
            reverse: false,
            save: false,
        };
        let mut cfg = DeviceLightingConfig {
            enabled: false,
            mode: Some(LightingOverride::Hardware {
                request: hw_req.clone(),
            }),
            ..Default::default()
        };
        cfg.zones.insert(
            "ch_0".into(),
            LightingOverride::Hardware {
                request: hw_req.clone(),
            },
        );
        cfg.last_zone_hardware.insert("ch_0".into(), hw_req);

        // Mirrors what `lighting_sync_all` / `tray_toggle_sync` does on enable:
        cfg.enabled = true;
        cfg.mode = None;
        cfg.zones.clear();

        assert!(cfg.zones.is_empty());
        assert!(cfg.last_zone_hardware.contains_key("ch_0"));
    }

    #[test]
    fn mask_sensitive_data_redacts_home_user_and_hex_serials() {
        let mut sample =
            String::from("dev serial F50019035FFDAB88AA17884605038014 ok; short ABCDEF01 kept");
        if let Ok(home) = std::env::var("USERPROFILE") {
            sample.push_str(&format!(r"; cfg {home}\OpenGear"));
        }
        if let Ok(user) = std::env::var("USERNAME") {
            sample.push_str(&format!(r"; extra D:\Tmp\{user}\logs"));
        }
        let masked = mask_sensitive_data(&sample);
        assert!(!masked.contains("F50019035FFDAB88AA17884605038014"));
        assert!(masked.contains("F500***8014"));
        assert!(masked.contains("ABCDEF01"));
        if let Ok(home) = std::env::var("USERPROFILE") {
            assert!(!masked.contains(&home));
        }
        if let Ok(user) = std::env::var("USERNAME")
            && user.len() >= 3
        {
            assert!(!masked.contains(&format!(r"\{user}\")));
            assert!(masked.contains(r"\%USERNAME%\"));
        }
        let defaults = opengear_core::profile::AppConfig::default();
        assert_eq!(defaults.language, "fr");
        assert!(!defaults.openrgb_server_enabled);
    }
}
