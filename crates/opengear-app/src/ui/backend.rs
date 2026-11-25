use super::state::battery_is_charging;
use super::{
    BackendMsg, SHUTDOWN_REQUESTED, SMART_FAN_ACTIVE, THERMAL_RGB_ACTIVE, UI_VISIBLE, UserEvent,
    send_reply, should_use_live_rgb,
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
    AsusWmiCoolingStatus, AuraChannelKind, AuraHwMode, AuraMainboardController,
    apply_asus_wmi_curve, probe_asus_wmi_cooling, read_asus_wmi_channel,
};
use opengear_core::cooling::{FanChannelConfig, FanHeaderKind, FanPolicyMode, FanTempSource};
use opengear_hp::{
    HpOmenStatus, OmenKeyboardType, OmenThermalMode, probe_hp_omen, read_hp_omen_live_fans,
    set_hp_omen_fan_level, set_hp_omen_kb_backlight, set_hp_omen_kb_zones, set_hp_omen_max_fan,
    set_hp_omen_thermal_mode,
};
use opengear_mainboard::{
    EneRamMode, HardwareSensorsSnapshot, RamRgbStatus, SuperIoSnapshot, read_superio_snapshot,
    restore_all_superio_fans_auto,
};
use opengear_platform::{
    DisplayInfo, LaptopBatteryInfo, PowerPlan, SystemTelemetry, TelemetryCollector, is_elevated,
    is_laptop_chassis, list_power_plans, read_display_info, read_laptop_battery,
    set_display_refresh_rate,
};

const SMART_FAN_FAILSAFE_TEMP_C: f32 = 100.0;
use super::keyboard_view::KeyboardView;
use opengear_core::{AppConfig, DetectedDevice, EffectKind, OperatingMode, Rgb, scan_hid_devices};
use opengear_corsair::{CorsairDevice, CorsairDeviceKind};
use opengear_input::{InputEngine, reset_held_state};
use opengear_logitech::{LogitechHeadset, LogitechKeyboard, LogitechMouse};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tao::event_loop::EventLoopProxy;
use tracing::{error, info};

const ARGB_MAX_LEDS: usize = 120;

pub(crate) struct BackendEngine {
    pub config_path: PathBuf,
    pub config: AppConfig,
    pub config_warning: Option<String>,
    pub local_assets: Arc<LocalAssetCatalog>,
    pub shared_sensors: Arc<Mutex<HardwareSensorsSnapshot>>,
    pub corsair_devices: Vec<CorsairDevice>,
    pub asus_aura: Option<AuraMainboardController>,
    pub asus_err: Option<String>,
    pub logi_mice: Vec<LogitechMouse>,
    pub logi_keyboards: Vec<LogitechKeyboard>,
    pub logi_headsets: Vec<LogitechHeadset>,
    /// Registered device drivers (see `crate::drivers::device_drivers`).
    pub drivers: Vec<Box<dyn opengear_core::driver::Driver>>,
    /// Devices opened through `drivers`.
    pub driver_devices: Vec<Box<dyn opengear_core::driver::Device>>,
    pub last_hid_sig: Vec<(u16, u16, u16)>,
    pub telemetry: TelemetryCollector,
    pub last_tel: SystemTelemetry,
    pub power_plans: Vec<PowerPlan>,
    pub cooling_status: AsusWmiCoolingStatus,
    pub cooling_channels: Vec<FanChannelConfig>,
    pub initial_cooling_channels: Vec<Option<FanChannelConfig>>,
    pub ram_rgb: RamRgbStatus,
    pub input_engine: InputEngine,
    pub scanned_devices: Vec<DetectedDevice>,
    pub aura_hw_mode: AuraHwMode,
    pub aura_hw_color: Rgb,
    pub aura_shutdown_mode: AuraHwMode,
    pub aura_shutdown_color: Rgb,
    pub start_time: Instant,
    pub foreground_app: Option<String>,
    pub manual_profile_name: String,
    pub auto_switched: bool,
    pub last_ambilight: Option<AmbilightFrame>,
    pub audio_smooth_peak: f32,
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
    pub battery_alerted: std::collections::HashMap<String, u8>,
    pub ram_restore_modes: Option<Vec<(u8, u8)>>,
    pub is_laptop: bool,
    /// Key editor geometry + active Windows layout, rebuilt every few seconds (see `keyboard_view`).
    pub keyboard_view_cache: Option<(Instant, KeyboardView)>,
    pub hp_omen: HpOmenStatus,
    pub laptop_battery: LaptopBatteryInfo,
    pub laptop_display: DisplayInfo,
    pub omen_color_table: Option<Vec<u8>>,
    pub omen_last_rgb_push: Instant,
    pub omen_last_fan_keepalive: Instant,
    pub omen_fan_modified: bool,
    pub omen_kb_restore_backlight: Option<bool>,
    pub last_ac_online: Option<bool>,
    pub ac_saved_hz: Option<u32>,
    pub ac_saved_thermal: Option<OmenThermalMode>,
    pub ac_saved_kb_backlight: Option<bool>,
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
        let scanned_devices = scan_hid_devices(false).unwrap_or_default();
        let last_hid_sig = compute_hid_signature();
        let power_plans = list_power_plans();

        let corsair_devices = CorsairDevice::connect_all();
        let drivers = crate::drivers::device_drivers();
        let driver_devices = super::devices::discover_all(&drivers);
        let (mut asus_aura, asus_err) = match AuraMainboardController::connect() {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let logi_mice = LogitechMouse::connect_all();
        let logi_keyboards = LogitechKeyboard::connect_all();
        let logi_headsets = LogitechHeadset::connect_all();

        if let Some(aura) = asus_aura.as_mut() {
            let counts = config.active_profile_mut().asus_argb_led_counts.clone();
            for (ch_idx, count) in counts {
                aura.set_channel_led_count(ch_idx, count);
            }
        }
        if let Some(kb) = corsair_devices
            .iter()
            .find(|d| d.info.kind == CorsairDeviceKind::Keyboard)
        {
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
        let hp_omen = probe_hp_omen();
        let is_laptop = is_laptop_chassis() || hp_omen.detected;
        let laptop_battery = if is_laptop {
            read_laptop_battery()
        } else {
            LaptopBatteryInfo::default()
        };
        let laptop_display = if is_laptop {
            read_display_info()
        } else {
            DisplayInfo::default()
        };
        let last_ac_online = if laptop_battery.present {
            Some(laptop_battery.ac_online)
        } else {
            None
        };
        let cooling_status = if hp_omen.detected {
            AsusWmiCoolingStatus {
                is_admin: is_elevated(),
                wmi_available: false,
                message: "PC portable HP OMEN détecté : contrôle ventilateurs via HP BIOS WMI (hpqBIntM).".into(),
            }
        } else {
            probe_asus_wmi_cooling()
        };
        let ram_rgb = crate::drivers::ram_rgb().probe();
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
        let cooling_channels: Vec<FanChannelConfig> = if hp_omen.detected {
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
            corsair_devices,
            asus_aura,
            asus_err,
            logi_mice,
            logi_keyboards,
            logi_headsets,
            drivers,
            driver_devices,
            last_hid_sig,
            telemetry,
            last_tel,
            power_plans,
            cooling_status,
            cooling_channels,
            initial_cooling_channels,
            ram_rgb,
            input_engine: InputEngine::new(),
            scanned_devices,
            aura_hw_mode: AuraHwMode::Static,
            aura_hw_color: Rgb::new(255, 24, 72),
            aura_shutdown_mode: AuraHwMode::Off,
            aura_shutdown_color: Rgb::BLACK,
            start_time: Instant::now(),
            foreground_app: None,
            manual_profile_name: String::new(),
            auto_switched: false,
            last_ambilight: None,
            audio_smooth_peak: 0.0,
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
            battery_alerted: std::collections::HashMap::new(),
            ram_restore_modes: None,
            is_laptop,
            keyboard_view_cache: None,
            hp_omen,
            laptop_battery,
            laptop_display,
            omen_color_table: None,
            omen_last_rgb_push: Instant::now(),
            omen_last_fan_keepalive: Instant::now(),
            omen_fan_modified: false,
            omen_kb_restore_backlight: None,
            last_ac_online,
            ac_saved_hz: None,
            ac_saved_thermal: None,
            ac_saved_kb_backlight: None,
        };
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
        };
        fan_watchdog::write_marker(&marker);
        if !marker.is_empty() && !self.watchdog_spawned {
            self.watchdog_spawned = fan_watchdog::spawn_watchdog().is_ok();
        }
    }

    pub(crate) fn restore_hardware_on_exit(&mut self) {
        self.input_engine.stop();
        reset_held_state();

        for dev in &mut self.corsair_devices {
            if dev.info.kind == CorsairDeviceKind::Keyboard
                && dev.info.mode == OperatingMode::Software
            {
                let _ = dev.set_mode(OperatingMode::Hardware);
            }
        }
        if let Some(aura) = self.asus_aura.as_mut()
            && aura.is_in_direct_mode()
        {
            let _ = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false);
        }
        self.ram_restore();
        if self.omen_fan_modified && self.hp_omen.wmi_accessible {
            let _ = set_hp_omen_max_fan(false);
            let _ = set_hp_omen_fan_level(None, &self.hp_omen.fan_max_rpms);
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
        if remaining_wmi.is_empty() && remaining_superio.is_empty() {
            fan_watchdog::clear_marker();
        } else {
            fan_watchdog::write_marker(&fan_watchdog::FanOverrideMarker {
                pid: std::process::id(),
                wmi_restore: remaining_wmi,
                superio_restore: remaining_superio,
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
        let mut last_omen_fan_poll = Instant::now();

        loop {
            if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                self.restore_hardware_on_exit();
                break;
            }

            let ui_open = UI_VISIBLE.load(Ordering::Relaxed);
            let prof_snapshot = self.config.active_profile_mut().clone();
            let is_corsair_kb_sw = self
                .corsair_devices
                .iter()
                .any(|d| d.info.kind == CorsairDeviceKind::Keyboard);
            let omen_sync_active = self.config.omen_sync_rgb
                && self.hp_omen.wmi_accessible
                && self.hp_omen.keyboard_type.is_software_rgb();
            let live_rgb_active =
                should_use_live_rgb(&prof_snapshot, is_corsair_kb_sw, omen_sync_active);
            let thermal_rgb_active = prof_snapshot.silent_loop3_thermal_rgb;
            let smart_fan_active = self.cooling_status.is_admin
                && self.cooling_status.wmi_available
                && self
                    .cooling_channels
                    .iter()
                    .any(|c| c.source != FanTempSource::CpuBios);
            THERMAL_RGB_ACTIVE.store(thermal_rgb_active, Ordering::Relaxed);
            SMART_FAN_ACTIVE.store(smart_fan_active, Ordering::Relaxed);

            let wait_ms = if live_rgb_active && !self.blackout_applied && !self.suspended {
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

            // 3. Battery refresh + Laptop/OMEN polls
            let logi_interval = if ui_open {
                Duration::from_secs(30)
            } else {
                Duration::from_secs(300)
            };
            if !self.suspended && last_logi_poll.elapsed() >= logi_interval {
                for m in &mut self.logi_mice {
                    m.refresh_telemetry();
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
                    self.laptop_display = read_display_info();
                }
                self.handle_laptop_ac_transition(&proxy);
                last_laptop_poll = Instant::now();
            }

            if !self.suspended
                && self.hp_omen.wmi_accessible
                && ui_open
                && last_omen_fan_poll.elapsed() >= Duration::from_secs(2)
            {
                if let Some((rpms, max_fan)) = read_hp_omen_live_fans() {
                    self.hp_omen.fan_rpms = rpms;
                    self.hp_omen.max_fan = max_fan;
                }
                last_omen_fan_poll = Instant::now();
            }

            // HP OMEN EC watchdog keep-alive (BIOS reverts manual fan speed after ~30-60s otherwise)
            if !self.suspended
                && self.hp_omen.wmi_accessible
                && self.omen_last_fan_keepalive.elapsed() >= Duration::from_secs(12)
            {
                if let Some(pct) = self.hp_omen.manual_fan_pct {
                    let _ = set_hp_omen_fan_level(Some(pct), &self.hp_omen.fan_max_rpms);
                }
                self.omen_last_fan_keepalive = Instant::now();
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

            // 4. Live RGB streaming
            let rgb_interval = if live_rgb_active {
                Duration::from_millis(35)
            } else {
                Duration::from_millis(500)
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
                    self.ram_rgb = crate::drivers::ram_rgb().probe();
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
            let view = KeyboardView::build(&self.config, &self.corsair_devices, self.is_laptop);
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

    fn push_live_rgb_frame(&mut self) {
        let t = self.start_time.elapsed().as_secs_f32();
        let prof = self.config.active_profile_mut().clone();

        let omen_sync = self.config.omen_sync_rgb
            && self.hp_omen.wmi_accessible
            && self.hp_omen.keyboard_type == OmenKeyboardType::FourZoneRgb;
        let any_strip_rgb = prof.enable_rgb || prof.sync_asus_aura || omen_sync;

        let needs_ambilight = any_strip_rgb && prof.rgb.kind == EffectKind::Ambilight;
        if needs_ambilight
            && let Some(frame) = sample_ambilight_screen(self.last_ambilight.as_ref())
        {
            self.last_ambilight = Some(frame);
        }

        let needs_audio_vis = any_strip_rgb && prof.rgb.kind == EffectKind::AudioVisualizer;
        if needs_audio_vis {
            let raw = (read_output_peak() * 1.45).clamp(0.0, 1.0);
            if raw > self.audio_smooth_peak {
                self.audio_smooth_peak = self.audio_smooth_peak * 0.35 + raw * 0.65;
            } else {
                self.audio_smooth_peak *= 0.84;
            }
        }

        let reactive_centers: Vec<(f32, f32, f32)> =
            if any_strip_rgb && prof.rgb.kind == EffectKind::ReactiveTyping {
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

        if prof.enable_rgb {
            for dev in &mut self.corsair_devices {
                if dev.info.kind == CorsairDeviceKind::Keyboard {
                    match prof.rgb.kind {
                        EffectKind::Ambilight => {
                            if let Some(ref amb) = self.last_ambilight {
                                let br = prof.rgb.brightness;
                                let _ = dev.render_custom_frame(|_i, nx, ny, _id| {
                                    amb.sample_uv(nx, ny).scale(br)
                                });
                            }
                        }
                        EffectKind::ReactiveTyping => {
                            let bg = prof.rgb.secondary.scale(0.14);
                            let fg = prof.rgb.primary;
                            let br = prof.rgb.brightness;
                            let spd = prof.rgb.speed;
                            let _ = dev.render_custom_frame(|_i, nx, ny, _id| {
                                let mut intensity = 0.0f32;
                                for &(cx, cy, age) in &reactive_centers {
                                    let dx = nx - cx;
                                    let dy = (ny - cy) * 0.45;
                                    let dist = (dx * dx + dy * dy).sqrt();
                                    let radius = age * 0.95 * spd;
                                    let ring = (1.0 - (dist - radius).abs() * 9.0).clamp(0.0, 1.0);
                                    let fade = (1.0 - age / 1.1).clamp(0.0, 1.0);
                                    intensity = intensity.max(ring * fade);
                                }
                                bg.lerp(fg, intensity).scale(br)
                            });
                        }
                        EffectKind::AudioVisualizer => {
                            let lvl = self.audio_smooth_peak;
                            let c1 = prof.rgb.primary;
                            let c2 = prof.rgb.secondary;
                            let br = prof.rgb.brightness;
                            let _ = dev.render_custom_frame(|_i, nx, ny, _id| {
                                let dist = (nx - 0.5).abs() * 2.0;
                                let bar = ((lvl - dist) * 5.0).clamp(0.0, 1.0);
                                let bg = c2.scale(0.08);
                                let peak_col = c1.lerp(c2, dist * 0.7 + (1.0 - ny) * 0.3);
                                bg.lerp(peak_col, bar).scale(br)
                            });
                        }
                        _ => {
                            let _ = dev.render_effect(&prof.rgb, t);
                        }
                    }
                }
            }
        }
        if prof.sync_asus_aura {
            if let Some(aura) = self.asus_aura.as_mut() {
                let counts: Vec<usize> = aura.info.channels.iter().map(|c| c.led_count).collect();
                for (ch_idx, count) in counts.into_iter().enumerate() {
                    let leds = sample_strip_rgb(
                        &prof.rgb,
                        count,
                        t,
                        self.last_ambilight.as_ref(),
                        &reactive_centers,
                        self.audio_smooth_peak,
                    );
                    let _ = aura.send_channel_direct(ch_idx, &leds);
                }
            }
        } else if prof.silent_loop3_thermal_rgb {
            let sensors = self.latest_sensors();
            if let (Some(ch_idx), Some(aura)) =
                (prof.silent_loop3_argb_channel, self.asus_aura.as_mut())
                && let Some(ch) = aura.info.channels.get(ch_idx)
            {
                let temp = sensors
                    .cpu_temp_c
                    .or(self.last_tel.gpu.edge_temp_c)
                    .or(self.last_tel.gpu.hotspot_temp_c)
                    .unwrap_or(35.0 + self.last_tel.cpu_usage_pct * 0.5);
                let col = thermal_rgb(temp);
                let leds = vec![col; ch.led_count];
                let _ = aura.send_channel_direct(ch_idx, &leds);
            }
        }

        if omen_sync && self.omen_last_rgb_push.elapsed() >= Duration::from_millis(120) {
            let strip = sample_strip_rgb(
                &prof.rgb,
                4,
                t,
                self.last_ambilight.as_ref(),
                &reactive_centers,
                self.audio_smooth_peak,
            );
            let zones = [strip[0], strip[1], strip[2], strip[3]];
            if let Ok(tbl) = set_hp_omen_kb_zones(zones, self.omen_color_table.as_deref()) {
                self.omen_color_table = Some(tbl);
                self.hp_omen.zone_colors = zones;
            }
            self.omen_last_rgb_push = Instant::now();
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
        if vendors.contains(&0x1B1C) {
            self.corsair_devices.clear();
            self.corsair_devices = CorsairDevice::connect_all();
        }
        if vendors.contains(&0x046D) {
            self.logi_mice.clear();
            self.logi_keyboards.clear();
            self.logi_headsets.clear();
            self.logi_mice = LogitechMouse::connect_all();
            self.logi_keyboards = LogitechKeyboard::connect_all();
            self.logi_headsets = LogitechHeadset::connect_all();
        }
        if vendors.contains(&0x0B05) {
            self.asus_aura = None;
            match AuraMainboardController::connect() {
                Ok(a) => {
                    self.asus_aura = Some(a);
                    self.asus_err = None;
                }
                Err(e) => self.asus_err = Some(e.to_string()),
            }
        }
        self.rediscover_drivers_for(vendors);
        self.scanned_devices = scan_hid_devices(false).unwrap_or_default();
    }

    pub(crate) fn reconnect_all(&mut self) {
        self.corsair_devices.clear();
        self.asus_aura = None;
        self.logi_mice.clear();
        self.logi_keyboards.clear();
        self.logi_headsets.clear();

        self.corsair_devices = CorsairDevice::connect_all();
        match AuraMainboardController::connect() {
            Ok(a) => {
                self.asus_aura = Some(a);
                self.asus_err = None;
            }
            Err(e) => self.asus_err = Some(e.to_string()),
        }
        self.logi_mice = LogitechMouse::connect_all();
        self.logi_keyboards = LogitechKeyboard::connect_all();
        self.logi_headsets = LogitechHeadset::connect_all();
        self.driver_devices = super::devices::discover_all(&self.drivers);

        self.cooling_status = probe_asus_wmi_cooling();
        self.power_plans = list_power_plans();
        self.scanned_devices = scan_hid_devices(false).unwrap_or_default();
        self.last_hid_sig = compute_hid_signature();
    }

    pub(crate) fn profile_needs_input_engine(&mut self) -> bool {
        let p = self.config.active_profile_mut();
        p.socd.enabled
            || p.key_chatter_ms > 0
            || p.mouse_debounce_ms > 0
            || p.win_lock
            || !p.bindings.is_empty()
            || p.rgb.kind == EffectKind::ReactiveTyping
    }

    pub(crate) fn reapply_runtime_state(&mut self) {
        let (sync, thermal, counts) = {
            let p = self.config.active_profile_mut();
            (
                p.sync_asus_aura,
                p.silent_loop3_thermal_rgb,
                p.asus_argb_led_counts.clone(),
            )
        };
        let blackout = self.blackout_lock || self.blackout_idle;
        if let Some(aura) = self.asus_aura.as_mut() {
            for (ch, n) in counts {
                aura.set_channel_led_count(ch, n);
            }
            if sync || thermal || blackout {
                let _ = aura.enter_direct_mode();
            }
        }
        self.blackout_applied = false;
        if self.profile_needs_input_engine()
            && !self.input_engine.is_running()
            && let Err(e) = self.input_engine.start()
        {
            error!("Moteur clavier non redémarré : {e}");
        }
    }

    pub(crate) fn apply_restored_config(&mut self) {
        self.sync_input_rules();
        let need_engine = self.profile_needs_input_engine();
        if need_engine && !self.input_engine.is_running() {
            let _ = self.input_engine.start();
        } else if !need_engine && self.input_engine.is_running() {
            self.input_engine.stop();
        }

        let (enable_rgb, sync_aura, thermal, counts) = {
            let p = self.config.active_profile_mut();
            (
                p.enable_rgb,
                p.sync_asus_aura,
                p.silent_loop3_thermal_rgb,
                p.asus_argb_led_counts.clone(),
            )
        };
        if !enable_rgb {
            for dev in &mut self.corsair_devices {
                if dev.info.kind == CorsairDeviceKind::Keyboard
                    && dev.info.mode == OperatingMode::Software
                {
                    let _ = dev.set_mode(OperatingMode::Hardware);
                }
            }
        }
        if let Some(aura) = self.asus_aura.as_mut() {
            for (ch, n) in counts {
                aura.set_channel_led_count(ch, n);
            }
            if sync_aura || thermal {
                let _ = aura.enter_direct_mode();
            } else if aura.is_in_direct_mode() {
                let _ = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false);
            }
        }
        self.persist_config();
    }

    pub(crate) fn push_blackout_frame(&mut self) {
        for dev in &mut self.corsair_devices {
            if dev.info.kind == CorsairDeviceKind::Keyboard {
                let _ = dev.render_custom_frame(|_, _, _, _| Rgb::BLACK);
            }
        }
        if let Some(aura) = self.asus_aura.as_mut() {
            let _ = aura.enter_direct_mode();
            let counts: Vec<usize> = aura
                .info
                .channels
                .iter()
                .map(|c| match c.kind {
                    AuraChannelKind::AddressableGen2 => c.led_count.max(ARGB_MAX_LEDS),
                    _ => c.led_count,
                })
                .collect();
            for (ch, n) in counts.into_iter().enumerate() {
                let _ = aura.send_channel_direct(ch, &vec![Rgb::BLACK; n]);
            }
        }
        if !self.blackout_applied {
            if self.hp_omen.wmi_accessible
                && self.hp_omen.keyboard_type.is_software_rgb()
                && self.hp_omen.backlight_on
            {
                self.omen_kb_restore_backlight = Some(true);
                let _ = set_hp_omen_kb_backlight(false);
            }
            self.ram_blackout();
            info!("Éclairage coupé (session verrouillée ou inactivité)");
        }
        self.blackout_applied = true;
    }

    fn ram_blackout(&mut self) {
        if self.ram_restore_modes.is_some() || self.ram_rgb.ene_modules.is_empty() || !is_elevated()
        {
            return;
        }
        let restore: Vec<(u8, u8)> = self
            .ram_rgb
            .ene_modules
            .iter()
            .map(|m| (m.address_7bit, m.current_mode))
            .filter(|&(_, mode)| mode != EneRamMode::Off as u8)
            .collect();
        if restore.is_empty() {
            return;
        }
        let targets: Vec<(&opengear_mainboard::EneRamModule, u8)> = self
            .ram_rgb
            .ene_modules
            .iter()
            .filter(|m| restore.iter().any(|(a, _)| *a == m.address_7bit))
            .map(|m| (m, EneRamMode::Off as u8))
            .collect();
        match crate::drivers::ram_rgb().set_modes(&targets) {
            Ok(n) if n > 0 => self.ram_restore_modes = Some(restore),
            Ok(_) => {}
            Err(e) => error!("Extinction RAM impossible : {e}"),
        }
    }

    pub(crate) fn ram_restore(&mut self) {
        let Some(restore) = self.ram_restore_modes.take() else {
            return;
        };
        let targets: Vec<(&opengear_mainboard::EneRamModule, u8)> = restore
            .iter()
            .filter_map(|(addr, mode)| {
                self.ram_rgb
                    .ene_modules
                    .iter()
                    .find(|m| m.address_7bit == *addr)
                    .map(|m| (m, *mode))
            })
            .collect();
        if let Err(e) = crate::drivers::ram_rgb().set_modes(&targets) {
            error!("Rallumage RAM impossible : {e}");
        }
    }

    fn end_blackout(&mut self) {
        let (enable_rgb, sync, thermal) = {
            let p = self.config.active_profile_mut();
            (p.enable_rgb, p.sync_asus_aura, p.silent_loop3_thermal_rgb)
        };
        if !enable_rgb {
            for dev in &mut self.corsair_devices {
                if dev.info.kind == CorsairDeviceKind::Keyboard
                    && dev.info.mode == OperatingMode::Software
                {
                    let _ = dev.set_mode(OperatingMode::Hardware);
                }
            }
        }
        if !(sync || thermal)
            && let Some(aura) = self.asus_aura.as_mut()
        {
            let _ = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false);
        }
        self.ram_restore();
        if self.omen_kb_restore_backlight.take() == Some(true)
            && self.hp_omen.wmi_accessible
            && self.hp_omen.keyboard_type.is_software_rgb()
        {
            let _ = set_hp_omen_kb_backlight(true);
        }
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
            info!("Passage sur batterie — activation du profil autonomie (60 Hz + Cool)");
            self.ac_saved_hz = self.laptop_display.current_hz;
            self.ac_saved_thermal = Some(self.hp_omen.thermal_mode);
            self.ac_saved_kb_backlight = if self.hp_omen.keyboard_type.is_software_rgb() {
                Some(self.hp_omen.backlight_on)
            } else {
                None
            };
            if self.laptop_display.supported_hz.contains(&60)
                && self.laptop_display.current_hz != Some(60)
                && set_display_refresh_rate(60).is_ok()
            {
                self.laptop_display = read_display_info();
            }
            if self.hp_omen.wmi_accessible {
                if set_hp_omen_thermal_mode(self.hp_omen.thermal_version, OmenThermalMode::Cool)
                    .is_ok()
                {
                    self.hp_omen.thermal_mode = OmenThermalMode::Cool;
                }
                if self.hp_omen.keyboard_type.is_software_rgb()
                    && self.hp_omen.backlight_on
                    && set_hp_omen_kb_backlight(false).is_ok()
                {
                    self.hp_omen.backlight_on = false;
                }
            }
        } else {
            info!("Passage sur secteur — restauration performances & fréquence d'écran");
            let restore_hz = self
                .ac_saved_hz
                .take()
                .or_else(|| self.laptop_display.supported_hz.iter().copied().max());
            if let Some(hz) = restore_hz
                && self.laptop_display.current_hz != Some(hz)
                && set_display_refresh_rate(hz).is_ok()
            {
                self.laptop_display = read_display_info();
            }
            if self.hp_omen.wmi_accessible {
                if let Some(mode) = self.ac_saved_thermal.take()
                    && set_hp_omen_thermal_mode(self.hp_omen.thermal_version, mode).is_ok()
                {
                    self.hp_omen.thermal_mode = mode;
                }
                if self.hp_omen.keyboard_type.is_software_rgb()
                    && self.ac_saved_kb_backlight.take() == Some(true)
                    && set_hp_omen_kb_backlight(true).is_ok()
                {
                    self.hp_omen.backlight_on = true;
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
        let mut readings: Vec<(String, String, u8, bool)> = Vec::new();
        for m in &self.logi_mice {
            if let Some(b) = &m.info.battery
                && let Some(pct) = b.percent
            {
                readings.push((
                    format!("mouse-{:04X}-{}", m.info.pid, m.info.device_index),
                    m.info.name.clone(),
                    pct,
                    battery_is_charging(b.state),
                ));
            }
        }
        for k in &self.logi_keyboards {
            if let Some(b) = &k.info.battery
                && let Some(pct) = b.percent
            {
                readings.push((
                    format!("kb-{:04X}-{}", k.info.pid, k.info.device_index),
                    k.info.name.clone(),
                    pct,
                    battery_is_charging(b.state),
                ));
            }
        }
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

pub(crate) fn sample_strip_rgb(
    cfg: &opengear_core::RgbEffectConfig,
    count: usize,
    t: f32,
    amb: Option<&AmbilightFrame>,
    reactive_centers: &[(f32, f32, f32)],
    audio_peak: f32,
) -> Vec<Rgb> {
    (0..count)
        .map(|i| {
            let nx = if count <= 1 {
                0.5
            } else {
                i as f32 / (count - 1) as f32
            };
            match cfg.kind {
                EffectKind::Ambilight => amb
                    .map(|a| a.sample_uv(nx, 0.5).scale(cfg.brightness))
                    .unwrap_or(cfg.primary),
                EffectKind::ReactiveTyping => {
                    let pulse = reactive_centers
                        .iter()
                        .map(|&(_, _, age)| (1.0 - age / 1.1).clamp(0.0, 1.0))
                        .fold(0.0f32, f32::max);
                    cfg.secondary
                        .scale(0.18)
                        .lerp(cfg.primary, pulse)
                        .scale(cfg.brightness)
                }
                EffectKind::AudioVisualizer => {
                    let dist = (nx - 0.5).abs() * 2.0;
                    let bar = ((audio_peak - dist) * 4.5).clamp(0.0, 1.0);
                    cfg.secondary
                        .scale(0.10)
                        .lerp(cfg.primary.lerp(cfg.secondary, dist), bar)
                        .scale(cfg.brightness)
                }
                _ => cfg.sample(i, nx, 0.5, t),
            }
        })
        .collect()
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
