use super::{
    BackendMsg, SHUTDOWN_REQUESTED, SMART_FAN_ACTIVE, THERMAL_RGB_ACTIVE, UI_VISIBLE, UserEvent,
    send_reply,
};
use crate::audio_ext::read_output_peak;
use crate::fan_watchdog;
use crate::local_assets::LocalAssetCatalog;
use crate::selftest::SelfTestReport;
use crate::system_events::SysEvent;
use crate::system_ext::{
    AmbilightFrame, get_foreground_exe_name, idle_duration, list_visible_apps,
    sample_ambilight_screen,
};
use crate::tray_icon::tray_notify;
use opengear_asus::{
    AsusWmiCoolingStatus, AuraMainboardController, apply_asus_wmi_curve, probe_asus_wmi_cooling,
    read_asus_wmi_channel,
};
use opengear_core::cooling::{FanChannelConfig, FanHeaderKind, FanPolicyMode, FanTempSource};
use opengear_core::driver::{Control, Device, DeviceKind, Setting, Value, setting_ids};
use opengear_core::lighting::{
    DeviceLightingConfig, EffectRequest, LightingOverride, Zone, frame_len, render_frame,
};
use opengear_mainboard::{
    HardwareSensorsSnapshot, RamRgbDevice, SuperIoSnapshot, read_superio_snapshot,
    restore_all_superio_fans_auto,
};
use opengear_platform::{
    LaptopBatteryInfo, PowerPlan, SystemTelemetry, TelemetryCollector, is_elevated,
    is_laptop_chassis, list_power_plans, read_laptop_battery,
};

const SMART_FAN_FAILSAFE_TEMP_C: f32 = 100.0;
use super::keyboard_view::KeyboardView;
use opengear_core::{
    AppConfig, DetectedDevice, EffectKind, Rgb, RgbEffectConfig, scan_hid_devices,
};
use opengear_corsair::{CorsairDevice, CorsairDeviceKind, key_code_for_led_index};
use opengear_input::{InputEngine, reset_held_state};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tao::event_loop::EventLoopProxy;
use tracing::{error, info};

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
    pub cooling_status: AsusWmiCoolingStatus,
    pub cooling_channels: Vec<FanChannelConfig>,
    pub initial_cooling_channels: Vec<Option<FanChannelConfig>>,
    pub input_engine: InputEngine,
    pub scanned_devices: Vec<DetectedDevice>,
    pub start_time: Instant,
    pub foreground_app: Option<String>,
    pub manual_profile_name: String,
    pub auto_switched: bool,
    pub last_ambilight: Option<AmbilightFrame>,
    pub audio_smooth_peak: f32,
    pub last_device_frame_at: HashMap<String, Instant>,
    pub live_active_devices: HashSet<String>,
    pub smart_fan_last_duty: [Option<u8>; 5],
    pub smart_fan_ref_temp: [f32; 5],
    pub superio: SuperIoSnapshot,
    pub last_selftest: Option<SelfTestReport>,
    pub undo_stack: Vec<(String, AppConfig)>,
    pub visible_apps_cache: Option<(Instant, Vec<String>)>,
    pub watchdog_spawned: bool,
    pub suspended: bool,
    pub resume_reconnect_at: Option<Instant>,
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

pub(crate) fn compute_hid_signature() -> Vec<(u16, u16, u16)> {
    let mut sig: Vec<(u16, u16, u16)> = scan_hid_devices(false)
        .unwrap_or_default()
        .into_iter()
        .map(|d| (d.vid, d.pid, d.usage_page))
        .collect();
    sig.sort_unstable();
    sig.dedup();
    sig
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
        migrate_legacy_profiles(&mut config);
        let scanned_devices = scan_hid_devices(false).unwrap_or_default();
        let last_hid_sig = compute_hid_signature();
        let power_plans = list_power_plans();

        let drivers = crate::drivers::device_drivers();
        let mut driver_devices =
            super::devices::discover_all(&drivers, &config.experimental_drivers);
        seed_and_resize_device_lighting(&mut config, &mut driver_devices);

        if let Some(kb) = driver_devices.iter().find_map(|d| {
            let c = d.as_any()?.downcast_ref::<CorsairDevice>()?;
            (c.info.kind == CorsairDeviceKind::Keyboard).then_some(c)
        }) {
            let pr = kb.info.pollrate;
            let br = kb.hw_brightness_pct();
            let prof = config.active_profile_mut();
            if let Some(rate) = pr {
                prof.polling_rate = rate;
            }
            prof.hw_brightness_pct = br;
        }

        let mut telemetry = TelemetryCollector::new();
        let last_tel = telemetry.sample();
        let is_laptop = is_laptop_chassis()
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
        let device_fans = device_cooling_owner(&mut driver_devices);
        let cooling_status = mainboard_cooling_status(device_fans.as_deref());
        let initial_cooling_channels: Vec<Option<FanChannelConfig>> = FanHeaderKind::ALL
            .iter()
            .map(|&h| {
                if cooling_status.wmi_available {
                    read_asus_wmi_channel(h)
                } else {
                    None
                }
            })
            .collect();
        // Laptop fans belong to the embedded controller (driven through a device's `Cooling`),
        // never to motherboard fan headers.
        let cooling_channels: Vec<FanChannelConfig> =
            if device_fans.is_some() || (is_laptop && !cooling_status.wmi_available) {
                Vec::new()
            } else {
                FanHeaderKind::ALL
                    .iter()
                    .zip(&initial_cooling_channels)
                    .map(|(&h, read)| {
                        read.clone()
                            .unwrap_or_else(|| FanChannelConfig::default_for(h))
                    })
                    .collect()
            };
        let superio = if is_laptop {
            SuperIoSnapshot::default()
        } else {
            read_superio_snapshot()
        };

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
            cooling_status,
            cooling_channels,
            initial_cooling_channels,
            input_engine: InputEngine::new(),
            scanned_devices,
            start_time: Instant::now(),
            foreground_app: None,
            manual_profile_name: String::new(),
            auto_switched: false,
            last_ambilight: None,
            audio_smooth_peak: 0.0,
            last_device_frame_at: HashMap::new(),
            live_active_devices: HashSet::new(),
            smart_fan_last_duty: [None; 5],
            smart_fan_ref_temp: [40.0; 5],
            superio,
            last_selftest: None,
            undo_stack: Vec::new(),
            visible_apps_cache: None,
            watchdog_spawned: false,
            suspended: false,
            resume_reconnect_at: None,
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
        };
        // A device may already be overridden (e.g. Max Fan left on): arm the crash safety net.
        eng.sync_cooling_marker();
        eng.manual_profile_name = eng.config.active_profile.clone();
        eng.sync_input_rules();
        let p0 = eng.config.active_profile_mut().clone();
        if p0.socd.enabled
            || p0.key_chatter_ms > 0
            || p0.mouse_debounce_ms > 0
            || p0.win_lock
            || !p0.bindings.is_empty()
        {
            let _ = eng.input_engine.start();
        }
        eng
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

    fn wmi_restore_config(&self, idx: usize) -> Option<FanChannelConfig> {
        let header = FanHeaderKind::ALL.get(idx).copied()?;
        Some(
            match self.initial_cooling_channels.get(idx).cloned().flatten() {
                Some(original) => original,
                None => {
                    let mut auto = FanChannelConfig::default_for(header);
                    auto.policy = FanPolicyMode::Standard;
                    auto
                }
            },
        )
    }

    pub(crate) fn sync_fan_marker(&mut self) {
        let marker = fan_watchdog::FanOverrideMarker {
            pid: std::process::id(),
            wmi_restore: (0..self.smart_fan_last_duty.len())
                .filter(|&i| self.smart_fan_last_duty[i].is_some())
                .filter_map(|i| self.wmi_restore_config(i))
                .collect(),
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

    pub(crate) fn restore_hardware_on_exit(&mut self) {
        self.input_engine.stop();
        reset_held_state();

        for dev in &mut self.driver_devices {
            if let Some(lighting) = dev.lighting() {
                let _ = lighting.release();
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

        let mut remaining_wmi = Vec::new();
        for i in 0..self.smart_fan_last_duty.len() {
            if self.smart_fan_last_duty[i].take().is_some()
                && let Some(cfg) = self.wmi_restore_config(i)
                && let Err(e) = apply_asus_wmi_curve(&cfg)
            {
                error!(
                    "Restauration ventilateur {:?} impossible à la fermeture : {e}",
                    cfg.header
                );
                remaining_wmi.push(cfg);
            }
        }
        let superio_logs = restore_all_superio_fans_auto();
        let superio_failed = superio_logs.iter().any(|l| {
            let low = l.to_ascii_lowercase();
            low.contains("échou") || low.contains("erreur") || low.contains("impossible")
        });
        let remaining_superio = if superio_failed {
            opengear_mainboard::saved_superio_states()
        } else {
            Vec::new()
        };
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

        loop {
            if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                self.restore_hardware_on_exit();
                break;
            }

            let ui_open = UI_VISIBLE.load(Ordering::Relaxed);
            let (live_rgb_active, animated_rgb_active, thermal_rgb_active) =
                self.lighting_activity_flags();
            let pending_static_rgb = live_rgb_active && self.has_pending_live_frame();
            let smart_fan_active = self.cooling_status.is_admin
                && self.cooling_status.wmi_available
                && self
                    .cooling_channels
                    .iter()
                    .any(|c| c.source != FanTempSource::CpuBios);
            THERMAL_RGB_ACTIVE.store(thermal_rgb_active, Ordering::Relaxed);
            SMART_FAN_ACTIVE.store(smart_fan_active, Ordering::Relaxed);

            let wait_ms = if live_rgb_active
                && (animated_rgb_active || pending_static_rgb)
                && !self.blackout_applied
                && !self.suspended
            {
                12
            } else if ui_open || smart_fan_active {
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
                        let st = self.full_state_json();
                        let _ = proxy.send_event(UserEvent::EvalJs(format!(
                            "window.__ogHotplug && window.__ogHotplug({st});"
                        )));
                    }
                }
                last_fg_check = Instant::now();
            }

            // 1. Live telemetry push
            let tel_interval = if ui_open {
                Duration::from_secs(1)
            } else {
                Duration::from_millis(2500)
            };
            if (ui_open || thermal_rgb_active || smart_fan_active)
                && last_tel_push.elapsed() >= tel_interval
            {
                self.last_tel = self.telemetry.sample();
                if ui_open {
                    let tel_json = self.telemetry_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogTelemetry && window.__ogTelemetry({tel_json});"
                    )));
                }
                last_tel_push = Instant::now();
            }

            // 1b. Smart Cooling (GPU / Max(CPU, GPU) with hysteresis)
            if smart_fan_active && last_smart_fan.elapsed() >= Duration::from_secs(3) {
                let mut marker_dirty = false;
                let sensors = self.latest_sensors();
                let gpu_edge: Option<f32> = self
                    .last_tel
                    .gpu
                    .edge_temp_c
                    .or(self.last_tel.gpu.hotspot_temp_c);
                let gpu_hot: Option<f32> = self
                    .last_tel
                    .gpu
                    .hotspot_temp_c
                    .or(self.last_tel.gpu.edge_temp_c);
                let cpu_t: Option<f32> = sensors.cpu_temp_c;
                for (idx, ch) in self.cooling_channels.iter().enumerate() {
                    if ch.source == FanTempSource::CpuBios || idx >= 5 {
                        continue;
                    }
                    let measured = match ch.source {
                        FanTempSource::GpuEdge => gpu_edge,
                        FanTempSource::GpuHotspot => gpu_hot,
                        FanTempSource::MaxCpuGpu => match (cpu_t, gpu_edge) {
                            (Some(c), Some(g)) => Some(c.max(g)),
                            (c, g) => c.or(g),
                        },
                        FanTempSource::CpuBios => continue,
                    };
                    let raw_t = measured.unwrap_or(SMART_FAN_FAILSAFE_TEMP_C);
                    let hyst = ch.hysteresis_c as f32;
                    let ref_t = self.smart_fan_ref_temp[idx];
                    // Rising: follow immediately. Falling: only once below the hysteresis band.
                    let eff_t = if raw_t >= ref_t || raw_t <= ref_t - hyst {
                        self.smart_fan_ref_temp[idx] = raw_t;
                        raw_t
                    } else {
                        ref_t
                    };
                    let target_duty = ch
                        .interpolate_duty(eff_t)
                        .max(ch.header.min_safe_duty_pct());
                    let changed = self.smart_fan_last_duty[idx]
                        .map(|prev| prev.abs_diff(target_duty) >= 3)
                        .unwrap_or(true);
                    if changed {
                        let mut flat = ch.clone();
                        flat.policy = FanPolicyMode::Manual;
                        for pt in &mut flat.curve {
                            pt.duty_pct = target_duty;
                        }
                        if apply_asus_wmi_curve(&flat).is_ok() {
                            let newly_overridden = self.smart_fan_last_duty[idx].is_none();
                            self.smart_fan_last_duty[idx] = Some(target_duty);
                            if newly_overridden {
                                marker_dirty = true;
                            }
                        }
                    }
                }
                if marker_dirty {
                    self.sync_fan_marker();
                }
                last_smart_fan = Instant::now();
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

            // 4. Live RGB streaming (fast 35ms only for animated effects or pending static flush)
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
                if UI_VISIBLE.load(Ordering::Relaxed) {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
            }
            BackendMsg::TrayToggleSync => {
                self.tray_toggle_sync();
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
                    self.superio = read_superio_snapshot();
                    self.driver_devices = super::devices::discover_all(
                        &self.drivers,
                        &self.config.experimental_drivers,
                    );
                    seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
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
                    let res = self.handle_cmd(&req.cmd, &req.payload, proxy);
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
                self.smart_fan_last_duty = [None; 5];
                self.resume_reconnect_at = Some(Instant::now() + Duration::from_millis(1200));
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
                let kind = match ov {
                    LightingOverride::Global => prof.rgb.kind,
                    LightingOverride::Software { effect } => effect.kind,
                    LightingOverride::Hardware { .. } | LightingOverride::Off => return,
                };
                any_live = true;
                match kind {
                    EffectKind::Static | EffectKind::PerKey | EffectKind::Gradient | EffectKind::Off => {}
                    EffectKind::Thermal => any_thermal = true,
                    _ => any_animated = true,
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
            has_sw && !self.last_device_frame_at.contains_key(&id)
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

    fn push_live_rgb_frame(&mut self) {
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
            }
        }

        if need_ambilight && let Some(frame) = sample_ambilight_screen(self.last_ambilight.as_ref())
        {
            self.last_ambilight = Some(frame);
        }

        if need_audio {
            let raw = (read_output_peak() * 1.45).clamp(0.0, 1.0);
            if raw > self.audio_smooth_peak {
                self.audio_smooth_peak = self.audio_smooth_peak * 0.35 + raw * 0.65;
            } else {
                self.audio_smooth_peak *= 0.84;
            }
        }

        let reactive_centers: Vec<(f32, f32, f32)> = if need_reactive {
            if !self.input_engine.is_running() {
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
        let audio_peak = self.audio_smooth_peak;
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
                    let _ = lighting.release();
                }
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
                audio_peak,
                thermal_col,
            };

            for zone in &zones {
                let ov = cfg.zones.get(&zone.id).unwrap_or(base);
                match ov {
                    LightingOverride::Off | LightingOverride::Hardware { .. } => {
                        frame.extend(std::iter::repeat_n(Rgb::BLACK, zone.leds.len()));
                    }
                    LightingOverride::Global => {
                        frame.extend(render_effect_for_zone(
                            &prof.rgb, zone, &placement, &frame_ctx,
                        ));
                    }
                    LightingOverride::Software { effect } => {
                        frame.extend(render_effect_for_zone(effect, zone, &placement, &frame_ctx));
                    }
                }
            }

            let _ = lighting.set_frame(&frame);
            self.live_active_devices.insert(id.clone());
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

    pub(crate) fn reconnect_vendors(&mut self, vendors: &[u16]) {
        self.rediscover_drivers_for(vendors);
        seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
        self.last_device_frame_at.clear();
        self.live_active_devices.clear();
        self.invalidate_keyboard_view();
        self.scanned_devices = scan_hid_devices(false).unwrap_or_default();
    }

    pub(crate) fn reconnect_all(&mut self) {
        self.driver_devices =
            super::devices::discover_all(&self.drivers, &self.config.experimental_drivers);
        seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
        self.last_device_frame_at.clear();
        self.live_active_devices.clear();
        self.invalidate_keyboard_view();

        self.cooling_keepalive.clear();
        let device_fans = device_cooling_owner(&mut self.driver_devices);
        self.cooling_status = mainboard_cooling_status(device_fans.as_deref());
        self.sync_cooling_marker();
        self.power_plans = list_power_plans();
        self.scanned_devices = scan_hid_devices(false).unwrap_or_default();
        self.last_hid_sig = compute_hid_signature();
    }

    pub(crate) fn profile_needs_input_engine(&mut self) -> bool {
        let p = self.config.active_profile_mut();
        let any_reactive = p.rgb.kind == EffectKind::ReactiveTyping
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
            || any_reactive
    }

    pub(crate) fn reapply_runtime_state(&mut self) {
        seed_and_resize_device_lighting(&mut self.config, &mut self.driver_devices);
        let prof = self.config.active_profile_mut().clone();
        self.blackout_applied = false;
        self.last_device_frame_at.clear();
        for dev in &mut self.driver_devices {
            let id = dev.info().id;
            let Some(lighting) = dev.lighting() else {
                continue;
            };
            if let Some(cfg) = prof.device_lighting.get(&id) {
                if !cfg.enabled {
                    let _ = lighting.release();
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
        migrate_legacy_profiles(&mut self.config);
        self.sync_input_rules();
        let need_engine = self.profile_needs_input_engine();
        if need_engine && !self.input_engine.is_running() {
            let _ = self.input_engine.start();
        } else if !need_engine && self.input_engine.is_running() {
            self.input_engine.stop();
        }
        self.reapply_runtime_state();
        self.persist_config();
    }

    pub(crate) fn push_blackout_frame(&mut self) {
        for dev in &mut self.driver_devices {
            let is_fast = dev
                .as_any()
                .is_some_and(|a| a.is::<CorsairDevice>() || a.is::<AuraMainboardController>());
            if self.blackout_applied && !is_fast {
                continue;
            }
            if let Some(lighting) = dev.lighting() {
                let len = frame_len(&lighting.zones());
                let _ = lighting.set_frame(&vec![Rgb::BLACK; len]);
            }
        }
        if !self.blackout_applied {
            info!("Éclairage coupé (session verrouillée ou inactivité)");
        }
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
                let _ = lighting.release();
            } else if let Some(LightingOverride::Hardware { request }) =
                cfg.and_then(|c| c.mode.as_ref())
            {
                let _ = apply_validated_hw_effect(lighting, request);
            }
        }
        self.last_device_frame_at.clear();
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
}

/// Model of the first device whose fans OpenGear drives through `Cooling`, if any.
pub(crate) fn device_cooling_owner(devices: &mut [Box<dyn Device>]) -> Option<String> {
    devices.iter_mut().find_map(|d| {
        let model = d.info().model;
        d.cooling().map(|_| model)
    })
}

/// Motherboard fan interface status. When a device already drives the fans (laptop embedded
/// controller), the ASUS WMI fan interface is not probed.
pub(crate) fn mainboard_cooling_status(device_fans: Option<&str>) -> AsusWmiCoolingStatus {
    match device_fans {
        Some(model) => AsusWmiCoolingStatus {
            is_admin: is_elevated(),
            wmi_available: false,
            message: format!("Ventilateurs pilotés par l'appareil « {model} »."),
        },
        None => probe_asus_wmi_cooling(),
    }
}

pub(crate) fn migrate_legacy_profiles(config: &mut AppConfig) {
    for prof in &mut config.profiles {
        if prof.lighting_migrated {
            continue;
        }
        prof.rgb
            .migrate_per_key_codes(|idx| key_code_for_led_index(0x1B1C, 0x1B73, idx));
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
            let is_corsair = d.as_any().is_some_and(|a| a.is::<CorsairDevice>());
            let is_asus = d
                .as_any()
                .is_some_and(|a| a.is::<AuraMainboardController>());
            // Legacy `omen_sync_rgb` only seeds the first lighting config of the HP OMEN
            // keyboard (matched by its device id, no downcast).
            let is_omen = id.starts_with("hp_omen:");
            let _is_ram = d.as_any().is_some_and(|a| a.is::<RamRgbDevice>());
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

struct LiveFrameCtx<'a> {
    t: f32,
    amb: Option<&'a AmbilightFrame>,
    reactive_centers: &'a [(f32, f32, f32)],
    audio_peak: f32,
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
            let lvl = ctx.audio_peak;
            let c1 = effect.primary;
            let c2 = effect.secondary;
            let br = effect.brightness;
            zone.leds
                .iter()
                .map(|led| {
                    let (nx, ny) = placement.map(led);
                    let dist = (nx - 0.5).abs() * 2.0;
                    let bar = ((lvl - dist) * 5.0).clamp(0.0, 1.0);
                    let bg = c2.scale(0.08);
                    let peak_col = c1.lerp(c2, dist * 0.7 + (1.0 - ny) * 0.3);
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
