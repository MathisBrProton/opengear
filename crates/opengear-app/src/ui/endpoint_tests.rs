//! Systematic in-process IPC & driver capability endpoint self-test suite.
//!
//! Runs both under `cargo test` and during live `opengear selftest` / UI auto-test audits using a
//! disposable temp config path and mock driver devices so zero hardware flash, live user profiles,
//! or OS input hooks are disturbed while every command handler branch, schema field, validation
//! path, and lighting/cooling invariant is verified and logged to `opengear.log`.

use super::backend::BackendEngine;
use opengear_core::driver::{
    ChoiceOption, Connection, Control, Device, DeviceInfo, DeviceKind, DeviceStatus, Driver,
    Section, Setting, Value as DriverValue, setting_ids,
};
use opengear_core::fans::{Cooling, FanInfo, FanTarget};
use opengear_core::lighting::{
    EffectParams, EffectRequest, HardwareEffect, Led, Lighting, Zone, ZoneKind,
};
use opengear_core::{AppConfig, Rgb};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{error, info};

#[derive(Debug, Clone)]
pub(crate) struct EndpointCheckResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
    pub elapsed_ms: u128,
}

#[derive(Default)]
struct MockDevState {
    frames: Vec<Vec<Rgb>>,
    hw_applied: Vec<EffectRequest>,
    released: usize,
    fan_targets: Vec<(Option<String>, FanTarget)>,
    fan_restored: usize,
    fan_overridden: bool,
    polling_choice: String,
    winlock: bool,
    brightness: f64,
    ch1_leds: u16,
}

struct MockAuditDriver {
    id: String,
    state: Arc<Mutex<MockDevState>>,
}

impl Driver for MockAuditDriver {
    fn name(&self) -> &'static str {
        "mock_audit"
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        vec![Box::new(MockFullDevice {
            id: self.id.clone(),
            state: Arc::clone(&self.state),
        })]
    }
}

struct MockExpDriver;

impl Driver for MockExpDriver {
    fn name(&self) -> &'static str {
        "mock_exp"
    }

    fn experimental(&self) -> bool {
        true
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        Vec::new()
    }
}

struct MockFullDevice {
    id: String,
    state: Arc<Mutex<MockDevState>>,
}

impl MockFullDevice {
    fn build(id: &str) -> (Box<dyn Device>, Arc<Mutex<MockDevState>>) {
        let state = Arc::new(Mutex::new(MockDevState {
            polling_choice: "1000".into(),
            winlock: false,
            brightness: 80.0,
            ch1_leds: 8,
            ..Default::default()
        }));
        (
            Box::new(Self {
                id: id.to_string(),
                state: Arc::clone(&state),
            }),
            state,
        )
    }
}

impl Device for MockFullDevice {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.id.clone(),
            vendor: "Audit".into(),
            model: "Mock Periph".into(),
            kind: DeviceKind::Motherboard,
            connection: Connection::Usb,
            firmware: Some("1.0.0".into()),
        }
    }

    fn status(&self) -> DeviceStatus {
        DeviceStatus {
            battery_pct: Some(75),
            ..Default::default()
        }
    }

    fn settings(&self) -> Vec<Setting> {
        let st = self.state.lock().unwrap();
        vec![
            Setting::choice(
                setting_ids::POLLING_RATE,
                "Fréquence",
                Section::Input,
                vec![
                    ChoiceOption::new("1000", "1000 Hz"),
                    ChoiceOption::new("4000", "4000 Hz"),
                ],
                &st.polling_choice,
            ),
            Setting::toggle(
                setting_ids::WIN_LOCK,
                "Verrou Windows",
                Section::Input,
                st.winlock,
            ),
            Setting::slider(
                setting_ids::HW_BRIGHTNESS,
                "Luminosité",
                Section::Lighting,
                (0.0, 100.0, 5.0),
                "%",
                st.brightness,
            ),
        ]
    }

    fn apply_setting(&mut self, id: &str, value: &DriverValue) -> anyhow::Result<()> {
        let mut st = self.state.lock().unwrap();
        match (id, value) {
            (setting_ids::POLLING_RATE, DriverValue::Text(v)) => st.polling_choice = v.clone(),
            (setting_ids::WIN_LOCK, DriverValue::Bool(v)) => st.winlock = *v,
            (setting_ids::HW_BRIGHTNESS, DriverValue::Number(v)) => st.brightness = *v,
            _ => anyhow::bail!("Réglage inconnu : {id}"),
        }
        Ok(())
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        Some(self)
    }

    fn cooling(&mut self) -> Option<&mut dyn Cooling> {
        Some(self)
    }
}

impl Lighting for MockFullDevice {
    fn zones(&self) -> Vec<Zone> {
        let ch1_n = self.state.lock().unwrap().ch1_leds.clamp(1, 60) as usize;
        vec![
            Zone {
                id: "ch_0".into(),
                name: "Zone A".into(),
                kind: ZoneKind::Single,
                leds: (0..4)
                    .map(|i| Led::at(format!("Logo {i}"), i as f32 / 3.0, 0.2))
                    .collect(),
                resizable: None,
            },
            Zone {
                id: "ch_1".into(),
                name: "Zone B".into(),
                kind: ZoneKind::Linear,
                leds: (0..ch1_n)
                    .map(|i| {
                        let nx = if ch1_n <= 1 {
                            0.5
                        } else {
                            i as f32 / (ch1_n - 1) as f32
                        };
                        Led::at(format!("Strip {i}"), nx, 0.8)
                    })
                    .collect(),
                resizable: Some((1, 60)),
            },
        ]
    }

    fn resize_zone(&mut self, zone_id: &str, led_count: u16) -> anyhow::Result<()> {
        if zone_id != "ch_1" {
            anyhow::bail!("Zone {zone_id} non redimensionnable");
        }
        self.state.lock().unwrap().ch1_leds = led_count.clamp(1, 60);
        Ok(())
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        vec![
            HardwareEffect {
                id: "Static".into(),
                label: "Statique".into(),
                params: EffectParams {
                    colors: (1, 1),
                    speed: None,
                    brightness: None,
                    direction: false,
                    savable: true,
                },
                zones: vec!["ch_0".into(), "ch_1".into()],
            },
            HardwareEffect {
                id: "Off".into(),
                label: "Éteint".into(),
                params: EffectParams::default(),
                zones: vec!["ch_0".into(), "ch_1".into()],
            },
        ]
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> anyhow::Result<()> {
        self.state.lock().unwrap().frames.push(colors.to_vec());
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> anyhow::Result<()> {
        self.state.lock().unwrap().hw_applied.push(req.clone());
        Ok(())
    }

    fn release(&mut self) -> anyhow::Result<()> {
        self.state.lock().unwrap().released += 1;
        Ok(())
    }
}

impl Cooling for MockFullDevice {
    fn fans(&self) -> Vec<FanInfo> {
        vec![FanInfo {
            id: "fan_0".into(),
            name: "Ventilateur Test".into(),
            rpm: Some(1200),
            max_rpm: Some(2400),
            min_pct: 25,
            manual_pct: self.state.lock().unwrap().fan_overridden.then_some(50),
        }]
    }

    fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> anyhow::Result<()> {
        let mut st = self.state.lock().unwrap();
        if let Some(id) = fan
            && id != "fan_0"
        {
            anyhow::bail!("Ventilateur inconnu : {id}");
        }
        st.fan_overridden = matches!(target, FanTarget::Fixed(_));
        st.fan_targets.push((fan.map(str::to_string), target));
        Ok(())
    }

    fn restore_firmware_control(&mut self) -> anyhow::Result<()> {
        let mut st = self.state.lock().unwrap();
        st.fan_overridden = false;
        st.fan_restored += 1;
        Ok(())
    }

    fn overridden(&self) -> bool {
        self.state.lock().unwrap().fan_overridden
    }
}

fn check<F>(results: &mut Vec<EndpointCheckResult>, name: &str, f: F)
where
    F: FnOnce() -> Result<String, String>,
{
    let t0 = Instant::now();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let elapsed_ms = t0.elapsed().as_millis();
    let (passed, detail) = match res {
        Ok(Ok(msg)) => {
            info!(endpoint = name, elapsed_ms, detail = %msg, "Endpoint auto-test PASS");
            (true, msg)
        }
        Ok(Err(err)) => {
            error!(endpoint = name, elapsed_ms, error = %err, "Endpoint auto-test FAIL");
            (false, err)
        }
        Err(_) => {
            error!(endpoint = name, elapsed_ms, "Endpoint auto-test PANIC");
            (false, "Panique Rust interceptée".into())
        }
    };
    results.push(EndpointCheckResult {
        name: name.to_string(),
        passed,
        detail,
        elapsed_ms,
    });
}

fn cmd_ok(eng: &mut BackendEngine, cmd: &str, payload: Value) -> Result<Value, String> {
    eng.handle_cmd(cmd, &payload, None)
        .map_err(|e| format!("{cmd} -> {e}"))
}

fn cmd_err(eng: &mut BackendEngine, cmd: &str, payload: Value) -> Result<String, String> {
    match eng.handle_cmd(cmd, &payload, None) {
        Ok(_) => Err(format!("{cmd} aurait dû échouer mais a réussi")),
        Err(e) => Ok(e),
    }
}

/// Run the full IPC endpoint audit inside a disposable temp workspace (`%TEMP%`) and return every
/// endpoint result with latency and detail. Safe to call in production `selftest` and unit tests.
pub(crate) fn run_all_endpoint_checks() -> Vec<EndpointCheckResult> {
    let mut results = Vec::new();
    let temp_cfg = std::env::temp_dir().join(format!(
        "opengear_endpoint_audit_{}_{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let (mock_dev, mock_state) = MockFullDevice::build("audit:mock_0");
    let mut eng =
        BackendEngine::new_headless(temp_cfg.clone(), AppConfig::default(), vec![mock_dev]);
    eng.drivers.push(Box::new(MockAuditDriver {
        id: "audit:mock_0".into(),
        state: Arc::clone(&mock_state),
    }));
    eng.drivers.push(Box::new(MockExpDriver));

    // Validate mock setting descriptor consistency cleanly.
    check(&mut results, "driver_settings_contract", || {
        for d in &eng.driver_devices {
            for s in d.settings() {
                if let Control::Slider { min, max, .. } = s.control
                    && let DriverValue::Number(v) = s.value
                    && (v < min || v > max)
                {
                    return Err(format!("{} hors bornes [{min}, {max}]: {v}", s.id));
                }
            }
        }
        Ok("Descripteurs settings valides".into())
    });

    // 1. Read-only / Query IPC endpoints
    check(&mut results, "get_state", || {
        let st = cmd_ok(&mut eng, "get_state", json!({}))?;
        for key in [
            "static_info",
            "app_version",
            "config",
            "devices",
            "device_list",
            "telemetry",
            "cooling",
            "keyboard",
            "power_plans",
            "experimental_drivers",
        ] {
            if st.get(key).is_none() {
                return Err(format!("Champ manquant dans get_state : {key}"));
            }
        }
        if st["config"]["profiles"].is_null() || st["config"]["active_profile"].is_null() {
            return Err("config.profiles ou config.active_profile absent".into());
        }
        Ok("Schéma complet présent".into())
    });

    for read_cmd in [
        "get_catalog",
        "get_telemetry",
        "get_diagnostics",
        "get_diagnostic_report",
        "input_socd_live",
        "list_backups",
        "corsair_list_backups",
        "scan_hid",
        "superio_refresh",
    ] {
        check(&mut results, read_cmd, || {
            let v = cmd_ok(&mut eng, read_cmd, json!({}))?;
            Ok(format!("JSON OK ({} octets)", v.to_string().len()))
        });
    }

    // 2. Unified Device Settings / Fan / Experimental Driver / Save
    check(&mut results, "device_set + errors", || {
        cmd_ok(
            &mut eng,
            "device_set",
            json!({ "device": "audit:mock_0", "setting": setting_ids::POLLING_RATE, "value": "4000" }),
        )?;
        cmd_ok(
            &mut eng,
            "device_set",
            json!({ "device": "audit:mock_0", "setting": setting_ids::WIN_LOCK, "value": true }),
        )?;
        cmd_ok(
            &mut eng,
            "device_set",
            json!({ "device": "audit:mock_0", "setting": setting_ids::HW_BRIGHTNESS, "value": 45.0 }),
        )?;
        cmd_err(
            &mut eng,
            "device_set",
            json!({ "device": "audit:unknown", "setting": "x", "value": true }),
        )?;
        cmd_err(
            &mut eng,
            "device_set",
            json!({ "device": "audit:mock_0", "setting": "unknown_setting", "value": true }),
        )?;
        let st = mock_state.lock().unwrap();
        if st.polling_choice != "4000" || !st.winlock || (st.brightness - 45.0).abs() > 0.01 {
            return Err("État interne device_set incohérent".into());
        }
        Ok("Toggle/Slider/Choice + rejet erreurs OK".into())
    });

    check(&mut results, "device_fan_set + floor_guard", || {
        // Values below min_pct (25%) must be clamped up to the safety floor.
        cmd_ok(
            &mut eng,
            "device_fan_set",
            json!({ "device": "audit:mock_0", "fan": "fan_0", "target": 10 }),
        )?;
        cmd_ok(
            &mut eng,
            "device_fan_set",
            json!({ "device": "audit:mock_0", "fan": "fan_0", "target": "auto" }),
        )?;
        cmd_err(
            &mut eng,
            "device_fan_set",
            json!({ "device": "audit:missing", "target": "auto" }),
        )?;
        let st = mock_state.lock().unwrap();
        if st.fan_targets.first().map(|t| t.1) != Some(FanTarget::Fixed(25)) {
            return Err(format!("Plancher non appliqué : {:?}", st.fan_targets));
        }
        Ok("PWM plancher (10->25%) + retour Auto + rejet inconnu OK".into())
    });

    check(
        &mut results,
        "device_set_dpi + device_set_eq (errors)",
        || {
            cmd_err(
                &mut eng,
                "device_set_dpi",
                json!({ "device": "audit:mock_0", "x": 800 }),
            )?;
            cmd_err(
                &mut eng,
                "device_set_eq",
                json!({ "device": "audit:mock_0", "gains": [0, 0, 0, 0, 0] }),
            )?;
            Ok("Rejets propres quand capacité absente".into())
        },
    );

    check(
        &mut results,
        "set_experimental_driver + save_config",
        || {
            cmd_ok(
                &mut eng,
                "set_experimental_driver",
                json!({ "name": "mock_exp", "enabled": true }),
            )?;
            cmd_ok(
                &mut eng,
                "set_experimental_driver",
                json!({ "name": "mock_exp", "enabled": false }),
            )?;
            cmd_err(
                &mut eng,
                "set_experimental_driver",
                json!({ "name": "unknown_driver", "enabled": true }),
            )?;
            cmd_ok(&mut eng, "reconnect_all", json!({}))?;
            cmd_ok(&mut eng, "save_config", json!({}))?;
            let (reloaded, warn) = AppConfig::load_or_recover(&temp_cfg);
            if warn.is_some() || reloaded.profiles.is_empty() {
                return Err(format!("Rechargement TOML défaillant : {warn:?}"));
            }
            Ok("Config persistée et rechargée proprement".into())
        },
    );

    // 3. Profiles, Presets, Undo
    check(
        &mut results,
        "profiles (create/switch/export/import/apps)",
        || {
            cmd_ok(&mut eng, "create_profile", json!({ "name": "AuditProfil" }))?;
            cmd_ok(&mut eng, "switch_profile", json!({ "name": "AuditProfil" }))?;
            cmd_ok(
                &mut eng,
                "profile_set_linked_apps",
                json!({ "apps": ["Notepad.exe", " "] }),
            )?;
            let exported = cmd_ok(&mut eng, "profile_export_json", json!({}))?;
            let raw = exported["json"]
                .as_str()
                .ok_or("profile_export_json sans champ json")?
                .to_string();
            cmd_ok(&mut eng, "profile_import_json", json!({ "json": raw }))?;
            cmd_err(&mut eng, "profile_import_json", json!({ "json": "{" }))?;
            cmd_err(
                &mut eng,
                "profile_import_json",
                json!({ "json": serde_json::json!({ "name": "   " }).to_string() }),
            )?;
            let prof = eng.config.active_profile_mut();
            if prof.linked_apps != vec!["notepad.exe".to_string()] {
                return Err("linked_apps non normalisé".into());
            }
            Ok("Roundtrip profil + rejet JSON invalide/nom vide OK".into())
        },
    );

    check(&mut results, "apply_preset + undo_last", || {
        for preset in ["silent", "gaming", "office", "lights_off"] {
            cmd_ok(&mut eng, "apply_preset", json!({ "preset": preset }))?;
        }
        cmd_err(&mut eng, "apply_preset", json!({ "preset": "inexistant" }))?;
        cmd_ok(&mut eng, "undo_last", json!({}))?;
        Ok("4 préréglages + annulation undo OK".into())
    });

    // 4. Lighting & RGB endpoints + mixed Hardware/Global render verification
    check(&mut results, "lighting_* & rgb_* & live_frame", || {
        cmd_ok(
            &mut eng,
            "set_rgb_config",
            json!({ "kind": "Static", "color": "#112233", "brightness": 0.8, "speed": 1.0, "commit": true }),
        )?;
        cmd_ok(
            &mut eng,
            "set_rgb_layers",
            json!({ "layers": [{ "name": "Calque", "enabled": true, "opacity": 0.5, "blend": "Add", "key_filter": ["KeyA"], "effect": { "kind": "Static", "color": "#FF0000" } }] }),
        )?;
        cmd_ok(
            &mut eng,
            "set_per_key_color",
            json!({ "key_code": "KeyQ", "color": "#00FF00" }),
        )?;
        cmd_ok(&mut eng, "clear_per_key", json!({}))?;
        cmd_ok(
            &mut eng,
            "lighting_resize_zone",
            json!({ "device_id": "audit:mock_0", "zone_id": "ch_1", "led_count": 10 }),
        )?;
        cmd_ok(
            &mut eng,
            "lighting_set_placement",
            json!({ "device_id": "audit:mock_0", "x": 0.5, "y": 0.5, "scale": 1.0 }),
        )?;
        cmd_ok(
            &mut eng,
            "lighting_set_device",
            json!({
                "device_id": "audit:mock_0",
                "zone_id": "ch_0",
                "override": {
                    "mode": "hardware",
                    "request": { "effect": "Static", "colors": ["#FF8800"], "save": false }
                }
            }),
        )?;
        eng.invalidate_rgb_frames();
        eng.push_live_rgb_frame();
        // Changing POLLING_RATE while live RGB is active must invalidate & re-push the LED frame.
        cmd_ok(&mut eng, "lighting_sync_all", json!({ "enabled": true }))?;
        eng.invalidate_rgb_frames();
        eng.push_live_rgb_frame();
        let before_poll_frames = mock_state.lock().unwrap().frames.len();
        cmd_ok(
            &mut eng,
            "device_set",
            json!({ "device": "audit:mock_0", "setting": setting_ids::POLLING_RATE, "value": "1000" }),
        )?;
        let after_poll_frames = mock_state.lock().unwrap().frames.len();
        cmd_ok(&mut eng, "lighting_sync_all", json!({ "enabled": false }))?;
        let st = mock_state.lock().unwrap();
        let has_preserved = st
            .frames
            .first()
            .is_some_and(|f| f.len() == 14 && f[0] == Rgb::new(0xFF, 0x88, 0x00));
        if after_poll_frames <= before_poll_frames {
            return Err("POLLING_RATE n'a pas réémis la trame RGB après ré-énumération USB".into());
        }
        if !has_preserved {
            return Err("La zone Hardware mixte n'a pas conservé sa couleur #FF8800".into());
        }
        if st.released == 0 || st.hw_applied.len() < 2 {
            return Err("release_device_lighting ou apply_hardware_effect non appelé".into());
        }
        drop(st);
        cmd_err(
            &mut eng,
            "lighting_set_device",
            json!({
                "device_id": "missing",
                "override": { "mode": "hardware", "request": { "effect": "Static" } }
            }),
        )?;
        Ok("Sync/Mixed/Release/Resize/Layers vérifiés".into())
    });

    // 5. Input / Keyboard / SOCD / Macros / Layouts
    check(&mut results, "input_* & keyboard_layout", || {
        cmd_ok(
            &mut eng,
            "set_keyboard_layout",
            json!({ "layout": "Tkl", "norme": "Iso" }),
        )?;
        cmd_ok(&mut eng, "input_set_enabled", json!({ "enabled": false }))?;
        cmd_ok(&mut eng, "input_set_winlock", json!({ "enabled": true }))?;
        cmd_ok(
            &mut eng,
            "input_set_binding",
            json!({ "key": "CapsLock", "kind": "remap", "target": "Escape" }),
        )?;
        cmd_ok(
            &mut eng,
            "input_set_layer_binding",
            json!({ "layer": "Fn", "key": "Q", "kind": "text", "text": "azerty" }),
        )?;
        cmd_ok(
            &mut eng,
            "input_set_mouse_binding",
            json!({ "button": "Back", "kind": "disable" }),
        )?;
        cmd_ok(
            &mut eng,
            "input_set_mouse_layer_binding",
            json!({ "layer": "Fn", "button": "Forward", "kind": "disable" }),
        )?;
        for preset in ["qwerty", "qd_only", "ad_only", "azerty"] {
            cmd_ok(
                &mut eng,
                "input_set_esports",
                json!({
                    "socd_enabled": false,
                    "socd_preset": preset,
                    "blocked_apps": ["Valorant.exe"],
                    "key_chatter_ms": 12,
                    "mouse_debounce_ms": 8
                }),
            )?;
        }
        cmd_ok(
            &mut eng,
            "input_remove_binding",
            json!({ "key": "CapsLock" }),
        )?;
        cmd_ok(
            &mut eng,
            "input_remove_layer_binding",
            json!({ "layer": "Fn", "key": "Q" }),
        )?;
        cmd_ok(
            &mut eng,
            "input_remove_mouse_binding",
            json!({ "button": "Back" }),
        )?;
        cmd_ok(
            &mut eng,
            "input_remove_mouse_layer_binding",
            json!({ "layer": "Fn", "button": "Forward" }),
        )?;
        cmd_ok(&mut eng, "input_macro_record_start", json!({}))?;
        cmd_ok(&mut eng, "input_macro_record_stop", json!({}))?;
        Ok("Remap/Couches/SOCD/Macros/Layouts OK".into())
    });

    // 6. System, Cooling, Detached HW & Error Paths
    check(&mut results, "sys/cooling/power_plan/unknown_cmd", || {
        cmd_ok(
            &mut eng,
            "set_ui_prefs",
            json!({ "ui_mode": "expert", "language": "en", "onboarding_done": true }),
        )?;
        cmd_ok(&mut eng, "set_ui_prefs", json!({ "language": "fr" }))?;
        cmd_ok(
            &mut eng,
            "sys_set_prefs",
            json!({
                "lights_off_on_lock": false,
                "idle_lights_off_min": 15,
                "battery_alerts": true,
                "auto_profile_switch": false,
                "openrgb_server": false
            }),
        )?;
        let anon = cmd_ok(&mut eng, "export_anonymous_report", json!({}))?;
        if anon["text"].as_str().is_none_or(|t| t.is_empty()) {
            return Err("export_anonymous_report vide".into());
        }
        let _ = std::fs::remove_file(std::env::temp_dir().join("opengear_anonymous_report.txt"));
        cmd_ok(
            &mut eng,
            "laptop_set_auto_battery",
            json!({ "enabled": true }),
        )?;
        cmd_ok(
            &mut eng,
            "cooling_update_channel",
            json!({
                "index": 0,
                "policy": "Silent",
                "source": "MaxCpuGpu",
                "hysteresis_c": 3,
                "smoothing_secs": 4,
                "curve": [{ "temp_c": 40, "duty_pct": 50 }],
                "apply": false
            }),
        )?;
        cmd_ok(
            &mut eng,
            "cooling_sl3_preset",
            json!({ "preset": "ultra_silent" }),
        )?;
        cmd_err(&mut eng, "cooling_update_channel", json!({ "index": 99 }))?;
        cmd_err(&mut eng, "cooling_read_bios_curves", json!({}))?;
        cmd_err(
            &mut eng,
            "set_power_plan",
            json!({ "guid": "invalid-guid" }),
        )?;
        cmd_err(
            &mut eng,
            "corsair_set_indicator_color",
            json!({ "indicator": "winlock_on", "color": "#FFFFFF" }),
        )?;
        cmd_err(&mut eng, "logi_set_dpi", json!({ "x": 800 }))?;
        cmd_err(&mut eng, "__nonexistent_endpoint__", json!({}))?;
        Ok("Validation courbes, GUID, périphériques absents & commande inconnue OK".into())
    });

    let _ = std::fs::remove_file(&temp_cfg);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ipc_endpoints_succeed_and_validate_errors() {
        let results = run_all_endpoint_checks();
        assert!(!results.is_empty());
        let failures: Vec<_> = results.iter().filter(|r| !r.passed).collect();
        assert!(
            failures.is_empty(),
            "Échecs endpoint auto-test : {failures:?}"
        );
    }
}
