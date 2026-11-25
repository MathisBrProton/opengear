use super::backend::BackendEngine;
#[cfg(test)]
use super::backend::{battery_alert_step, changed_vendors};
use super::{BackendMsg, MINIMIZE_TO_TRAY, UserEvent};
use crate::audio_ext::{set_endpoint_mute, set_endpoint_volume};
use crate::selftest::run_full_selftest;
use crate::system_ext::idle_duration;
use crate::tray_icon::{
    create_windows_shortcuts, is_autostart_enabled, release_single_instance, set_autostart_enabled,
};
use opengear_asus::{
    AuraHwMode, CurvePoint, EneRamMode, FanPolicyMode, FanTempSource, OmenGpuPowerLevel,
    OmenThermalMode, apply_asus_wmi_curve, apply_ram_rgb, install_official_pawnio, is_elevated,
    list_power_plans, probe_hp_omen, probe_ram_rgb, read_asus_wmi_channel, read_display_info,
    read_laptop_battery, read_superio_snapshot, relaunch_as_admin, restore_superio_fan_auto,
    set_active_power_plan, set_display_brightness, set_display_refresh_rate,
    set_hp_omen_fan_level, set_hp_omen_gpu_power, set_hp_omen_kb_backlight,
    set_hp_omen_kb_zones, set_hp_omen_max_fan, set_hp_omen_thermal_mode, set_superio_fan_pwm,
};
use opengear_core::{
    EffectKind, KeyAction, KeyboardProfile, MacroStep, OperatingMode, PollingRate, Rgb, SocdPair,
    scan_hid_devices,
};
use opengear_corsair::proto::{
    PROP_BRIGHTNESS_INDICATOR_COLOR, PROP_ESPORTS_BACKLIGHT_COLOR, PROP_PROFILE_INDICATOR_COLOR,
    PROP_WINLOCK_OFF_COLOR, PROP_WINLOCK_ON_COLOR,
};
use opengear_corsair::{
    CorsairDevice, CorsairDeviceKind, HwpLightingKind, list_corsair_backups, restore_corsair_backup,
};
use opengear_input::movement_keys_effective;
use opengear_logitech::{
    KeyboardRgbMode, LodLevel, LogitechHeadset, LogitechMouse, MouseButtonAction, MouseLedMode,
    OnboardMacroStep, OnboardMode, backup_path_for, list_sector_backups,
};
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
        "set_sync_all" => "Synchronisation de l'éclairage",
        "switch_profile" => "Changement de profil",
        "create_profile" => "Création de profil",
        "set_azerty" => "Disposition du clavier",
        "input_set_winlock" => "Verrou de la touche Windows",
        "input_set_binding" => "Assignation de touche",
        "input_remove_binding" => "Suppression d'assignation",
        "input_set_esports" => "Réglages e-sport",
        "profile_set_linked_apps" => "Applications liées au profil",
        "profile_import_json" => "Import de profil",
        "apply_preset" => "Préréglage",
        "asus_set_led_count" => "Nombre de LED",
        "cooling_sl3_argb" => "Éclairage du waterblock",
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
    pub(crate) fn spawn_async_job(
        &self,
        id: u64,
        cmd: &str,
        tx: mpsc::Sender<BackendMsg>,
    ) {
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
            let next = !(prof.enable_rgb && prof.sync_asus_aura);
            prof.enable_rgb = next;
            prof.sync_asus_aura = next;
            next
        };
        if !on {
            for dev in &mut self.corsair_devices {
                if dev.info.kind == CorsairDeviceKind::Keyboard {
                    let _ = dev.set_mode(OperatingMode::Hardware);
                }
            }
            if let Some(aura) = self.asus_aura.as_mut() {
                let _ = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false);
            }
        } else if let Some(aura) = self.asus_aura.as_mut() {
            let _ = aura.enter_direct_mode();
        }
        self.persist_config();
    }

    fn corsair_mut(&mut self, p: &Value) -> std::result::Result<&mut CorsairDevice, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.corsair_devices
            .get_mut(idx)
            .ok_or_else(|| "Périphérique Corsair non connecté".to_string())
    }

    fn logi_mouse_mut(&mut self, p: &Value) -> std::result::Result<&mut LogitechMouse, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.logi_mice
            .get_mut(idx)
            .ok_or_else(|| "Souris Logitech non connectée".to_string())
    }

    fn logi_headset_mut(&mut self, p: &Value) -> std::result::Result<&mut LogitechHeadset, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.logi_headsets
            .get_mut(idx)
            .ok_or_else(|| "Casque Logitech non connecté".to_string())
    }

    fn logi_kb_mut(&mut self, p: &Value) -> std::result::Result<&mut opengear_logitech::LogitechKeyboard, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.logi_keyboards
            .get_mut(idx)
            .ok_or_else(|| "Clavier Logitech non connecté".to_string())
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
        let Some(label) = (if record_undo { undo_label_for(cmd) } else { None }) else {
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
            p.sync_asus_aura = true;
            p.rgb.kind = kind;
            p.rgb.primary = primary;
            p.rgb.brightness = brightness;
        }
        if let Some(aura) = self.asus_aura.as_mut() {
            let _ = aura.enter_direct_mode();
        }
        summary.push(format!("Éclairage : {}", kind.label()));

        if let Some(policy) = fan_policy {
            if self.hp_omen.detected {
                if self.hp_omen.wmi_accessible {
                    let target = match preset {
                        "silent" => OmenThermalMode::Cool,
                        "gaming" => OmenThermalMode::Performance,
                        _ => OmenThermalMode::Default,
                    };
                    match set_hp_omen_thermal_mode(self.hp_omen.thermal_version, target) {
                        Ok(_) => {
                            self.hp_omen.thermal_mode = target;
                            summary.push(format!("OMEN Thermique : {}", target.label()));
                        }
                        Err(e) => summary.push(format!("OMEN Thermique inchangé ({e})")),
                    }
                } else {
                    summary.push("OMEN Thermique inchangé (droits administrateur requis)".into());
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
                    policy.asus_profile_str(),
                    if failed > 0 { format!(", {failed} refusé(s) par le BIOS") } else { String::new() }
                ));
            } else {
                summary.push("Ventilateurs inchangés (droits administrateur + ASUS WMI requis)".into());
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
        if preset == "lights_off"
            && self.hp_omen.wmi_accessible
            && self.hp_omen.keyboard_type.is_software_rgb()
        {
            let _ = set_hp_omen_kb_backlight(false);
            self.hp_omen.backlight_on = false;
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

        let (st, detail, action) = match opengear_asus::pawnio_availability() {
            opengear_asus::PawnIoAvailability::Ready => ("ok", "Pilote signé PawnIO actif (capteurs CPU, SMBus, SuperIO).".to_string(), Value::Null),
            opengear_asus::PawnIoAvailability::NeedsAdmin => (
                "warn",
                "Le pilote PawnIO est installé mais nécessite les droits administrateur.".to_string(),
                Value::Null,
            ),
            opengear_asus::PawnIoAvailability::NotInstalled => (
                "warn",
                "Le pilote signé PawnIO n'est pas installé : température CPU native, RAM RGB et ventilateurs directs indisponibles.".to_string(),
                json!({ "label": "Installer PawnIO", "cmd": "pawnio_install", "payload": {} }),
            ),
        };
        push("pawnio", "Pilote matériel PawnIO", st, detail, action);

        if self.hp_omen.detected {
            push(
                "hp_omen",
                "HP OMEN BIOS WMI",
                if !admin {
                    "warn"
                } else if self.hp_omen.wmi_accessible {
                    "ok"
                } else {
                    "warn"
                },
                self.hp_omen.diagnostic.clone(),
                if !admin {
                    json!({ "label": "Relancer en administrateur", "cmd": "cooling_relaunch_admin", "payload": {} })
                } else {
                    Value::Null
                },
            );
        } else {
            push(
                "asus_wmi",
                "Interface ventilateurs ASUS (WMI)",
                if self.cooling_status.wmi_available { "ok" } else { "warn" },
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

        let n_dev = self.corsair_devices.len()
            + self.logi_mice.len()
            + self.logi_keyboards.len()
            + self.logi_headsets.len()
            + usize::from(self.asus_aura.is_some())
            + usize::from(self.hp_omen.detected);
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

        if !self.is_laptop && !self.hp_omen.detected {
            if let Some(err) = &self.asus_err {
                push("aura", "Contrôleur ASUS Aura", "warn", format!("Non connecté : {err}"), Value::Null);
            }
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
            if !needs_engine || engine_ok { "ok" } else { "error" },
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
            Some(w) => push("config", "Fichier de profils", "error", w.clone(), Value::Null),
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
        let _ = writeln!(r, "=== Rapport de diagnostic OpenGear {} ===", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(r, "Carte mère : {} (BIOS {})", s.motherboard, s.bios_version);
        let _ = writeln!(r, "CPU : {} | GPU : {} | RAM : {:.0} Go", s.cpu_name, s.gpu_name, s.ram_total_gb);
        let _ = writeln!(r, "Administrateur : {} | Mode UI : {}", is_elevated(), self.config.ui_mode);
        let _ = writeln!(r, "\n--- Appareils pilotés ---");
        for d in &self.corsair_devices {
            let _ = writeln!(
                r,
                "Corsair {} ({:04X}:{:04X}, {}) FW {} | mode {:?} | report {} o | FlashTap 0xF5 = {}",
                d.info.product_name, d.info.vid, d.info.pid, d.info.kind.label(),
                d.info.firmware_version, d.info.mode, d.info.out_report_len, d.info.flashtap_probe
            );
        }
        for m in &self.logi_mice {
            let _ = writeln!(
                r,
                "Logitech souris {} ({:04X}:{:04X}, slot 0x{:02X}) HID++ v{} FW {}",
                m.info.name, m.info.vid, m.info.pid, m.info.device_index,
                m.info.proto_version, m.info.firmware.join(" / ")
            );
        }
        for k in &self.logi_keyboards {
            let _ = writeln!(r, "Logitech clavier {} ({:04X}:{:04X})", k.info.name, k.info.vid, k.info.pid);
        }
        for h in &self.logi_headsets {
            let _ = writeln!(r, "Logitech casque {} ({:04X}:{:04X})", h.info.name, h.info.vid, h.info.pid);
        }
        if self.hp_omen.detected {
            let zones: Vec<String> = self.hp_omen.zone_colors.iter().map(|c| c.to_hex()).collect();
            let _ = writeln!(
                r,
                "HP OMEN {} ({}) wmi={} thermal_v={} mode={:?} max_fan={} fans={} rpms={:?} kbd={} backlight={} zones={:?}",
                self.hp_omen.model,
                self.hp_omen.board_id,
                self.hp_omen.wmi_accessible,
                self.hp_omen.thermal_version,
                self.hp_omen.thermal_mode,
                self.hp_omen.max_fan,
                self.hp_omen.fan_count,
                self.hp_omen.fan_rpms,
                self.hp_omen.keyboard_label,
                self.hp_omen.backlight_on,
                zones,
            );
            for line in &self.hp_omen.probe_log {
                let _ = writeln!(r, "  [hpqBIntM] {line}");
            }
        }
        if self.laptop_battery.present {
            let b = &self.laptop_battery;
            let d = &self.laptop_display;
            let _ = writeln!(
                r,
                "Portable : Bat={:?}% (Santé={:?}%, Cycles={:?}, Secteur={}) | Écran={}x{}@{:?}Hz (supportés={:?}, lum={:?}%)",
                b.percent,
                b.health_pct,
                b.cycle_count,
                b.ac_online,
                d.width,
                d.height,
                d.current_hz,
                d.supported_hz,
                d.brightness_pct
            );
        }
        match &self.asus_aura {
            Some(a) => {
                let _ = writeln!(r, "ASUS Aura {} FW {} | {} canaux", a.info.product_name, a.info.firmware_version, a.info.channels.len());
            }
            None if !self.is_laptop && !self.hp_omen.detected => {
                let _ = writeln!(r, "ASUS Aura : {}", self.asus_err.as_deref().unwrap_or("absent"));
            }
            None => {}
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
        let _ = writeln!(r, "\n--- Fin du journal ({}) ---", crate::app_log::log_path().display());
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
                    let sys_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
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
                if let Some(m) = p["ui_mode"].as_str() {
                    if m == "simple" || m == "expert" {
                        self.config.ui_mode = m.to_string();
                    }
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
                st["last_action_msg"] = json!(format!("Préréglage appliqué — {}", summary.join(" · ")));
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
                Ok(json!({ "message": format!("Profils sauvegardés dans {}", self.config_path.display()) }))
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
            "set_azerty" => {
                self.config.use_azerty_labels = p["enabled"].as_bool().unwrap_or(true);
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
            "set_sync_all" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                {
                    let prof = self.config.active_profile_mut();
                    prof.enable_rgb = on;
                    prof.sync_asus_aura = on;
                }
                if !on {
                    for dev in &mut self.corsair_devices {
                        if dev.info.kind == CorsairDeviceKind::Keyboard {
                            let _ = dev.set_mode(OperatingMode::Hardware);
                        }
                    }
                    if let Some(aura) = self.asus_aura.as_mut() {
                        let _ = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false);
                    }
                } else if let Some(aura) = self.asus_aura.as_mut() {
                    let _ = aura.enter_direct_mode();
                }
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
                    if let Some(k) = p["kind"].as_str() {
                        if let Some(ek) = parse_effect_kind(k) {
                            prof.rgb.kind = ek;
                        }
                    }
                    if let Some(hex) = p["primary"].as_str() {
                        if let Some(c) = Rgb::from_hex(hex) {
                            prof.rgb.primary = c;
                        }
                    }
                    if let Some(hex) = p["secondary"].as_str() {
                        if let Some(c) = Rgb::from_hex(hex) {
                            prof.rgb.secondary = c;
                        }
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
                    for dev in &mut self.corsair_devices {
                        if dev.info.kind == CorsairDeviceKind::Keyboard {
                            let _ = dev.set_mode(OperatingMode::Hardware);
                        }
                    }
                }
                if commit {
                    self.persist_config();
                }
                Ok(self.full_state_json())
            }
            "set_per_key_color" => {
                let led_idx = p["led_index"].as_u64().ok_or("led_index manquant")? as usize;
                let hex = p["color"].as_str().ok_or("color manquant")?;
                let col = Rgb::from_hex(hex).ok_or("Couleur hex invalide")?;
                let prof = self.config.active_profile_mut();
                prof.rgb.kind = EffectKind::PerKey;
                prof.rgb.per_key.insert(led_idx, col);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "clear_per_key" => {
                self.config.active_profile_mut().rgb.per_key.clear();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "corsair_set_pollrate" => {
                let hz = p["hz"].as_u64().ok_or("hz manquant")? as u16;
                let rate = PollingRate::from_hz(hz).ok_or("Fréquence invalide")?;
                let dev = self.corsair_mut(p)?;
                dev.set_pollrate(rate).map_err(|e| e.to_string())?;
                self.config.active_profile_mut().polling_rate = rate;
                self.persist_config();
                Ok(self.full_state_json())
            }
            "corsair_set_brightness" => {
                let pct = p["percent"].as_u64().ok_or("percent manquant")? as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_hw_brightness_pct(pct).map_err(|e| e.to_string())?;
                self.config.active_profile_mut().hw_brightness_pct = pct.min(100);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "corsair_set_mode" => {
                let m = match p["mode"].as_str().unwrap_or("hw") {
                    "sw" => OperatingMode::Software,
                    _ => OperatingMode::Hardware,
                };
                let dev = self.corsair_mut(p)?;
                dev.set_mode(m).map_err(|e| e.to_string())?;
                if m == OperatingMode::Hardware {
                    self.config.active_profile_mut().enable_rgb = false;
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "corsair_set_hw_winlock" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                let dev = self.corsair_mut(p)?;
                dev.set_hw_winlock(on).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_set_flashtap" => {
                let on = p["enabled"].as_bool().ok_or("enabled manquant")?;
                let dev = self.corsair_mut(p)?;
                if !dev.info.flashtap_supported {
                    return Err("Ce périphérique ne propose pas le FlashTap matériel".into());
                }
                dev.set_flashtap(on).map_err(|e| e.to_string())?;
                let mut msg = if on {
                    "FlashTap matériel activé : le clavier gère lui-même la priorité des touches opposées".to_string()
                } else {
                    "FlashTap matériel désactivé".to_string()
                };
                if on && self.config.active_profile_mut().socd.enabled {
                    self.config.active_profile_mut().socd.enabled = false;
                    self.sync_input_rules();
                    self.persist_config();
                    msg.push_str(" — SOCD logiciel désactivé pour éviter un double traitement");
                }
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(msg);
                Ok(st)
            }
            "corsair_set_winlock_shortcuts" => {
                let mask = p["mask"].as_u64().unwrap_or(1) as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_winlock_shortcuts(mask).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_set_debounce" => {
                let en = p["enabled"].as_bool().unwrap_or(true);
                let ms = p["timeout_ms"].as_u64().unwrap_or(30) as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_hw_debounce(en, ms).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_set_hw_profile" => {
                let idx = p["index"].as_u64().unwrap_or(0) as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_active_hw_profile(idx).map_err(|e| e.to_string())?;
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
                dev.set_indicator_color(prop, col).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_mouse_set_dpi_stage" => {
                let stage = p["stage"].as_u64().unwrap_or(0) as u8;
                let dpi = p["dpi"].as_u64().unwrap_or(800) as u16;
                let dev = self.corsair_mut(p)?;
                dev.set_mouse_dpi_stage(stage, dpi).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_mouse_set_active_stage" => {
                let stage = p["stage"].as_u64().unwrap_or(0) as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_mouse_active_stage(stage).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_mouse_set_lift_height" => {
                let level = p["level"].as_u64().unwrap_or(1) as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_mouse_lift_height(level).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_headset_set_sidetone" => {
                let pct = p["percent"].as_u64().unwrap_or(0) as u8;
                let muted = p["muted"].as_bool().unwrap_or(pct == 0);
                let dev = self.corsair_mut(p)?;
                dev.set_headset_sidetone(pct, muted).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_reload_hwp" => {
                let dev = self.corsair_mut(p)?;
                dev.reload_hwp_profiles().map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "corsair_flash_hwp_lighting" => {
                let prof_idx = p["profile_index"].as_u64().unwrap_or(0) as usize;
                let kind = HwpLightingKind::parse_id(p["kind"].as_str().unwrap_or("StaticColor"));
                let primary = p["primary"].as_str().and_then(Rgb::from_hex).unwrap_or(Rgb::CYAN);
                let secondary = p["secondary"]
                    .as_str()
                    .and_then(Rgb::from_hex)
                    .unwrap_or(Rgb::new(155, 60, 255));
                let speed = p["speed"].as_u64().unwrap_or(1) as u8;
                let direction = p["direction"].as_u64().unwrap_or(1) as u8;
                let new_name = p["name"].as_str().map(|s| s.to_string());
                let dev = self.corsair_mut(p)?;
                dev.flash_hwp_lighting(
                    prof_idx,
                    kind,
                    primary,
                    secondary,
                    speed,
                    direction,
                    new_name.as_deref(),
                )
                .map_err(|e| e.to_string())?;
                self.config.active_profile_mut().enable_rgb = false;
                self.persist_config();
                Ok(self.full_state_json())
            }
            "asus_set_direct" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                self.config.active_profile_mut().sync_asus_aura = on;
                if let Some(aura) = self.asus_aura.as_mut() {
                    if on {
                        aura.enter_direct_mode().map_err(|e| e.to_string())?;
                    } else {
                        aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false)
                            .map_err(|e| e.to_string())?;
                    }
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "asus_set_hw_mode" => {
                let mode_str = p["mode"].as_str().unwrap_or("Static");
                let color = p["color"].as_str().and_then(Rgb::from_hex).unwrap_or(self.aura_hw_color);
                let commit = p["commit"].as_bool().unwrap_or(false);
                let mode = parse_aura_mode(mode_str);
                self.aura_hw_mode = mode;
                self.aura_hw_color = color;
                self.config.active_profile_mut().sync_asus_aura = false;
                let aura = self.asus_aura.as_mut().ok_or("ASUS Aura non connecté")?;
                aura.set_all_hw_mode(mode, color, false)
                    .map_err(|e| e.to_string())?;
                if commit {
                    aura.commit().map_err(|e| e.to_string())?;
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "asus_set_shutdown_mode" => {
                let mode_str = p["mode"].as_str().unwrap_or("Off");
                let color = p["color"].as_str().and_then(Rgb::from_hex).unwrap_or(Rgb::BLACK);
                let mode = parse_aura_mode(mode_str);
                self.aura_shutdown_mode = mode;
                self.aura_shutdown_color = color;
                let aura = self.asus_aura.as_mut().ok_or("ASUS Aura non connecté")?;
                aura.set_all_hw_mode(mode, color, true)
                    .map_err(|e| e.to_string())?;
                aura.commit().map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "asus_set_led_count" => {
                let ch = p["channel"].as_u64().ok_or("channel manquant")? as usize;
                let count = (p["count"].as_u64().ok_or("count manquant")? as usize).clamp(1, 120);
                if let Some(aura) = self.asus_aura.as_mut() {
                    aura.set_channel_led_count(ch, count);
                }
                self.config
                    .active_profile_mut()
                    .asus_argb_led_counts
                    .insert(ch, count);
                self.persist_config();
                Ok(self.full_state_json())
            }
            "ram_scan" => {
                self.ram_rgb = probe_ram_rgb();
                Ok(self.full_state_json())
            }
            "ram_apply_rgb" => {
                let mode_str = p["mode"].as_str().unwrap_or("Static");
                let color_hex = p["color"].as_str().unwrap_or("#00D2FF");
                let color = Rgb::from_hex(color_hex).unwrap_or(Rgb::CYAN);
                let speed = p["speed"].as_u64().unwrap_or(2).min(4) as u8;
                let reverse = p["reverse"].as_bool().unwrap_or(false);
                let save_eeprom = p["save_eeprom"].as_bool().unwrap_or(false);
                let target_addr = p["address"].as_u64().map(|v| v as u8);
                let mode = EneRamMode::from_str_id(mode_str);
                if self.ram_rgb.ene_modules.is_empty() && self.ram_rgb.is_admin {
                    self.ram_rgb = probe_ram_rgb();
                }
                apply_ram_rgb(
                    &self.ram_rgb.ene_modules,
                    target_addr,
                    mode,
                    color,
                    speed,
                    reverse,
                    save_eeprom,
                )
                .map_err(|e| e.to_string())?;
                self.ram_rgb.last_mode = mode.id().to_string();
                self.ram_rgb.last_color = color.to_hex();
                self.ram_rgb.last_speed = speed;
                for m in self.ram_rgb.ene_modules.iter_mut() {
                    if target_addr.map_or(true, |a| a == m.address_7bit) {
                        m.current_mode = mode as u8;
                    }
                }
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
            "logi_set_pollrate" => {
                let hz = p["hz"].as_u64().ok_or("Hz manquant")? as u16;
                let m = self.logi_mouse_mut(p)?;
                m.set_report_rate_hz(hz).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_set_mode" => {
                let om = match p["mode"].as_str().unwrap_or("Onboard") {
                    "Host" => OnboardMode::Host,
                    _ => OnboardMode::Onboard,
                };
                let m = self.logi_mouse_mut(p)?;
                m.set_onboard_mode(om).map_err(|e| e.to_string())?;
                m.refresh_telemetry();
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
                        if i < prof.buttons.len() {
                            if let Some(hex) = b.as_str() {
                                if let Some(raw) = parse_4byte_hex(hex) {
                                    prof.buttons[i] = MouseButtonAction::decode(raw);
                                }
                            }
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
            "logi_set_angle_snapping" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                let m = self.logi_mouse_mut(p)?;
                m.set_angle_snapping(on).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_set_led" => {
                let mode = match p["mode"].as_str().unwrap_or("Fixed") {
                    "Off" => MouseLedMode::Off,
                    "Breathing" => MouseLedMode::Breathing,
                    "Cycle" => MouseLedMode::Cycle,
                    _ => MouseLedMode::Fixed,
                };
                let color = p["color"].as_str().and_then(Rgb::from_hex).unwrap_or(Rgb::CYAN);
                let m = self.logi_mouse_mut(p)?;
                let zones: Vec<u8> = m.info.led_zones.iter().map(|z| z.index).collect();
                for zid in zones {
                    m.set_led_zone(zid, mode, color, 4000)
                        .map_err(|e| e.to_string())?;
                }
                Ok(self.full_state_json())
            }
            "logi_kb_set_rgb" => {
                let mode = match p["mode"].as_str().unwrap_or("Fixed") {
                    "Off" => KeyboardRgbMode::Off,
                    "Breathing" => KeyboardRgbMode::Breathing,
                    "Cycle" => KeyboardRgbMode::Cycle,
                    "Wave" => KeyboardRgbMode::Wave,
                    _ => KeyboardRgbMode::Fixed,
                };
                let color = p["color"].as_str().and_then(Rgb::from_hex).unwrap_or(Rgb::CYAN);
                let kb = self.logi_kb_mut(p)?;
                kb.set_rgb_effect(mode, color, 4000)
                    .map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_kb_set_brightness" => {
                let pct = p["percent"].as_u64().unwrap_or(100) as u8;
                let kb = self.logi_kb_mut(p)?;
                kb.set_brightness_pct(pct).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
            }
            "logi_set_sidetone" => {
                let pct = p["percent"].as_u64().unwrap_or(0) as u8;
                let h = self.logi_headset_mut(p)?;
                h.set_sidetone(pct).map_err(|e| e.to_string())?;
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
                    return Err("Droits Admin + ASUSManagement WMI requis pour relire les courbes BIOS".into());
                }
                self.cooling_channels = opengear_asus::FanHeaderKind::ALL
                    .iter()
                    .map(|&h| {
                        read_asus_wmi_channel(h)
                            .unwrap_or_else(|| opengear_asus::FanChannelConfig::default_for(h))
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
                        ch.curve[i] = CurvePoint { temp_c: t, duty_pct: d };
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
                    ch.curve = opengear_asus::FanChannelConfig::silent_loop3_preset(preset);
                    if self.cooling_status.is_admin {
                        let _ = apply_asus_wmi_curve(ch);
                    }
                }
                Ok(self.full_state_json())
            }
            "cooling_sl3_argb" => {
                if let Some(ch) = p["channel"].as_u64() {
                    self.config.active_profile_mut().silent_loop3_argb_channel = Some(ch as usize);
                }
                if let Some(thermal) = p["thermal"].as_bool() {
                    self.config.active_profile_mut().silent_loop3_thermal_rgb = thermal;
                    if thermal {
                        if let Some(aura) = self.asus_aura.as_mut() {
                            let _ = aura.enter_direct_mode();
                        }
                    }
                }
                if let Some(hex) = p["color"].as_str() {
                    if let Some(col) = Rgb::from_hex(hex) {
                        self.config.active_profile_mut().silent_loop3_thermal_rgb = false;
                        let ch_idx = self
                            .config
                            .active_profile_mut()
                            .silent_loop3_argb_channel
                            .unwrap_or(1);
                        if let Some(aura) = self.asus_aura.as_mut() {
                            let _ = aura.enter_direct_mode();
                            let count = aura
                                .info
                                .channels
                                .get(ch_idx)
                                .map(|c| c.led_count)
                                .unwrap_or(16);
                            let leds = vec![col; count];
                            aura.send_channel_direct(ch_idx, &leds)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
                self.persist_config();
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
                self.config.active_profile_mut().bindings.insert(key, action);
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
                                SocdPair { key_a: "A".into(), key_b: "D".into() },
                                SocdPair { key_a: "W".into(), key_b: "S".into() },
                            ],
                            "qd_only" => vec![SocdPair { key_a: "Q".into(), key_b: "D".into() }],
                            "ad_only" => vec![SocdPair { key_a: "A".into(), key_b: "D".into() }],
                            _ => vec![
                                SocdPair { key_a: "Q".into(), key_b: "D".into() },
                                SocdPair { key_a: "Z".into(), key_b: "S".into() },
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
                self.sync_input_rules();
                self.persist_config();
                Ok(self.full_state_json())
            }
            "omen_refresh" => {
                self.hp_omen = probe_hp_omen();
                if self.is_laptop || self.laptop_battery.present {
                    self.laptop_battery = read_laptop_battery();
                    self.laptop_display = read_display_info();
                }
                Ok(self.full_state_json())
            }
            "omen_set_thermal_mode" => {
                let mode = OmenThermalMode::from_id(p["mode"].as_str().unwrap_or("default"));
                set_hp_omen_thermal_mode(self.hp_omen.thermal_version, mode)
                    .map_err(|e| e.to_string())?;
                self.hp_omen.thermal_mode = mode;
                self.hp_omen.manual_fan_pct = None;
                Ok(self.full_state_json())
            }
            "omen_set_max_fan" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                set_hp_omen_max_fan(on).map_err(|e| e.to_string())?;
                self.hp_omen.max_fan = on;
                if on {
                    self.hp_omen.manual_fan_pct = None;
                    self.omen_fan_modified = true;
                }
                Ok(self.full_state_json())
            }
            "omen_set_manual_fan" => {
                let pct = p["pct"].as_u64().map(|v| (v as u8).clamp(30, 100));
                set_hp_omen_fan_level(pct, &self.hp_omen.fan_max_rpms)
                    .map_err(|e| e.to_string())?;
                self.hp_omen.manual_fan_pct = pct;
                self.omen_last_fan_keepalive = std::time::Instant::now();
                if pct.is_some() {
                    self.omen_fan_modified = true;
                }
                Ok(self.full_state_json())
            }
            "omen_set_gpu_power" => {
                let lvl = OmenGpuPowerLevel::from_id(p["level"].as_str().unwrap_or("min"));
                set_hp_omen_gpu_power(lvl).map_err(|e| e.to_string())?;
                self.hp_omen.gpu_power = lvl;
                Ok(self.full_state_json())
            }
            "omen_set_kb_backlight" => {
                if !self.hp_omen.keyboard_type.is_software_rgb() {
                    return Err("Le rétroéclairage de ce clavier OMEN n'est pas pilotable par logiciel (utilise Fn + F4).".into());
                }
                let on = p["enabled"].as_bool().unwrap_or(true);
                set_hp_omen_kb_backlight(on).map_err(|e| e.to_string())?;
                self.hp_omen.backlight_on = on;
                Ok(self.full_state_json())
            }
            "omen_set_kb_zones" => {
                let arr = p["colors"].as_array().ok_or("colors manquant")?;
                if arr.len() != 4 {
                    return Err("Exactement 4 couleurs attendues".into());
                }
                let mut cols = [Rgb::CYAN; 4];
                for i in 0..4 {
                    let hex = arr[i].as_str().ok_or("Couleur zone invalide")?;
                    cols[i] = Rgb::from_hex(hex).ok_or("Hex zone invalide")?;
                    self.config.omen_zone_colors[i] = cols[i].to_hex();
                }
                self.config.omen_sync_rgb = false;
                let tbl = set_hp_omen_kb_zones(cols, self.omen_color_table.as_deref())
                    .map_err(|e| e.to_string())?;
                self.omen_color_table = Some(tbl);
                self.hp_omen.zone_colors = cols;
                if !self.hp_omen.backlight_on {
                    if set_hp_omen_kb_backlight(true).is_ok() {
                        self.hp_omen.backlight_on = true;
                    }
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "omen_set_sync_rgb" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                self.config.omen_sync_rgb = on;
                if on {
                    if !self.hp_omen.backlight_on && self.hp_omen.wmi_accessible {
                        let _ = set_hp_omen_kb_backlight(true);
                        self.hp_omen.backlight_on = true;
                    }
                    self.omen_last_rgb_push =
                        std::time::Instant::now() - std::time::Duration::from_secs(1);
                }
                self.persist_config();
                Ok(self.full_state_json())
            }
            "laptop_set_brightness" => {
                let pct = p["percent"].as_u64().ok_or("percent manquant")?.min(100) as u8;
                set_display_brightness(pct).map_err(|e| format!("{e:#}"))?;
                self.laptop_display.brightness_pct = Some(pct);
                Ok(self.full_state_json())
            }
            "laptop_set_refresh_rate" => {
                let hz = p["hz"].as_u64().ok_or("hz manquant")? as u32;
                let applied = set_display_refresh_rate(hz).map_err(|e| e.to_string())?;
                self.laptop_display = read_display_info();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(format!("Écran basculé sur {applied} Hz"));
                Ok(st)
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
        "Off" => Some(EffectKind::Off),
        _ => None,
    }
}

fn parse_aura_mode(s: &str) -> AuraHwMode {
    match s {
        "Off" => AuraHwMode::Off,
        "Static" => AuraHwMode::Static,
        "Breathing" => AuraHwMode::Breathing,
        "Flashing" => AuraHwMode::Flashing,
        "SpectrumCycle" => AuraHwMode::SpectrumCycle,
        "Rainbow" => AuraHwMode::Rainbow,
        "SpectrumCycleBreathing" => AuraHwMode::SpectrumCycleBreathing,
        "ChaseFade" => AuraHwMode::ChaseFade,
        "SpectrumCycleChaseFade" => AuraHwMode::SpectrumCycleChaseFade,
        "Chase" => AuraHwMode::Chase,
        "SpectrumCycleChase" => AuraHwMode::SpectrumCycleChase,
        "Wave" => AuraHwMode::Wave,
        "ChaseRainbowPulse" => AuraHwMode::ChaseRainbowPulse,
        "RandomFlicker" => AuraHwMode::RandomFlicker,
        "GentleTransition" => AuraHwMode::GentleTransition,
        "WavePropagation" => AuraHwMode::WavePropagation,
        "RedPulse" => AuraHwMode::RedPulse,
        _ => AuraHwMode::Static,
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
    use opengear_asus::FanTempSource;

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
        let before = [(0x046D, 0xC547, 0xFF00), (0x0B05, 0x19AF, 0xFF72), (0x1B1C, 0x1B73, 0xFF42)];
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
