use super::{
    BackendMsg, SHUTDOWN_REQUESTED, SMART_FAN_ACTIVE, THERMAL_RGB_ACTIVE, UI_VISIBLE, UserEvent,
    send_reply,
};
use crate::audio_ext::{AudioSpectrum, read_audio_spectrum};
use crate::fan_watchdog;
use crate::local_assets::LocalAssetCatalog;
use crate::selftest::SelfTestReport;
use crate::system_events::SysEvent;
use crate::system_ext::{
    AmbilightFrame, get_foreground_exe_name, idle_duration, list_visible_apps,
    sample_ambilight_screen,
};
use crate::tray_icon::{TrayMenuState, tray_notify, update_tray_state};
use opengear_core::cooling::{
    FanCalibration, FanChannelConfig, FanCurveTracker, FanPolicyMode, FanTempSource,
    SensorReadings, SoftwareFanMode, SoftwareFanRule, SoftwareFanTracker, fan_rule_key,
    resolve_temp_reading,
};
use opengear_core::driver::{Control, Device, DeviceKind, Setting, Value, setting_ids};
use opengear_core::fans::FanTarget;
use opengear_core::lighting::{
    DeviceLightingConfig, EffectRequest, LightingOverride, Zone, frame_len, render_frame,
};
use opengear_mainboard::HardwareSensorsSnapshot;
use opengear_platform::{
    LaptopBatteryInfo, PowerPlan, SystemTelemetry, TelemetryCollector, is_elevated,
    is_laptop_chassis, list_power_plans, read_laptop_battery,
};

use super::keyboard_view::KeyboardView;
use opengear_core::{
    AppConfig, DetectedDevice, EffectKind, Rgb, RgbEffectConfig, blend_rgb, scan_hid_devices,
};
use opengear_input::{InputEngine, reset_held_state};
use opengear_openrgb::server::{OpenRgbSdkServer, OpenRgbServerEvent, SdkController};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tao::event_loop::EventLoopProxy;
use tracing::{error, info};

#[derive(Debug, Clone)]
pub(crate) struct CurveDeviceState {
    pub is_admin: bool,
    pub available: bool,
    pub message: String,
    pub channels: Vec<FanChannelConfig>,
    pub initial: Vec<FanChannelConfig>,
    pub trackers: Vec<FanCurveTracker>,
}

#[derive(Debug, Clone)]
pub(crate) struct FanCalibrationState {
    pub device_id: String,
    pub fan_id: String,
    pub fan_name: String,
    pub steps: Vec<u8>,
    pub step_idx: usize,
    pub step_started: Instant,
    pub samples: Vec<(u8, u32)>,
}

pub(crate) struct BackendEngine {
    pub config_path: PathBuf,
    pub config: AppConfig,
    pub config_warning: Option<String>,
    pub local_assets: Arc<LocalAssetCatalog>,
    pub shared_sensors: Arc<Mutex<HardwareSensorsSnapshot>>,
    /// Registered device drivers (see `crate::drivers::device_drivers`).
    pub drivers: Vec<Box<dyn opengear_core::driver::Driver>>,
    /// Devices opened through `drivers`.
    pub driver_devices: Vec<Box<dyn Device>>,
    pub last_hid_sig: Vec<(u16, u16, u16)>,
    pub telemetry: TelemetryCollector,
    pub last_tel: SystemTelemetry,
    pub power_plans: Vec<PowerPlan>,
    pub curve_states: BTreeMap<String, CurveDeviceState>,
    pub input_engine: InputEngine,
    pub scanned_devices: Vec<DetectedDevice>,
    pub start_time: Instant,
    pub foreground_app: Option<String>,
    pub manual_profile_name: String,
    pub auto_switched: bool,
    pub last_ambilight: Option<AmbilightFrame>,
    pub audio_spectrum: AudioSpectrum,
    pub last_device_frame_at: HashMap<String, Instant>,
    pub last_device_frames: HashMap<String, Vec<Rgb>>,
    pub live_active_devices: HashSet<String>,
    pub last_selftest: Option<SelfTestReport>,
    pub undo_stack: Vec<(String, AppConfig)>,
    pub visible_apps_cache: Option<(Instant, Vec<String>)>,
    pub watchdog_spawned: bool,
    pub suspended: bool,
    pub resume_reconnect_at: Option<Instant>,
    /// Post-boot / post-resume settle window during which sleeping wireless peripherals
    /// (whose USB dongle is already present) are checked periodically without waiting for USB hotplug.
    pub wireless_wake_window_until: Option<Instant>,
    pub blackout_lock: bool,
    pub blackout_idle: bool,
    pub blackout_applied: bool,
    pub battery_alerted: HashMap<String, u8>,
    pub is_laptop: bool,
    /// Key editor geometry + active Windows layout, rebuilt every few seconds (see `keyboard_view`).
    pub keyboard_view_cache: Option<(Instant, KeyboardView)>,
    pub laptop_battery: LaptopBatteryInfo,
    /// Last keep-alive sent to each device whose `Cooling` needs one.
    pub cooling_keepalive: HashMap<String, Instant>,
    /// Overridden cooling devices last written to the crash marker.
    pub cooling_marker_ids: Vec<String>,
    pub last_ac_online: Option<bool>,
    /// `(device, setting, previous value)` changed by the battery saver, restored on AC.
    pub ac_saved_settings: Vec<(String, String, Value)>,
    /// True in headless/selftest audits: suppresses OS tray/fan-watchdog side effects and live hooks.
    pub headless: bool,
    /// Optional localhost OpenRGB SDK TCP bridge (`127.0.0.1:6742`).
    pub openrgb_server: Option<OpenRgbSdkServer>,
    pub openrgb_server_error: Option<String>,
    /// Stateful software cooling trackers keyed by `"{device_id}::{fan_id}"`.
    pub software_fan_trackers: BTreeMap<String, SoftwareFanTracker>,
    /// Active non-blocking step-by-step PWM <-> RPM fan calibration.
    pub fan_calibration: Option<FanCalibrationState>,
}

/// One change made by the battery saver: `(setting, new value, previous value)`.
pub(crate) type SaverChange = (String, Value, Value);

/// What the battery saver changes on a device, from its settings: lowest profile, 60 Hz panel,
/// keyboard backlight off. Only settings that actually change are returned.
pub(crate) fn battery_saver_changes(settings: &[Setting]) -> Vec<SaverChange> {
    let mut out = Vec::new();
    for s in settings {
        let target = match (s.id.as_str(), &s.control) {
            (setting_ids::PERFORMANCE_PROFILE, Control::Choice { options })
                if options.iter().any(|o| o.id == setting_ids::PROFILE_QUIET) =>
            {
                Value::Text(setting_ids::PROFILE_QUIET.into())
            }
            (setting_ids::DISPLAY_REFRESH_RATE, Control::Choice { options })
                if options.iter().any(|o| o.id == "60") =>
            {
                Value::Text("60".into())
            }
            (setting_ids::BACKLIGHT, Control::Toggle) => Value::Bool(false),
            _ => continue,
        };
        if s.value != target {
            out.push((s.id.clone(), target, s.value.clone()));
        }
    }
    out
}

/// Highest refresh rate offered by a `DISPLAY_REFRESH_RATE` setting (restored on AC when the
/// previous rate is unknown).
pub(crate) fn highest_refresh_rate(settings: &[Setting]) -> Option<Value> {
    settings
        .iter()
        .find(|s| s.id == setting_ids::DISPLAY_REFRESH_RATE)
        .and_then(|s| match &s.control {
            Control::Choice { options } => options
                .iter()
                .filter_map(|o| o.id.parse::<u32>().ok())
                .max()
                .map(|hz| Value::Text(hz.to_string())),
            _ => None,
        })
}

pub(crate) fn signature_from_detected(devices: &[DetectedDevice]) -> Vec<(u16, u16, u16)> {
    let mut sig: Vec<(u16, u16, u16)> = devices
        .iter()
        .map(|d| (d.vid, d.pid, d.usage_page))
        .collect();
    sig.sort_unstable();
    sig.dedup();
    sig
}

pub(crate) fn compute_hid_signature() -> Vec<(u16, u16, u16)> {
    signature_from_detected(&scan_hid_devices(false).unwrap_or_default())
}

pub(crate) fn changed_vendors(old: &[(u16, u16, u16)], new: &[(u16, u16, u16)]) -> Vec<u16> {
    let mut v: Vec<u16> = old
        .iter()
        .filter(|e| !new.contains(e))
        .chain(new.iter().filter(|e| !old.contains(e)))
        .map(|e| e.0)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

pub(crate) fn battery_alert_step(
    last: Option<u8>,
    pct: u8,
    charging: bool,
) -> (Option<u8>, Option<u8>) {
    if charging || pct > 20 {
        return (None, None);
    }
    if pct <= 5 && last != Some(5) {
        return (Some(5), Some(5));
    }
    if pct <= 15 && last.is_none() {
        return (Some(15), Some(15));
    }
    (last, None)
}

impl BackendEngine {
    pub(crate) fn new(
        local_assets: Arc<LocalAssetCatalog>,
        shared_sensors: Arc<Mutex<HardwareSensorsSnapshot>>,
    ) -> Self {
        let config_path = AppConfig::default_config_path();
        let (mut config, config_warning) = AppConfig::load_or_recover(&config_path);
        let drivers = crate::drivers::device_drivers();
        let mut telemetry = TelemetryCollector::new();
        let (scanned_devices, power_plans, mut driver_devices, chassis_laptop, last_tel) =
            std::thread::scope(|s| {
                let h_scan = s.spawn(|| scan_hid_devices(false).unwrap_or_default());
                let h_plans = s.spawn(list_power_plans);
                let exp = &config.experimental_drivers;
                let h_devs = s.spawn(|| super::devices::discover_all(&drivers, exp));
                let h_laptop = s.spawn(is_laptop_chassis);
                // Keep TelemetryCollector on the backend thread that owns it afterwards.
                let tel = telemetry.sample();
                (
                    h_scan.join().unwrap_or_default(),
                    h_plans.join().unwrap_or_default(),
                    h_devs.join().unwrap_or_default(),
                    h_laptop.join().unwrap_or(false),
                    tel,
                )
            });
        let last_hid_sig = signature_from_detected(&scanned_devices);
        migrate_legacy_profiles(&mut config, &mut driver_devices);
        seed_and_resize_device_lighting(&mut config, &mut driver_devices);

        if let Some(kb) = driver_devices
            .iter()
            .find(|d| d.info().kind == DeviceKind::Keyboard)
        {
            let settings = kb.settings();
            let pr = settings.iter().find_map(|s| {
                if s.id == setting_ids::POLLING_RATE
                    && let Value::Text(hz_str) = &s.value
                {
                    hz_str
                        .parse::<u16>()
                        .ok()
                        .and_then(opengear_core::PollingRate::from_hz)
                } else {
                    None
                }
            });
            let br = settings.iter().find_map(|s| {
                if s.id == setting_ids::HW_BRIGHTNESS
                    && let Value::Number(pct) = &s.value
                {
                    Some((*pct).clamp(0.0, 100.0) as u8)
                } else {
                    None
                }
            });
            let prof = config.active_profile_mut();
            if let Some(rate) = pr {
                prof.polling_rate = rate;
            }
            if let Some(pct) = br {
                prof.hw_brightness_pct = pct;
            }
        }

        let is_laptop = chassis_laptop
            || driver_devices
                .iter()
                .any(|d| d.info().kind == DeviceKind::Laptop);
        let laptop_battery = if is_laptop {
            read_laptop_battery()
        } else {
            LaptopBatteryInfo::default()
        };
        let last_ac_online = if laptop_battery.present {
            Some(laptop_battery.ac_online)
        } else {
            None
        };
        let curve_states = collect_curve_states(&mut driver_devices);

        let mut eng = Self {
            config_path,
            config,
            config_warning,
            local_assets,
            shared_sensors,
            drivers,
            driver_devices,
            last_hid_sig,
            telemetry,
            last_tel,
            power_plans,
            curve_states,
            input_engine: InputEngine::new(),
            scanned_devices,
            start_time: Instant::now(),
            foreground_app: None,
            manual_profile_name: String::new(),
            auto_switched: false,
            last_ambilight: None,
            audio_spectrum: AudioSpectrum::default(),
            last_device_frame_at: HashMap::new(),
            last_device_frames: HashMap::new(),
            live_active_devices: HashSet::new(),
            last_selftest: None,
            undo_stack: Vec::new(),
            visible_apps_cache: None,
            watchdog_spawned: false,
            suspended: false,
            resume_reconnect_at: None,
            wireless_wake_window_until: Some(Instant::now() + Duration::from_secs(55)),
            blackout_lock: false,
            blackout_idle: false,
            blackout_applied: false,
            battery_alerted: HashMap::new(),
            is_laptop,
            keyboard_view_cache: None,
            laptop_battery,
            cooling_keepalive: HashMap::new(),
            cooling_marker_ids: Vec::new(),
            last_ac_online,
            ac_saved_settings: Vec::new(),
            headless: false,
            openrgb_server: None,
            openrgb_server_error: None,
            software_fan_trackers: BTreeMap::new(),
            fan_calibration: None,
        };
        // A device may already be overridden (e.g. Max Fan left on): arm the crash safety net.
        eng.sync_cooling_marker();
        eng.apply_profile_fan_rules();
        eng.manual_profile_name = eng.config.active_profile.clone();
        eng.sync_input_rules();
        let p0 = eng.config.active_profile_mut().clone();
        if p0.socd.enabled
            || p0.key_chatter_ms > 0
            || p0.mouse_debounce_ms > 0
            || p0.win_lock
            || !p0.bindings.is_empty()
            || !p0.mouse_bindings.is_empty()
        {
            let _ = eng.input_engine.start();
        }
        eng.sync_openrgb_server();
        eng.sync_tray_state();
        eng
    }

    /// Construct a sandboxed headless engine backed by a disposable config path and injected devices,
    /// without starting OS hooks, tray syncs, WMI polling, or fan watchdog markers.
    pub(crate) fn new_headless(
        config_path: PathBuf,
        mut config: AppConfig,
        mut driver_devices: Vec<Box<dyn Device>>,
    ) -> Self {
        migrate_legacy_profiles(&mut config, &mut driver_devices);
        seed_and_resize_device_lighting(&mut config, &mut driver_devices);
        let curve_states = collect_curve_states(&mut driver_devices);
        let manual_profile_name = config.active_profile.clone();
        Self {
            config_path,
            config,
            config_warning: None,
            local_assets: Arc::new(LocalAssetCatalog::default()),
            shared_sensors: Arc::new(Mutex::new(HardwareSensorsSnapshot::default())),
            drivers: Vec::new(),
            driver_devices,
            last_hid_sig: Vec::new(),
            telemetry: TelemetryCollector::new(),
            last_tel: SystemTelemetry::default(),
            power_plans: Vec::new(),
            curve_states,
            input_engine: InputEngine::new(),
            scanned_devices: Vec::new(),
            start_time: Instant::now(),
            foreground_app: None,
            manual_profile_name,
            auto_switched: false,
            last_ambilight: None,
            audio_spectrum: AudioSpectrum::default(),
            last_device_frame_at: HashMap::new(),
            last_device_frames: HashMap::new(),
            live_active_devices: HashSet::new(),
            last_selftest: None,
            undo_stack: Vec::new(),
            visible_apps_cache: None,
            watchdog_spawned: false,
            suspended: false,
            resume_reconnect_at: None,
            wireless_wake_window_until: None,
            blackout_lock: false,
            blackout_idle: false,
            blackout_applied: false,
            battery_alerted: HashMap::new(),
            is_laptop: false,
            keyboard_view_cache: None,
            laptop_battery: LaptopBatteryInfo::default(),
            cooling_keepalive: HashMap::new(),
            cooling_marker_ids: Vec::new(),
            last_ac_online: None,
            ac_saved_settings: Vec::new(),
            headless: true,
            openrgb_server: None,
            openrgb_server_error: None,
            software_fan_trackers: BTreeMap::new(),
            fan_calibration: None,
        }
    }

    pub(crate) fn invalidate_rgb_frames(&mut self) {
        self.last_device_frame_at.clear();
        self.last_device_frames.clear();
    }

    pub(crate) fn invalidate_rgb_frame_for(&mut self, dev_id: &str) {
        self.last_device_frame_at.remove(dev_id);
        self.last_device_frames.remove(dev_id);
    }

    pub(crate) fn sync_tray_state(&mut self) {
        if self.headless {
            return;
        }
        let sensors = self.latest_sensors();
        let gpu_t = self
            .last_tel
            .gpu
            .edge_temp_c
            .or(self.last_tel.gpu.hotspot_temp_c);
        let bat = self
            .driver_devices
            .iter_mut()
            .filter_map(|d| d.status().battery_pct)
            .next();
        let mut parts = Vec::new();
        if let Some(c) = sensors.cpu_temp_c {
            parts.push(format!("CPU {}°C", c.round() as i32));
        }
        if let Some(g) = gpu_t {
            parts.push(format!("GPU {}°C", g.round() as i32));
        }
        if let Some(b) = bat {
            parts.push(format!("Batterie {b}%"));
        }
        let prof = self.config.active_profile_mut().clone();
        let sync_rgb = prof.enable_rgb || prof.device_lighting.values().any(|c| c.enabled);
        let state = TrayMenuState {
            active_profile: self.config.active_profile.clone(),
            profiles: self
                .config
                .profiles
                .iter()
                .map(|p| p.name.clone())
                .collect(),
            socd_enabled: prof.socd.enabled,
            sync_rgb_enabled: sync_rgb,
            status_line: parts.join(" · "),
        };
        update_tray_state(state);
    }

    pub(crate) fn persist_config(&mut self) {
        if let Err(e) = self.config.save(&self.config_path) {
            error!("Sauvegarde des profils impossible : {e:#}");
        }
    }

    pub(crate) fn sync_input_rules(&mut self) {
        let prof = self.config.active_profile_mut().clone();
        self.input_engine.update_rules(
            prof.win_lock,
            &prof.bindings,
            &prof.mouse_bindings,
            &prof.layers,
            &prof.mouse_layers,
            &prof.socd,
            prof.key_chatter_ms,
            prof.mouse_debounce_ms,
        );
    }

    pub(crate) fn latest_sensors(&self) -> HardwareSensorsSnapshot {
        self.shared_sensors
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn curve_restore_configs(&self) -> Vec<(String, FanChannelConfig)> {
        let mut out = Vec::new();
        for (dev_id, st) in &self.curve_states {
            for (idx, tracker) in st.trackers.iter().enumerate() {
                if tracker.last_duty().is_none() {
                    continue;
                }
                let Some(cur) = st.channels.get(idx) else {
                    continue;
                };
                let mut restore = st
                    .initial
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| FanChannelConfig::default_for(cur.header));
                if restore.policy == FanPolicyMode::Manual {
                    restore.policy = FanPolicyMode::Standard;
                }
                out.push((dev_id.clone(), restore));
            }
        }
        out
    }

    pub(crate) fn sync_fan_marker(&mut self) {
        if self.headless {
            return;
        }
        let wmi_restore: Vec<FanChannelConfig> = self
            .curve_restore_configs()
            .into_iter()
            .map(|(_, cfg)| cfg)
            .collect();
        let marker = fan_watchdog::FanOverrideMarker {
            pid: std::process::id(),
            wmi_restore,
            superio_restore: opengear_mainboard::saved_superio_states(),
            cooling_devices: self.overridden_cooling_ids(),
        };
        self.cooling_marker_ids = marker.cooling_devices.clone();
        fan_watchdog::write_marker(&marker);
        if !marker.is_empty() && !self.watchdog_spawned {
            self.watchdog_spawned = fan_watchdog::spawn_watchdog().is_ok();
        }
    }

    /// Ids of the devices whose fans are currently not under firmware control.
    pub(crate) fn overridden_cooling_ids(&mut self) -> Vec<String> {
        self.driver_devices
            .iter_mut()
            .filter_map(|d| {
                let id = d.info().id;
                d.cooling().filter(|c| c.overridden()).map(|_| id)
            })
            .collect()
    }

    /// Rewrite the crash marker when the set of overridden cooling devices changed.
    pub(crate) fn sync_cooling_marker(&mut self) {
        if self.overridden_cooling_ids() != self.cooling_marker_ids {
            self.sync_fan_marker();
        }
    }

    /// Refresh live fan speeds and send due keep-alives (generic over every `Cooling`).
    fn poll_cooling(&mut self, refresh: bool) {
        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            let Some(c) = dev.cooling() else {
                continue;
            };
            if refresh {
                c.refresh();
            }
            let Some(every) = c.keep_alive_interval() else {
                continue;
            };
            if !c.overridden() {
                self.cooling_keepalive.remove(&id);
                continue;
            }
            let due = self
                .cooling_keepalive
                .get(&id)
                .is_none_or(|at| at.elapsed() >= every);
            if due {
                if let Err(e) = c.keep_alive() {
                    error!("Maintien des ventilateurs de {id} impossible : {e}");
                }
                self.cooling_keepalive.insert(id, Instant::now());
            }
        }
        self.sync_cooling_marker();
    }

    pub(crate) fn sensor_readings(&self) -> SensorReadings {
        let sensors = self.latest_sensors();
        SensorReadings {
            cpu: sensors.cpu_temp_c,
            gpu_edge: self
                .last_tel
                .gpu
                .edge_temp_c
                .or(self.last_tel.gpu.hotspot_temp_c),
            gpu_hotspot: self
                .last_tel
                .gpu
                .hotspot_temp_c
                .or(self.last_tel.gpu.edge_temp_c),
            nvme: sensors.nvme_temp_c,
            motherboard: sensors.motherboard_temp_c,
        }
    }

    pub(crate) fn has_active_software_fan_curves(&self) -> bool {
        self.config
            .profiles
            .iter()
            .find(|p| p.name == self.config.active_profile)
            .is_some_and(|p| p.fan_rules.values().any(SoftwareFanRule::is_software_curve))
    }

    /// Evaluate and apply all active `SoftwareFanRule` entries (`Curve` / `Mix`) across `Cooling`
    /// devices, enforcing each fan's hardware safety floor and keeping the crash watchdog in sync.
    pub(crate) fn tick_software_fans(&mut self, dt_secs: f32) {
        let rules = self.config.active_profile_mut().fan_rules.clone();
        if rules.is_empty() {
            return;
        }
        let readings = self.sensor_readings();
        let cal_dev = self
            .fan_calibration
            .as_ref()
            .map(|c| (c.device_id.clone(), c.fan_id.clone()));

        let mut plan: Vec<(String, String, u8)> = Vec::new();
        for dev in &mut self.driver_devices {
            let dev_id = dev.info().id;
            let Some(c) = dev.cooling() else {
                continue;
            };
            for fan in c.fans() {
                if cal_dev
                    .as_ref()
                    .is_some_and(|(cd, cf)| cd == &dev_id && cf == &fan.id)
                {
                    continue;
                }
                let key = fan_rule_key(&dev_id, &fan.id);
                let Some(rule) = rules.get(&key) else {
                    continue;
                };
                if !rule.is_software_curve() {
                    continue;
                }
                let tracker = self.software_fan_trackers.entry(key).or_default();
                if let Some(target_duty) =
                    tracker.evaluate(rule, fan.min_pct, &readings, dt_secs, 2)
                {
                    plan.push((dev_id.clone(), fan.id, target_duty));
                }
            }
        }

        for (dev_id, fan_id, duty) in plan {
            let key = fan_rule_key(&dev_id, &fan_id);
            if self
                .set_device_fan(&dev_id, Some(&fan_id), FanTarget::Fixed(duty))
                .is_ok()
                && let Some(tr) = self.software_fan_trackers.get_mut(&key)
            {
                tr.mark_applied(duty);
            }
        }
    }

    /// Apply the active profile's `fan_rules` (e.g. after switching profiles or startup).
    pub(crate) fn apply_profile_fan_rules(&mut self) {
        self.software_fan_trackers.clear();
        let rules = self.config.active_profile_mut().fan_rules.clone();
        let readings = self.sensor_readings();
        let mut actions: Vec<(String, String, FanTarget, bool)> = Vec::new();

        for dev in &mut self.driver_devices {
            let dev_id = dev.info().id;
            let Some(c) = dev.cooling() else {
                continue;
            };
            for fan in c.fans() {
                let key = fan_rule_key(&dev_id, &fan.id);
                match rules.get(&key) {
                    Some(rule) if rule.mode == SoftwareFanMode::Fixed => {
                        actions.push((
                            dev_id.clone(),
                            fan.id,
                            FanTarget::Fixed(rule.fixed_pct),
                            false,
                        ));
                    }
                    Some(rule) if rule.is_software_curve() => {
                        let mut tr = SoftwareFanTracker::new();
                        if let Some(duty) = tr.evaluate(rule, fan.min_pct, &readings, 0.0, 1) {
                            actions.push((dev_id.clone(), fan.id, FanTarget::Fixed(duty), true));
                        }
                        self.software_fan_trackers.insert(key, tr);
                    }
                    _ => {
                        if fan.manual_pct.is_some() {
                            actions.push((dev_id.clone(), fan.id, FanTarget::Auto, false));
                        }
                    }
                }
            }
        }

        for (dev_id, fan_id, target, mark_tr) in actions {
            if self.set_device_fan(&dev_id, Some(&fan_id), target).is_ok()
                && mark_tr
                && let FanTarget::Fixed(d) = target
                && let Some(tr) = self
                    .software_fan_trackers
                    .get_mut(&fan_rule_key(&dev_id, &fan_id))
            {
                tr.mark_applied(d);
            }
        }
    }

    /// Begin a non-blocking step-by-step PWM <-> RPM calibration sweep on `(device_id, fan_id)`.
    pub(crate) fn start_fan_calibration(
        &mut self,
        device_id: &str,
        fan_id: &str,
    ) -> Result<(), String> {
        if self.fan_calibration.is_some() {
            return Err("Une calibration de ventilateur est déjà en cours".into());
        }
        let (fan_name, min_pct) = {
            let dev = self
                .device_mut(device_id)
                .ok_or_else(|| format!("Appareil introuvable : {device_id}"))?;
            let c = dev
                .cooling()
                .ok_or_else(|| format!("Ventilateurs non pilotables sur {device_id}"))?;
            let fan = c
                .fans()
                .into_iter()
                .find(|f| f.id == fan_id)
                .ok_or_else(|| format!("Ventilateur introuvable : {fan_id}"))?;
            (fan.name, fan.min_pct.min(100))
        };
        let mut steps: Vec<u8> = [100u8, 80, 65, 50, 40, 30, 20, 10, 0]
            .into_iter()
            .filter(|&p| p >= min_pct)
            .collect();
        if !steps.contains(&min_pct) {
            steps.push(min_pct);
        }
        let first = steps[0];
        self.set_device_fan(device_id, Some(fan_id), FanTarget::Fixed(first))?;
        self.fan_calibration = Some(FanCalibrationState {
            device_id: device_id.to_string(),
            fan_id: fan_id.to_string(),
            fan_name,
            steps,
            step_idx: 0,
            step_started: Instant::now(),
            samples: Vec::new(),
        });
        Ok(())
    }

    pub(crate) fn cancel_fan_calibration(&mut self) {
        let Some(cal) = self.fan_calibration.take() else {
            return;
        };
        let _ = self.set_device_fan(&cal.device_id, Some(&cal.fan_id), FanTarget::Auto);
        self.apply_profile_fan_rules();
    }

    fn tick_fan_calibration(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        let Some(ref cal) = self.fan_calibration else {
            return;
        };
        let settle = if cal.step_idx == 0 {
            Duration::from_millis(2500)
        } else {
            Duration::from_millis(2000)
        };
        if cal.step_started.elapsed() < settle {
            return;
        }
        let (dev_id, fan_id, duty) = (
            cal.device_id.clone(),
            cal.fan_id.clone(),
            cal.steps[cal.step_idx],
        );
        let measured_rpm = self
            .device_mut(&dev_id)
            .and_then(|d| d.cooling())
            .and_then(|c| {
                c.refresh();
                c.fans()
                    .into_iter()
                    .find(|f| f.id == fan_id)
                    .and_then(|f| f.rpm)
            })
            .unwrap_or(0);

        let mut next_step: Option<u8> = None;
        let mut finished_samples: Option<Vec<(u8, u32)>> = None;
        if let Some(ref mut cal) = self.fan_calibration {
            cal.samples.push((duty, measured_rpm));
            cal.step_idx += 1;
            if cal.step_idx < cal.steps.len() {
                next_step = Some(cal.steps[cal.step_idx]);
                cal.step_started = Instant::now();
            } else {
                finished_samples = Some(cal.samples.clone());
            }
        }

        if let Some(next_duty) = next_step {
            let _ = self.set_device_fan(&dev_id, Some(&fan_id), FanTarget::Fixed(next_duty));
            if UI_VISIBLE.load(Ordering::Relaxed) {
                let st = self.full_state_json();
                let _ = proxy.send_event(UserEvent::EvalJs(format!(
                    "window.__ogHotplug && window.__ogHotplug({st});"
                )));
            }
        } else if let Some(samples) = finished_samples {
            self.fan_calibration = None;
            let _ = self.set_device_fan(&dev_id, Some(&fan_id), FanTarget::Auto);
            let result = FanCalibration::from_samples(samples);
            self.config
                .fan_calibrations
                .insert(fan_rule_key(&dev_id, &fan_id), result);
            self.apply_profile_fan_rules();
            self.persist_config();
            if UI_VISIBLE.load(Ordering::Relaxed) {
                let st = self.full_state_json();
                let _ = proxy.send_event(UserEvent::EvalJs(format!(
                    "window.__ogHotplug && window.__ogHotplug({st});"
                )));
            }
        }
    }

    pub(crate) fn restore_hardware_on_exit(&mut self) {
        self.openrgb_server = None;
        self.input_engine.stop();
        reset_held_state();

        let lighting_cfgs = self.config.active_profile_mut().device_lighting.clone();
        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            if let Some(lighting) = dev.lighting() {
                release_device_lighting(lighting, lighting_cfgs.get(&id));
            }
        }
        let mut remaining_cooling = Vec::new();
        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            if let Some(c) = dev.cooling()
                && c.overridden()
                && let Err(e) = c.restore_firmware_control()
            {
                error!("Ventilateurs de {id} non rendus au firmware à la fermeture : {e}");
                remaining_cooling.push(id);
            }
        }

        let to_restore = self.curve_restore_configs();
        let mut remaining_wmi = Vec::new();
        for (dev_id, cfg) in to_restore {
            let applied = self
                .curve_cooling_for_mut(&dev_id)
                .map(|fc| fc.apply_channel_curve(&cfg))
                .unwrap_or_else(|| Err(anyhow::anyhow!("Contrôleur {dev_id} absent")));
            if let Err(e) = applied {
                error!(
                    "Restauration ventilateur {:?} ({dev_id}) impossible à la fermeture : {e}",
                    cfg.header
                );
                remaining_wmi.push(cfg);
            }
        }
        for st in self.curve_states.values_mut() {
            for tracker in &mut st.trackers {
                tracker.reset();
            }
        }
        let remaining_superio = opengear_mainboard::saved_superio_states();
        if remaining_wmi.is_empty() && remaining_superio.is_empty() && remaining_cooling.is_empty()
        {
            fan_watchdog::clear_marker();
        } else {
            fan_watchdog::write_marker(&fan_watchdog::FanOverrideMarker {
                pid: std::process::id(),
                wmi_restore: remaining_wmi,
                superio_restore: remaining_superio,
                cooling_devices: remaining_cooling,
            });
        }
    }

    pub(crate) fn run_loop(
        &mut self,
        rx: mpsc::Receiver<BackendMsg>,
        proxy: EventLoopProxy<UserEvent>,
        backend_tx: mpsc::Sender<BackendMsg>,
    ) {
        let mut last_rgb_push = Instant::now();
        let mut last_tel_push = Instant::now();
        let mut last_hotplug_check = Instant::now();
        let mut last_fg_check = Instant::now();
        let mut last_smart_fan = Instant::now();
        let mut last_logi_poll = Instant::now();
        let mut last_idle_check = Instant::now();
        let mut last_laptop_poll = Instant::now();
        let mut last_cooling_poll = Instant::now();
        let mut last_cooling_refresh = Instant::now();
        let mut last_dormant_check = Instant::now();

        loop {
            if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                self.restore_hardware_on_exit();
                break;
            }

            let ui_open = UI_VISIBLE.load(Ordering::Relaxed);
            let (live_rgb_active, animated_rgb_active, thermal_rgb_active) =
                self.lighting_activity_flags();
            let pending_static_rgb = live_rgb_active && self.has_pending_live_frame();
            let smart_fan_active = self.has_active_software_fan_curves()
                || self.curve_states.values().any(|st| {
                    st.is_admin
                        && st.available
                        && st
                            .channels
                            .iter()
                            .any(|c| c.source != FanTempSource::CpuBios)
                });
            THERMAL_RGB_ACTIVE.store(thermal_rgb_active, Ordering::Relaxed);
            SMART_FAN_ACTIVE.store(smart_fan_active, Ordering::Relaxed);

            let openrgb_clients_active = self
                .openrgb_server
                .as_ref()
                .is_some_and(|s| s.client_count() > 0);
            let wait_ms = if ((live_rgb_active && (animated_rgb_active || pending_static_rgb))
                || openrgb_clients_active)
                && !self.blackout_applied
                && !self.suspended
            {
                12
            } else if ui_open || smart_fan_active || self.fan_calibration.is_some() {
                150
            } else {
                500
            };

            match rx.recv_timeout(Duration::from_millis(wait_ms)) {
                Ok(msg) => {
                    if self.dispatch_msg(msg, &proxy, &backend_tx) {
                        return;
                    }
                    while let Ok(msg) = rx.try_recv() {
                        if self.dispatch_msg(msg, &proxy, &backend_tx) {
                            return;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.restore_hardware_on_exit();
                    break;
                }
            }

            // Deferred resume reconnect (non-blocking 1.2s USB settle after wake)
            if let Some(at) = self.resume_reconnect_at
                && Instant::now() >= at
            {
                self.resume_reconnect_at = None;
                info!("Reconnexion matérielle post-veille");
                self.reconnect_all();
                self.reapply_runtime_state();
                if UI_VISIBLE.load(Ordering::Relaxed) {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
            }

            // 0. Foreground window check (auto-profile switching & anti-SOCD suspension)
            let need_fg =
                self.config.auto_profile_switch || self.config.active_profile_mut().socd.enabled;
            if need_fg && last_fg_check.elapsed() >= Duration::from_millis(750) {
                let fg = get_foreground_exe_name();
                if fg != self.foreground_app {
                    self.foreground_app = fg.clone();
                    let mut state_changed = false;

                    if self.config.auto_profile_switch
                        && let Some(ref exe) = fg
                        && exe != "opengear.exe"
                    {
                        let matched = self
                            .config
                            .profiles
                            .iter()
                            .find(|pr| pr.linked_apps.iter().any(|a| a.eq_ignore_ascii_case(exe)))
                            .map(|pr| pr.name.clone());

                        if let Some(target_prof) = matched {
                            if self.config.active_profile != target_prof {
                                if !self.auto_switched {
                                    self.manual_profile_name = self.config.active_profile.clone();
                                }
                                self.config.active_profile = target_prof;
                                self.auto_switched = true;
                                self.sync_input_rules();
                                self.apply_profile_fan_rules();
                                state_changed = true;
                            }
                        } else if self.auto_switched
                            && self
                                .config
                                .profiles
                                .iter()
                                .any(|pr| pr.name == self.manual_profile_name)
                        {
                            self.config.active_profile = self.manual_profile_name.clone();
                            self.auto_switched = false;
                            self.sync_input_rules();
                            self.apply_profile_fan_rules();
                            state_changed = true;
                        }
                    }

                    let blocked = {
                        let prof = self.config.active_profile_mut();
                        fg.as_ref()
                            .map(|exe| {
                                prof.socd
                                    .blocked_apps
                                    .iter()
                                    .any(|b| b.eq_ignore_ascii_case(exe))
                            })
                            .unwrap_or(false)
                    };
                    if blocked != self.input_engine.is_socd_suspended() {
                        self.input_engine.set_socd_suspended(blocked);
                        state_changed = true;
                    }

                    if state_changed {
                        self.sync_tray_state();
                        let st = self.full_state_json();
                        let _ = proxy.send_event(UserEvent::EvalJs(format!(
                            "window.__ogHotplug && window.__ogHotplug({st});"
                        )));
                    }
                }
                last_fg_check = Instant::now();
            }

            // 1. Live telemetry push (also keeps tray status tooltip fresh even when hidden)
            let tel_interval = if ui_open {
                Duration::from_secs(1)
            } else {
                Duration::from_millis(2500)
            };
            if (ui_open || thermal_rgb_active || smart_fan_active)
                && last_tel_push.elapsed() >= tel_interval
            {
                self.last_tel = self.telemetry.sample();
                self.sync_tray_state();
                if ui_open {
                    let tel_json = self.telemetry_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogTelemetry && window.__ogTelemetry({tel_json});"
                    )));
                }
                last_tel_push = Instant::now();
            }

            // 1b. Smart Cooling (BIOS CurveCooling + universal software Cooling curves/mix)
            if smart_fan_active && last_smart_fan.elapsed() >= Duration::from_secs(2) {
                let dt_secs = last_smart_fan.elapsed().as_secs_f32();
                let mut marker_dirty = false;
                let readings = self.sensor_readings();
                let mut pending_applies: Vec<(String, usize, u8, FanChannelConfig)> = Vec::new();
                for (dev_id, st) in &mut self.curve_states {
                    if !st.is_admin || !st.available {
                        continue;
                    }
                    for (idx, ch) in st.channels.iter().enumerate() {
                        if ch.source == FanTempSource::CpuBios {
                            continue;
                        }
                        let Some(tracker) = st.trackers.get_mut(idx) else {
                            continue;
                        };
                        let measured =
                            resolve_temp_reading(ch.source, ch.custom_mix.as_ref(), &readings);
                        if let Some(target_duty) = tracker.evaluate(ch, measured, dt_secs, 3) {
                            let mut flat = ch.clone();
                            flat.policy = FanPolicyMode::Manual;
                            for pt in &mut flat.curve {
                                pt.duty_pct = target_duty;
                            }
                            pending_applies.push((dev_id.clone(), idx, target_duty, flat));
                        }
                    }
                }
                for (dev_id, idx, target_duty, flat) in pending_applies {
                    let ok = self
                        .curve_cooling_for_mut(&dev_id)
                        .is_some_and(|fc| fc.apply_channel_curve(&flat).is_ok());
                    if ok
                        && let Some(st) = self.curve_states.get_mut(&dev_id)
                        && let Some(tracker) = st.trackers.get_mut(idx)
                    {
                        if tracker.last_duty().is_none() {
                            marker_dirty = true;
                        }
                        tracker.mark_applied(target_duty);
                    }
                }
                if marker_dirty {
                    self.sync_fan_marker();
                }
                self.tick_software_fans(dt_secs);
                last_smart_fan = Instant::now();
            }

            if !self.suspended && self.fan_calibration.is_some() {
                self.tick_fan_calibration(&proxy);
            }

            // 2. Fallback hotplug poll only when UI is open (WM_DEVICECHANGE handles tray mode)
            if ui_open && !self.suspended && last_hotplug_check.elapsed() >= Duration::from_secs(30)
            {
                if self.check_hotplug() {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
                last_hotplug_check = Instant::now();
            }

            // 2b. Re-probe dormant wireless dongles (e.g. a mouse that was still asleep at boot
            // or system resume) without touching already-connected devices or polling idle trays
            // once the post-boot/wake window expires.
            let in_wake_window = self
                .wireless_wake_window_until
                .is_some_and(|until| Instant::now() < until);
            if !in_wake_window {
                self.wireless_wake_window_until = None;
            }
            let dormant_interval = if ui_open || in_wake_window {
                Duration::from_secs(5)
            } else {
                Duration::ZERO
            };
            if !self.suspended
                && !dormant_interval.is_zero()
                && last_dormant_check.elapsed() >= dormant_interval
            {
                if self.check_dormant_wireless() && ui_open {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
                last_dormant_check = Instant::now();
            }

            // 3. Battery refresh + laptop polls
            let status_interval = if ui_open {
                Duration::from_secs(30)
            } else {
                Duration::from_secs(300)
            };
            if !self.suspended && last_logi_poll.elapsed() >= status_interval {
                for dev in &mut self.driver_devices {
                    dev.refresh_status();
                }
                self.check_battery_alerts();
                last_logi_poll = Instant::now();
            }

            let laptop_interval = if ui_open {
                Duration::from_secs(3)
            } else {
                Duration::from_secs(10)
            };
            if !self.suspended
                && (self.is_laptop || self.laptop_battery.present)
                && (ui_open || self.config.laptop_auto_battery_saver)
                && last_laptop_poll.elapsed() >= laptop_interval
            {
                self.laptop_battery = read_laptop_battery();
                if ui_open {
                    // The built-in panel can change behind our back (Fn keys, Windows settings).
                    for dev in &mut self.driver_devices {
                        if dev.info().kind == DeviceKind::Display {
                            dev.refresh_status();
                        }
                    }
                }
                self.handle_laptop_ac_transition(&proxy);
                last_laptop_poll = Instant::now();
            }

            // 3a. Device fans: live speeds while the UI is open, keep-alives whenever a device
            // needs them (e.g. an EC that reverts a manual speed after ~30-60 s).
            if !self.suspended && last_cooling_poll.elapsed() >= Duration::from_secs(1) {
                let refresh = ui_open && last_cooling_refresh.elapsed() >= Duration::from_secs(3);
                self.poll_cooling(refresh);
                if refresh {
                    last_cooling_refresh = Instant::now();
                }
                last_cooling_poll = Instant::now();
            }

            // 3b. Inactivity lights-off
            if last_idle_check.elapsed() >= Duration::from_secs(5) {
                let min = self.config.idle_lights_off_min;
                self.blackout_idle = min > 0
                    && idle_duration()
                        .map(|d| d >= Duration::from_secs(min as u64 * 60))
                        .unwrap_or(false);
                last_idle_check = Instant::now();
            }

            // 3c. Lights-off hold / restore
            if !self.suspended {
                let blackout = self.blackout_lock || self.blackout_idle;
                if blackout {
                    if !self.blackout_applied || last_rgb_push.elapsed() >= Duration::from_secs(1) {
                        self.push_blackout_frame();
                        last_rgb_push = Instant::now();
                    }
                } else if self.blackout_applied {
                    self.end_blackout();
                }
            }

            // 4. External OpenRGB SDK bridge + Live RGB streaming
            self.poll_openrgb_server();
            let rgb_interval = if live_rgb_active && pending_static_rgb {
                Duration::ZERO
            } else if live_rgb_active && animated_rgb_active {
                Duration::from_millis(35)
            } else {
                Duration::from_millis(1000)
            };
            if !self.suspended
                && !self.blackout_applied
                && (live_rgb_active || thermal_rgb_active)
                && last_rgb_push.elapsed() >= rgb_interval
            {
                self.push_live_rgb_frame();
                last_rgb_push = Instant::now();
            }
        }
    }

    fn dispatch_msg(
        &mut self,
        msg: BackendMsg,
        proxy: &EventLoopProxy<UserEvent>,
        backend_tx: &mpsc::Sender<BackendMsg>,
    ) -> bool {
        match msg {
            BackendMsg::Shutdown(ack) => {
                self.restore_hardware_on_exit();
                let _ = ack.send(());
                return true;
            }
            BackendMsg::SysEvent(ev) => {
                self.handle_sys_event(ev, proxy);
            }
            BackendMsg::TrayToggleSocd => {
                self.tray_toggle_socd();
                self.sync_tray_state();
                if UI_VISIBLE.load(Ordering::Relaxed) {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
            }
            BackendMsg::TrayToggleSync => {
                self.tray_toggle_sync();
                self.sync_tray_state();
                if UI_VISIBLE.load(Ordering::Relaxed) {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
            }
            BackendMsg::TraySwitchProfile(name) => {
                self.tray_switch_profile(&name);
                self.sync_tray_state();
                if UI_VISIBLE.load(Ordering::Relaxed) {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
            }
            BackendMsg::TrayApplyPreset(preset) => {
                self.tray_apply_preset(&preset);
                self.sync_tray_state();
                if UI_VISIBLE.load(Ordering::Relaxed) {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
            }
            BackendMsg::AsyncWorkerDone {
                id,
                action_msg,
                selftest,
                refresh_hw,
                error,
            } => {
                if refresh_hw {
                    self.driver_devices = super::devices::discover_all(
                        &self.drivers,
                        &self.config.experimental_drivers,
                    );
                    seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
                    self.refresh_curve_states();
                }
                if let Some(rep) = selftest {
                    self.last_selftest = Some(rep);
                }
                if let Some(err) = error {
                    send_reply(proxy, id, Err(err));
                } else {
                    let mut st = self.full_state_json();
                    if let Some(m) = action_msg {
                        st["last_action_msg"] = json!(m);
                    }
                    send_reply(proxy, id, Ok(st));
                }
            }
            BackendMsg::Rpc(req) => {
                if req.cmd == "pawnio_install" || req.cmd == "run_selftest" {
                    self.spawn_async_job(req.id, &req.cmd, backend_tx.clone());
                } else {
                    let res = self.handle_cmd(&req.cmd, &req.payload, Some(proxy));
                    send_reply(proxy, req.id, res);
                }
            }
        }
        false
    }

    fn handle_sys_event(&mut self, ev: SysEvent, proxy: &EventLoopProxy<UserEvent>) {
        match ev {
            SysEvent::Suspend => {
                info!("Mise en veille détectée — suspension des échanges HID");
                self.suspended = true;
                self.resume_reconnect_at = None;
                reset_held_state();
            }
            SysEvent::Resume => {
                info!("Sortie de veille — reconnexion planifiée");
                self.suspended = false;
                for st in self.curve_states.values_mut() {
                    for tr in &mut st.trackers {
                        tr.reset();
                    }
                }
                self.resume_reconnect_at = Some(Instant::now() + Duration::from_millis(2000));
                self.wireless_wake_window_until = Some(Instant::now() + Duration::from_secs(55));
            }
            SysEvent::Lock => {
                if self.config.lights_off_on_lock {
                    self.blackout_lock = true;
                    self.push_blackout_frame();
                }
            }
            SysEvent::Unlock => {
                self.blackout_lock = false;
                reset_held_state();
            }
            SysEvent::DevicesChanged => {
                if !self.suspended && self.check_hotplug() && UI_VISIBLE.load(Ordering::Relaxed) {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
            }
        }
    }

    /// Key editor view, rebuilt when older than 2 s so a Windows layout switch (Alt+Maj), a
    /// keyboard plugged in or a setting change shows up without an explicit refresh.
    pub(crate) fn keyboard_view(&mut self) -> &KeyboardView {
        let stale = self
            .keyboard_view_cache
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(2));
        if stale {
            let view = KeyboardView::build(&self.config, &self.driver_devices, self.is_laptop);
            self.keyboard_view_cache = Some((Instant::now(), view));
        }
        &self
            .keyboard_view_cache
            .as_ref()
            .expect("cache filled above")
            .1
    }

    /// Forget the cached view after a change that affects it.
    pub(crate) fn invalidate_keyboard_view(&mut self) {
        self.keyboard_view_cache = None;
    }

    pub(crate) fn lighting_activity_flags(&mut self) -> (bool, bool, bool) {
        let prof = self.config.active_profile_mut().clone();
        let mut any_live = false;
        let mut any_animated = false;
        let mut any_thermal = false;
        for d in &mut self.driver_devices {
            let id = d.info().id;
            if d.lighting().is_none() {
                continue;
            }
            let Some(cfg) = prof.device_lighting.get(&id) else {
                continue;
            };
            if !cfg.enabled {
                continue;
            }
            let mut consider = |ov: &LightingOverride| {
                let is_global = matches!(ov, LightingOverride::Global);
                let kind = match ov {
                    LightingOverride::Global => prof.rgb.kind,
                    LightingOverride::Software { effect } => effect.kind,
                    LightingOverride::Hardware { .. } | LightingOverride::Off => return,
                };
                any_live = true;
                let check_kind = |k: EffectKind, animated: &mut bool, thermal: &mut bool| match k {
                    EffectKind::Static
                    | EffectKind::PerKey
                    | EffectKind::Gradient
                    | EffectKind::Off => {}
                    EffectKind::Thermal => *thermal = true,
                    _ => *animated = true,
                };
                check_kind(kind, &mut any_animated, &mut any_thermal);
                if is_global {
                    for layer in prof
                        .rgb_layers
                        .iter()
                        .filter(|l| l.enabled && l.opacity > 0.0)
                    {
                        check_kind(layer.effect.kind, &mut any_animated, &mut any_thermal);
                    }
                }
            };
            let base = cfg.mode.as_ref().unwrap_or(&LightingOverride::Global);
            consider(base);
            for ov in cfg.zones.values() {
                consider(ov);
            }
        }
        (any_live, any_animated, any_thermal)
    }

    /// True if at least one enabled software-lit device has not received a frame since the last
    /// lighting configuration change.
    pub(crate) fn has_pending_live_frame(&mut self) -> bool {
        let prof = self.config.active_profile_mut().clone();
        self.driver_devices.iter_mut().any(|d| {
            let id = d.info().id;
            if d.lighting().is_none() {
                return false;
            }
            let Some(cfg) = prof.device_lighting.get(&id) else {
                return false;
            };
            let base = cfg.mode.as_ref().unwrap_or(&LightingOverride::Global);
            let has_sw = cfg.enabled
                && (!matches!(base, LightingOverride::Hardware { .. })
                    || cfg
                        .zones
                        .values()
                        .any(|z| !matches!(z, LightingOverride::Hardware { .. })));
            has_sw && !self.last_device_frames.contains_key(&id)
        })
    }

    fn compute_thermal_rgb(&self) -> Rgb {
        let sensors = self.latest_sensors();
        let temp = sensors
            .cpu_temp_c
            .or(self.last_tel.gpu.edge_temp_c)
            .or(self.last_tel.gpu.hotspot_temp_c)
            .unwrap_or(35.0 + self.last_tel.cpu_usage_pct * 0.5);
        thermal_rgb(temp)
    }

    pub(crate) fn push_live_rgb_frame(&mut self) {
        let t = self.start_time.elapsed().as_secs_f32();
        let prof = self.config.active_profile_mut().clone();
        let thermal_col = self.compute_thermal_rgb();

        let mut need_ambilight = false;
        let mut need_audio = false;
        let mut need_reactive = false;
        for d in &mut self.driver_devices {
            let id = d.info().id;
            if d.lighting().is_none() {
                continue;
            }
            let Some(cfg) = prof.device_lighting.get(&id) else {
                continue;
            };
            if !cfg.enabled {
                continue;
            }
            for ov in std::iter::once(cfg.mode.as_ref().unwrap_or(&LightingOverride::Global))
                .chain(cfg.zones.values())
            {
                let is_global = matches!(ov, LightingOverride::Global);
                let kind = match ov {
                    LightingOverride::Global => prof.rgb.kind,
                    LightingOverride::Software { effect } => effect.kind,
                    LightingOverride::Hardware { .. } | LightingOverride::Off => continue,
                };
                match kind {
                    EffectKind::Ambilight => need_ambilight = true,
                    EffectKind::AudioVisualizer => need_audio = true,
                    EffectKind::ReactiveTyping => need_reactive = true,
                    _ => {}
                }
                if is_global {
                    for layer in prof
                        .rgb_layers
                        .iter()
                        .filter(|l| l.enabled && l.opacity > 0.0)
                    {
                        match layer.effect.kind {
                            EffectKind::Ambilight => need_ambilight = true,
                            EffectKind::AudioVisualizer => need_audio = true,
                            EffectKind::ReactiveTyping => need_reactive = true,
                            _ => {}
                        }
                    }
                }
            }
        }

        if need_ambilight && let Some(frame) = sample_ambilight_screen(self.last_ambilight.as_ref())
        {
            self.last_ambilight = Some(frame);
        }

        if need_audio {
            self.audio_spectrum = read_audio_spectrum();
        }

        let reactive_centers: Vec<(f32, f32, f32)> = if need_reactive {
            if !self.headless && !self.input_engine.is_running() {
                let _ = self.input_engine.start();
            }
            let ripples = self.input_engine.recent_key_ripples(1.1);
            let view = self.keyboard_view();
            ripples
                .into_iter()
                .filter_map(|(kname, age)| view.norm_center(&kname).map(|(x, y)| (x, y, age)))
                .collect()
        } else {
            Vec::new()
        };

        let amb = self.last_ambilight.clone();
        let audio = self.audio_spectrum;
        let placements = self.config.device_placements.clone();

        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            let Some(lighting) = dev.lighting() else {
                continue;
            };
            let cfg = prof.device_lighting.get(&id).cloned().unwrap_or_default();
            let base = cfg.mode.as_ref().unwrap_or(&LightingOverride::Global);
            let has_software_zone = cfg.enabled
                && (!matches!(base, LightingOverride::Hardware { .. })
                    || cfg
                        .zones
                        .values()
                        .any(|z| !matches!(z, LightingOverride::Hardware { .. })));

            if !has_software_zone {
                if self.live_active_devices.remove(&id) {
                    release_device_lighting(lighting, prof.device_lighting.get(&id));
                }
                self.last_device_frames.remove(&id);
                continue;
            }

            let min_dt = Duration::from_secs_f32(1.0 / lighting.max_fps().max(1) as f32);
            if self
                .last_device_frame_at
                .get(&id)
                .is_some_and(|at| at.elapsed() < min_dt)
            {
                continue;
            }

            let zones = lighting.zones();
            let placement = placements.get(&id).copied().unwrap_or_default();
            let mut frame = Vec::with_capacity(frame_len(&zones));

            let frame_ctx = LiveFrameCtx {
                t,
                amb: amb.as_ref(),
                reactive_centers: &reactive_centers,
                audio,
                thermal_col,
            };

            for zone in &zones {
                let ov = cfg.zones.get(&zone.id).unwrap_or(base);
                match ov {
                    LightingOverride::Off => {
                        frame.extend(std::iter::repeat_n(Rgb::BLACK, zone.leds.len()));
                    }
                    LightingOverride::Hardware { request } => {
                        // Preserve hardware color on mixed devices rather than sending Rgb::BLACK.
                        let fill = if request.effect.eq_ignore_ascii_case("Off")
                            || request.effect.ends_with(":Off")
                        {
                            Rgb::BLACK
                        } else {
                            request.colors.first().copied().unwrap_or(Rgb::CYAN)
                        };
                        frame.extend(std::iter::repeat_n(fill, zone.leds.len()));
                    }
                    LightingOverride::Global => {
                        let mut colors =
                            render_effect_for_zone(&prof.rgb, zone, &placement, &frame_ctx);
                        for layer in prof
                            .rgb_layers
                            .iter()
                            .filter(|l| l.enabled && l.opacity > 0.0)
                        {
                            let over =
                                render_effect_for_zone(&layer.effect, zone, &placement, &frame_ctx);
                            for (i, led) in zone.leds.iter().enumerate() {
                                let matches_mask = layer.key_filter.is_empty()
                                    || led
                                        .key_code
                                        .as_ref()
                                        .is_some_and(|kc| layer.key_filter.iter().any(|m| m == kc));
                                if matches_mask {
                                    colors[i] =
                                        blend_rgb(colors[i], over[i], layer.blend, layer.opacity);
                                }
                            }
                        }
                        frame.extend(colors);
                    }
                    LightingOverride::Software { effect } => {
                        frame.extend(render_effect_for_zone(effect, zone, &placement, &frame_ctx));
                    }
                }
            }

            let first_activation = self.live_active_devices.insert(id.clone());
            if !self
                .last_device_frames
                .get(&id)
                .is_some_and(|prev| prev == &frame)
            {
                let _ = lighting.set_frame(&frame);
                // Keep static frames un-cached on the very first activation so one confirmation
                // frame is resent 35 ms later (covers MCUs like ASUS AULA3 transitioning to Direct).
                if !first_activation {
                    self.last_device_frames.insert(id.clone(), frame);
                }
            }
            self.last_device_frame_at.insert(id, Instant::now());
        }
    }

    pub(crate) fn check_hotplug(&mut self) -> bool {
        let sig = compute_hid_signature();
        if sig == self.last_hid_sig {
            return false;
        }
        let vendors = changed_vendors(&self.last_hid_sig, &sig);
        info!("Changement USB/HID détecté ({vendors:04X?}) — reconnexion ciblée…");
        self.last_hid_sig = sig;
        self.reconnect_vendors(&vendors);
        self.reapply_runtime_state();
        true
    }

    pub(crate) fn check_dormant_wireless(&mut self) -> bool {
        let added = super::devices::discover_dormant_wireless(
            &mut self.driver_devices,
            &self.drivers,
            &self.config.experimental_drivers,
        );
        if added == 0 {
            return false;
        }
        info!(
            added,
            "Périphérique sans fil réveillé détecté automatiquement"
        );
        seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
        self.invalidate_keyboard_view();
        self.refresh_curve_states();
        if !self.headless {
            self.sync_openrgb_server();
            self.sync_tray_state();
        }
        true
    }

    pub(crate) fn reconnect_vendors(&mut self, vendors: &[u16]) {
        let enabled = &self.config.experimental_drivers;
        let touched_prefixes: Vec<String> = self
            .drivers
            .iter()
            .filter(|d| {
                (!d.experimental() || enabled.contains(d.name()))
                    && d.usb_vendor_ids().iter().any(|v| vendors.contains(v))
            })
            .map(|d| format!("{}:", d.name()))
            .collect();
        self.rediscover_drivers_for(vendors);
        seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
        for prefix in &touched_prefixes {
            self.last_device_frame_at
                .retain(|id, _| !id.starts_with(prefix));
            self.last_device_frames
                .retain(|id, _| !id.starts_with(prefix));
            self.live_active_devices
                .retain(|id| !id.starts_with(prefix));
        }
        self.invalidate_keyboard_view();
        self.scanned_devices = scan_hid_devices(false).unwrap_or_default();
        self.sync_tray_state();
    }

    pub(crate) fn reconnect_all(&mut self) {
        self.driver_devices =
            super::devices::discover_all(&self.drivers, &self.config.experimental_drivers);
        seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
        self.invalidate_rgb_frames();
        self.live_active_devices.clear();
        self.invalidate_keyboard_view();

        self.cooling_keepalive.clear();
        self.refresh_curve_states();
        if !self.headless {
            self.sync_cooling_marker();
            self.power_plans = list_power_plans();
            self.scanned_devices = scan_hid_devices(false).unwrap_or_default();
            self.last_hid_sig = compute_hid_signature();
            self.sync_openrgb_server();
            self.sync_tray_state();
        }
    }

    pub(crate) fn sync_openrgb_server(&mut self) {
        if self.headless || !self.config.openrgb_server_enabled {
            self.openrgb_server = None;
            self.openrgb_server_error = None;
            return;
        }
        let controllers: Vec<SdkController> = self
            .driver_devices
            .iter_mut()
            .filter_map(|d| {
                let info = d.info();
                d.lighting()
                    .map(|l| SdkController::from_device(&info, l.zones()))
            })
            .collect();
        if let Some(ref srv) = self.openrgb_server {
            srv.set_controllers(controllers);
            self.openrgb_server_error = None;
            return;
        }
        match OpenRgbSdkServer::start("127.0.0.1:6742", controllers) {
            Ok(srv) => {
                self.openrgb_server = Some(srv);
                self.openrgb_server_error = None;
            }
            Err(e) => {
                tracing::warn!("Pont OpenRGB SDK non démarré : {e:#}");
                self.openrgb_server = None;
                self.openrgb_server_error = Some(e.to_string());
            }
        }
    }

    fn poll_openrgb_server(&mut self) {
        let Some(ref srv) = self.openrgb_server else {
            return;
        };
        let events = srv.poll_events();
        if events.is_empty() || self.suspended || self.blackout_applied {
            return;
        }
        // Keep only the latest frame per device in this tick.
        let mut latest_frames: HashMap<String, Vec<Rgb>> = HashMap::new();
        for ev in events {
            if let OpenRgbServerEvent::UpdateLeds { device_id, colors } = ev {
                latest_frames.insert(device_id, colors);
            }
        }
        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            if let Some(frame) = latest_frames.get(&id)
                && let Some(lighting) = dev.lighting()
                && lighting.set_frame(frame).is_ok()
            {
                self.live_active_devices.insert(id.clone());
                self.last_device_frames.insert(id, frame.clone());
            }
        }
    }

    pub(crate) fn profile_needs_input_engine(&mut self) -> bool {
        let p = self.config.active_profile_mut();
        let any_reactive = p.rgb.kind == EffectKind::ReactiveTyping
            || p.rgb_layers
                .iter()
                .any(|l| l.enabled && l.effect.kind == EffectKind::ReactiveTyping)
            || p.device_lighting.values().any(|cfg| {
                cfg.enabled
                    && std::iter::once(cfg.mode.as_ref())
                        .flatten()
                        .chain(cfg.zones.values())
                        .any(|ov| {
                            matches!(
                                ov,
                                LightingOverride::Software { effect }
                                if effect.kind == EffectKind::ReactiveTyping
                            )
                        })
            });
        p.socd.enabled
            || p.key_chatter_ms > 0
            || p.mouse_debounce_ms > 0
            || p.win_lock
            || !p.bindings.is_empty()
            || !p.mouse_bindings.is_empty()
            || p.layers.values().any(|m| !m.is_empty())
            || p.mouse_layers.values().any(|m| !m.is_empty())
            || any_reactive
    }

    pub(crate) fn reapply_runtime_state(&mut self) {
        seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
        let prof = self.config.active_profile_mut().clone();
        self.blackout_applied = false;
        self.invalidate_rgb_frames();
        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            let Some(lighting) = dev.lighting() else {
                continue;
            };
            if let Some(cfg) = prof.device_lighting.get(&id) {
                if !cfg.enabled {
                    release_device_lighting(lighting, Some(cfg));
                } else if let Some(LightingOverride::Hardware { request }) = &cfg.mode {
                    let _ = apply_validated_hw_effect(lighting, request);
                }
                for ov in cfg.zones.values() {
                    if let LightingOverride::Hardware { request } = ov {
                        let _ = apply_validated_hw_effect(lighting, request);
                    }
                }
            }
        }
        if self.profile_needs_input_engine()
            && !self.input_engine.is_running()
            && let Err(e) = self.input_engine.start()
        {
            error!("Moteur clavier non redémarré : {e}");
        }
    }

    pub(crate) fn apply_restored_config(&mut self) {
        migrate_legacy_profiles(&mut self.config, &mut self.driver_devices);
        self.sync_input_rules();
        let need_engine = self.profile_needs_input_engine();
        if need_engine && !self.input_engine.is_running() {
            let _ = self.input_engine.start();
        } else if !need_engine && self.input_engine.is_running() {
            self.input_engine.stop();
        }
        self.reapply_runtime_state();
        self.apply_profile_fan_rules();
        self.persist_config();
    }

    pub(crate) fn push_blackout_frame(&mut self) {
        for dev in &mut self.driver_devices {
            if let Some(lighting) = dev.lighting() {
                let is_fast = lighting.max_fps() >= 20;
                if self.blackout_applied && !is_fast {
                    continue;
                }
                let len = frame_len(&lighting.zones());
                let _ = lighting.set_frame(&vec![Rgb::BLACK; len]);
            }
        }
        if !self.blackout_applied {
            info!("Éclairage coupé (session verrouillée ou inactivité)");
        }
        self.last_device_frames.clear();
        self.blackout_applied = true;
    }

    fn end_blackout(&mut self) {
        let prof = self.config.active_profile_mut().clone();
        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            let Some(lighting) = dev.lighting() else {
                continue;
            };
            let cfg = prof.device_lighting.get(&id);
            let enabled = cfg.is_some_and(|c| c.enabled);
            if !enabled {
                release_device_lighting(lighting, cfg);
            } else if let Some(LightingOverride::Hardware { request }) =
                cfg.and_then(|c| c.mode.as_ref())
            {
                let _ = apply_validated_hw_effect(lighting, request);
            }
        }
        self.invalidate_rgb_frames();
        self.blackout_applied = false;
        info!("Éclairage rétabli");
    }

    fn handle_laptop_ac_transition(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        if !self.laptop_battery.present {
            return;
        }
        let ac = self.laptop_battery.ac_online;
        let Some(prev) = self.last_ac_online else {
            self.last_ac_online = Some(ac);
            return;
        };
        if prev == ac {
            return;
        }
        self.last_ac_online = Some(ac);
        if !self.config.laptop_auto_battery_saver {
            return;
        }
        if !ac {
            info!(
                "Passage sur batterie — activation du profil autonomie (60 Hz + profil silencieux)"
            );
            self.ac_saved_settings.clear();
            let plan: Vec<(String, Vec<SaverChange>)> = self
                .driver_devices
                .iter()
                .map(|d| (d.info().id, battery_saver_changes(&d.settings())))
                .filter(|(_, changes)| !changes.is_empty())
                .collect();
            for (device, changes) in plan {
                for (setting, target, previous) in changes {
                    match self.set_device_setting(&device, &setting, &target) {
                        Ok(_) => self
                            .ac_saved_settings
                            .push((device.clone(), setting, previous)),
                        Err(e) => {
                            info!("Économie batterie : {setting} inchangé sur {device} ({e})")
                        }
                    }
                }
            }
        } else {
            info!("Passage sur secteur — restauration performances & fréquence d'écran");
            let saved = std::mem::take(&mut self.ac_saved_settings);
            // Without a saved rate (started on battery), the panel goes back to its fastest rate.
            let fallback_hz: Vec<(String, Value)> = self
                .driver_devices
                .iter()
                .filter_map(|d| {
                    let id = d.info().id;
                    let has_saved = saved.iter().any(|(dev, setting, _)| {
                        dev == &id && setting == setting_ids::DISPLAY_REFRESH_RATE
                    });
                    (!has_saved)
                        .then(|| highest_refresh_rate(&d.settings()))
                        .flatten()
                        .map(|v| (id, v))
                })
                .collect();
            let restores = saved.into_iter().chain(
                fallback_hz
                    .into_iter()
                    .map(|(d, v)| (d, setting_ids::DISPLAY_REFRESH_RATE.to_string(), v)),
            );
            for (device, setting, value) in restores {
                let current = self.device_mut(&device).and_then(|d| {
                    d.settings()
                        .into_iter()
                        .find(|s| s.id == setting)
                        .map(|s| s.value)
                });
                if current.as_ref() == Some(&value) {
                    continue;
                }
                if let Err(e) = self.set_device_setting(&device, &setting, &value) {
                    info!("Retour secteur : {setting} non restauré sur {device} ({e})");
                }
            }
        }
        if UI_VISIBLE.load(Ordering::Relaxed) {
            let st = self.full_state_json();
            let _ = proxy.send_event(UserEvent::EvalJs(format!(
                "window.__ogHotplug && window.__ogHotplug({st});"
            )));
        }
    }

    fn check_battery_alerts(&mut self) {
        if !self.config.battery_alerts {
            return;
        }
        let readings: Vec<(String, String, u8, bool)> = self
            .driver_devices
            .iter()
            .filter_map(|d| {
                let st = d.status();
                let pct = st.battery_pct?;
                let info = d.info();
                Some((info.id, info.model, pct, st.charging.unwrap_or(false)))
            })
            .collect();
        for (key, name, pct, charging) in readings {
            let last = self.battery_alerted.get(&key).copied();
            let (next, notify) = battery_alert_step(last, pct, charging);
            match next {
                Some(level) => {
                    self.battery_alerted.insert(key, level);
                }
                None => {
                    self.battery_alerted.remove(&key);
                }
            }
            if let Some(level) = notify {
                let body = format!("{name} : {pct} % restants — pense à la recharger.");
                let title = if level <= 5 {
                    "Batterie presque vide"
                } else {
                    "Batterie faible"
                };
                if !tray_notify(title, &body) {
                    info!("{title} — {body}");
                }
            }
        }
    }

    pub(crate) fn visible_apps_cached(&mut self) -> Vec<String> {
        if let Some((at, apps)) = &self.visible_apps_cache
            && at.elapsed() < Duration::from_secs(5)
        {
            return apps.clone();
        }
        let apps = list_visible_apps();
        self.visible_apps_cache = Some((Instant::now(), apps.clone()));
        apps
    }
    pub(crate) fn refresh_curve_states(&mut self) {
        let fresh = collect_curve_states(&mut self.driver_devices);
        let prev = std::mem::take(&mut self.curve_states);
        for (id, mut st) in fresh {
            if let Some(old) = prev.get(&id)
                && old.channels.len() == st.channels.len()
            {
                st.channels = old.channels.clone();
                st.trackers = old.trackers.clone();
            }
            self.curve_states.insert(id, st);
        }
    }
}

/// Model of the first laptop device whose fans OpenGear drives through `Cooling`, if any.
pub(crate) fn device_cooling_owner(devices: &mut [Box<dyn Device>]) -> Option<String> {
    devices.iter_mut().find_map(|d| {
        let info = d.info();
        (info.kind == DeviceKind::Laptop && d.cooling().is_some()).then_some(info.model)
    })
}

/// Read curve-cooling capability through `CurveCooling` for every motherboard controller when no
/// laptop EC owns chassis cooling.
pub(crate) fn collect_curve_states(
    devices: &mut [Box<dyn Device>],
) -> BTreeMap<String, CurveDeviceState> {
    let mut out = BTreeMap::new();
    if device_cooling_owner(devices).is_some() {
        return out;
    }
    let admin = is_elevated();
    for d in devices {
        let id = d.info().id;
        let Some(fc) = d.fan_curves() else {
            continue;
        };
        let (available, message) = fc.cooling_diagnostic();
        let channels = fc.curve_channels();
        let trackers = vec![FanCurveTracker::new(); channels.len()];
        out.insert(
            id,
            CurveDeviceState {
                is_admin: admin,
                available,
                message,
                initial: channels.clone(),
                channels,
                trackers,
            },
        );
    }
    out
}

pub(crate) fn migrate_legacy_profiles(config: &mut AppConfig, devices: &mut [Box<dyn Device>]) {
    let key_codes: Vec<Option<String>> = devices
        .iter_mut()
        .filter(|d| d.info().kind == DeviceKind::Keyboard)
        .find_map(|d| {
            d.lighting().map(|l| {
                l.zones()
                    .into_iter()
                    .flat_map(|z| z.leds.into_iter().map(|led| led.key_code))
                    .collect()
            })
        })
        .unwrap_or_default();
    for prof in &mut config.profiles {
        if prof.lighting_migrated {
            continue;
        }
        prof.rgb
            .migrate_per_key_codes(|idx| key_codes.get(idx).and_then(|o| o.as_deref()));
        prof.lighting_migrated = true;
    }
}

pub(crate) fn seed_and_resize_device_lighting(
    config: &mut AppConfig,
    devices: &mut [Box<dyn Device>],
) {
    let omen_sync = config.omen_sync_rgb;
    for prof in &mut config.profiles {
        for d in devices.iter_mut() {
            let id = d.info().id;
            let is_corsair = id.starts_with("corsair:");
            let is_asus = id.starts_with("asus_aura:");
            let is_omen = id.starts_with("hp_omen:");
            if d.lighting().is_none() {
                continue;
            }
            prof.device_lighting.entry(id).or_insert_with(|| {
                let mut cfg = DeviceLightingConfig::default();
                if is_corsair {
                    cfg.enabled = prof.enable_rgb;
                } else if is_asus {
                    cfg.enabled = prof.sync_asus_aura || prof.silent_loop3_thermal_rgb;
                    if prof.silent_loop3_thermal_rgb
                        && let Some(ch) = prof.silent_loop3_argb_channel
                    {
                        let mut eff = prof.rgb.clone();
                        eff.kind = EffectKind::Thermal;
                        cfg.zones.insert(
                            format!("ch_{ch}"),
                            LightingOverride::Software { effect: eff },
                        );
                    }
                    for (&ch, &count) in &prof.asus_argb_led_counts {
                        cfg.zone_led_counts
                            .insert(format!("ch_{ch}"), count.clamp(1, 120) as u16);
                    }
                } else if is_omen {
                    cfg.enabled = omen_sync;
                } else {
                    cfg.enabled = false;
                }
                cfg
            });
        }
    }

    let counts_map: HashMap<String, Vec<(String, u16)>> = config
        .active_profile_mut()
        .device_lighting
        .iter()
        .map(|(dev_id, cfg)| {
            (
                dev_id.clone(),
                cfg.zone_led_counts
                    .iter()
                    .map(|(z, &c)| (z.clone(), c))
                    .collect(),
            )
        })
        .collect();

    for d in devices.iter_mut() {
        let id = d.info().id;
        if let Some(counts) = counts_map.get(&id)
            && let Some(lighting) = d.lighting()
        {
            for (zone_id, led_count) in counts {
                let _ = lighting.resize_zone(zone_id, *led_count);
            }
        }
    }
}

pub(crate) fn apply_validated_hw_effect(
    lighting: &mut dyn opengear_core::lighting::Lighting,
    req: &EffectRequest,
) -> anyhow::Result<()> {
    let effects = lighting.hardware_effects();
    let def = effects
        .iter()
        .find(|e| e.id == req.effect)
        .ok_or_else(|| anyhow::anyhow!("Effet matériel inconnu : {}", req.effect))?;
    let validated = def.validate(req)?;
    lighting.apply_hardware_effect(&validated)
}

/// Releases software control and, when a hardware effect (`last_hardware` / `last_zone_hardware`)
/// was previously configured on this device, restores it in volatile RAM (`save = false`) instead
/// of leaving controllers such as ASUS Aura on their fallback mode.
pub(crate) fn release_device_lighting(
    lighting: &mut dyn opengear_core::lighting::Lighting,
    cfg: Option<&DeviceLightingConfig>,
) {
    if let Err(err) = lighting.release() {
        tracing::warn!(?err, "Échec release éclairage matériel");
    }
    if let Some(c) = cfg {
        if let Some(ref req) = c.last_hardware {
            let mut restore = req.clone();
            restore.save = false;
            tracing::info!(
                effect = %restore.effect,
                colors = ?restore.colors.iter().map(|c| c.to_hex()).collect::<Vec<_>>(),
                "Restauration effet matériel mémorisé (appareil)"
            );
            if let Err(err) = apply_validated_hw_effect(lighting, &restore) {
                tracing::warn!(?err, effect = %restore.effect, "Échec restauration effet matériel appareil");
            }
        }
        for (zid, req) in &c.last_zone_hardware {
            let mut restore = req.clone();
            restore.save = false;
            if restore.zone.is_none() {
                restore.zone = Some(zid.clone());
            }
            tracing::info!(
                zone = %zid,
                effect = %restore.effect,
                colors = ?restore.colors.iter().map(|c| c.to_hex()).collect::<Vec<_>>(),
                "Restauration effet matériel mémorisé (zone)"
            );
            if let Err(err) = apply_validated_hw_effect(lighting, &restore) {
                tracing::warn!(?err, zone = %zid, effect = %restore.effect, "Échec restauration effet matériel zone");
            }
        }
    }
}

struct LiveFrameCtx<'a> {
    t: f32,
    amb: Option<&'a AmbilightFrame>,
    reactive_centers: &'a [(f32, f32, f32)],
    audio: AudioSpectrum,
    thermal_col: Rgb,
}

fn render_effect_for_zone(
    effect: &RgbEffectConfig,
    zone: &Zone,
    placement: &opengear_core::lighting::Placement,
    ctx: &LiveFrameCtx<'_>,
) -> Vec<Rgb> {
    match effect.kind {
        EffectKind::Ambilight => zone
            .leds
            .iter()
            .map(|led| {
                let (nx, ny) = placement.map(led);
                ctx.amb
                    .map(|a| a.sample_uv(nx, ny).scale(effect.brightness))
                    .unwrap_or(effect.primary.scale(effect.brightness))
            })
            .collect(),
        EffectKind::ReactiveTyping => {
            let bg = effect.secondary.scale(0.14);
            let fg = effect.primary;
            let br = effect.brightness;
            let spd = effect.speed;
            zone.leds
                .iter()
                .map(|led| {
                    let (nx, ny) = placement.map(led);
                    let mut intensity = 0.0f32;
                    for &(cx, cy, age) in ctx.reactive_centers {
                        let dx = nx - cx;
                        let dy = (ny - cy) * 0.45;
                        let dist = (dx * dx + dy * dy).sqrt();
                        let radius = age * 0.95 * spd;
                        let ring = (1.0 - (dist - radius).abs() * 9.0).clamp(0.0, 1.0);
                        let fade = (1.0 - age / 1.1).clamp(0.0, 1.0);
                        intensity = intensity.max(ring * fade);
                    }
                    bg.lerp(fg, intensity).scale(br)
                })
                .collect()
        }
        EffectKind::AudioVisualizer => {
            let c1 = effect.primary;
            let c2 = effect.secondary;
            let br = effect.brightness;
            let bands = ctx.audio.bands;
            zone.leds
                .iter()
                .map(|led| {
                    let (nx, ny) = placement.map(led);
                    // Interpolate the 8 logarithmic FFT bands across horizontal position [0..1].
                    let pos = nx.clamp(0.0, 1.0) * 7.0;
                    let idx0 = (pos.floor() as usize).min(7);
                    let idx1 = (idx0 + 1).min(7);
                    let frac = pos - (idx0 as f32);
                    let band_lvl =
                        (bands[idx0] * (1.0 - frac) + bands[idx1] * frac).clamp(0.0, 1.0);
                    let height_fill = 1.0 - ny.clamp(0.0, 1.0);
                    let bar =
                        ((band_lvl * 1.1 - height_fill) * 5.0 + band_lvl * 0.4).clamp(0.0, 1.0);
                    let bg = c2.scale(0.07);
                    let peak_col = c1.lerp(c2, nx * 0.65 + (1.0 - ny) * 0.35);
                    bg.lerp(peak_col, bar).scale(br)
                })
                .collect()
        }
        EffectKind::Thermal => vec![ctx.thermal_col.scale(effect.brightness); zone.leds.len()],
        _ => render_frame(std::slice::from_ref(zone), placement, effect, ctx.t),
    }
}

pub(crate) fn thermal_rgb(temp_c: f32) -> Rgb {
    let t = temp_c.clamp(30.0, 85.0);
    if t < 48.0 {
        let f = (t - 30.0) / 18.0;
        Rgb::new(0, (208.0 + f * 25.0) as u8, (255.0 - f * 105.0) as u8)
    } else if t < 66.0 {
        let f = (t - 48.0) / 18.0;
        Rgb::new(
            (f * 255.0) as u8,
            (233.0 - f * 48.0) as u8,
            (150.0 - f * 120.0) as u8,
        )
    } else {
        let f = (t - 66.0) / 19.0;
        Rgb::new(255, (185.0 - f * 155.0) as u8, (30.0 + f * 40.0) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opengear_core::driver::{ChoiceOption, Section};

    fn profile(current: &str) -> Setting {
        Setting::choice(
            setting_ids::PERFORMANCE_PROFILE,
            "Mode",
            Section::Performance,
            ["quiet", "balanced", "performance"]
                .iter()
                .map(|id| ChoiceOption::new(*id, *id))
                .collect(),
            current,
        )
    }

    fn refresh(current: &str, rates: &[&str]) -> Setting {
        Setting::choice(
            setting_ids::DISPLAY_REFRESH_RATE,
            "Hz",
            Section::Power,
            rates.iter().map(|r| ChoiceOption::new(*r, *r)).collect(),
            current,
        )
    }

    #[test]
    fn battery_saver_lowers_profile_rate_and_backlight_and_remembers_them() {
        let settings = vec![
            profile("performance"),
            refresh("144", &["60", "144"]),
            Setting::toggle(setting_ids::BACKLIGHT, "Clavier", Section::Lighting, true),
            Setting::toggle("vendor.other", "Autre", Section::General, true),
        ];
        let changes = battery_saver_changes(&settings);
        assert_eq!(
            changes,
            vec![
                (
                    setting_ids::PERFORMANCE_PROFILE.to_string(),
                    Value::Text("quiet".into()),
                    Value::Text("performance".into())
                ),
                (
                    setting_ids::DISPLAY_REFRESH_RATE.to_string(),
                    Value::Text("60".into()),
                    Value::Text("144".into())
                ),
                (
                    setting_ids::BACKLIGHT.to_string(),
                    Value::Bool(false),
                    Value::Bool(true)
                ),
            ]
        );
    }

    #[test]
    fn battery_saver_skips_settings_already_low_or_unsupported() {
        let settings = vec![
            profile("quiet"),
            refresh("120", &["120", "165"]),
            Setting::toggle(setting_ids::BACKLIGHT, "Clavier", Section::Lighting, false),
        ];
        assert!(battery_saver_changes(&settings).is_empty());
        assert_eq!(
            highest_refresh_rate(&settings),
            Some(Value::Text("165".into()))
        );
        assert_eq!(highest_refresh_rate(&[profile("quiet")]), None);
    }
}
