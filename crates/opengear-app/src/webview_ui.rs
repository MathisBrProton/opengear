use crate::audio_ext::{read_audio_state, read_output_peak, set_endpoint_mute, set_endpoint_volume};
use crate::local_assets::LocalAssetCatalog;
use crate::selftest::{SelfTestReport, run_full_selftest};
use crate::system_ext::{
    AmbilightFrame, get_foreground_exe_name, idle_duration, list_visible_apps,
    sample_ambilight_screen,
};
use crate::system_events;
use crate::tray_icon::{
    TrayAction, acquire_single_instance, build_window_icon, create_windows_shortcuts,
    ensure_ico_beside_exe, is_autostart_enabled, release_single_instance, set_autostart_enabled,
    start_system_tray, stop_system_tray, tray_notify, trim_working_set,
};
use anyhow::Result;
use opengear_asus::{
    AsusWmiCoolingStatus, AuraChannelKind, AuraHwMode, AuraMainboardController, CurvePoint,
    EneRamMode, FanChannelConfig, FanHeaderKind, FanPolicyMode, FanTempSource,
    HardwareSensorsSnapshot, PowerPlan, RamRgbStatus, SuperIoSnapshot, SystemTelemetry,
    PawnIoAvailability, TelemetryCollector, apply_asus_wmi_curve, apply_ram_rgb,
    install_official_pawnio, is_elevated, list_power_plans, pawnio_availability,
    probe_asus_wmi_cooling, probe_ram_rgb, read_asus_wmi_channel,
    read_hardware_sensors, read_superio_snapshot, relaunch_as_admin, restore_all_superio_fans_auto,
    restore_superio_fan_auto, saved_superio_states, set_active_power_plan, set_superio_fan_pwm,
};
use crate::fan_watchdog;
use opengear_core::{
    AppConfig, DetectedDevice, EffectKind, KeyAction, KeyboardProfile, MacroStep, OperatingMode,
    PollingRate, Rgb, SocdPair, scan_hid_devices,
};
use opengear_corsair::{
    CorsairDevice, CorsairDeviceKind, HwpLightingKind, K70_TKL_KEYS,
    proto::{
        PROP_BRIGHTNESS_INDICATOR_COLOR, PROP_ESPORTS_BACKLIGHT_COLOR, PROP_PROFILE_INDICATOR_COLOR,
        PROP_WINLOCK_OFF_COLOR, PROP_WINLOCK_ON_COLOR,
    },
};
use opengear_input::{ALL_BINDABLE_KEYS, InputEngine};
use opengear_logitech::{
    BatteryPowerModel, BatteryState, EQ_PRESETS, KeyboardRgbMode, LodLevel, LogitechHeadset, LogitechKeyboard,
    LogitechMouse, MouseButtonAction, MouseLedMode, OnboardMacroStep, OnboardMode,
    backup_path_for, list_sector_backups,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use tracing::{error, info};
use wry::{WebViewBuilder, http};

const INDEX_HTML: &[u8] = include_bytes!("../ui/index.html");
const APP_CSS: &[u8] = include_bytes!("../ui/app.css");
const APP_JS: &[u8] = include_bytes!("../ui/app.js");
const IMG_MOUSE: &[u8] = include_bytes!("../assets/mouse_superlight.jpg");
const IMG_MOUSE_WHITE: &[u8] = include_bytes!("../assets/mouse_superlight_white.jpg");
const IMG_KEYBOARD: &[u8] = include_bytes!("../assets/keyboard_k70_tkl.jpg");
const IMG_MOTHERBOARD: &[u8] = include_bytes!("../assets/motherboard_b650a.jpg");
const IMG_HEADSET: &[u8] = include_bytes!("../assets/headset_prox.jpg");
const IMG_AIO: &[u8] = include_bytes!("../assets/aio_silentloop3.jpg");

enum UserEvent {
    EvalJs(String),
    Tray(TrayAction),
    CloseApp,
}

/// Temperature assumed by Smart Cooling when its sensor is unavailable: high on purpose, so a
/// missing GPU/CPU reading ramps the fans up instead of silently under-cooling.
const SMART_FAN_FAILSAFE_TEMP_C: f32 = 80.0;

static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(true);
static UI_VISIBLE: AtomicBool = AtomicBool::new(true);
static THERMAL_RGB_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize)]
struct IpcRequest {
    id: u64,
    cmd: String,
    #[serde(default)]
    payload: Value,
}

#[cfg(windows)]
fn apply_dark_titlebar(window: &tao::window::Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    let hwnd = window.hwnd() as windows_sys::Win32::Foundation::HWND;
    let dark: i32 = 1;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            20,
            &dark as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(not(windows))]
fn apply_dark_titlebar(_window: &tao::window::Window) {}

fn build_webview(
    window: &tao::window::Window,
    proto_assets: Arc<LocalAssetCatalog>,
    ipc_tx: mpsc::Sender<IpcRequest>,
) -> Result<wry::WebView> {
    WebViewBuilder::new()
        .with_custom_protocol("opengear".into(), move |_webview_id, req| {
            let path = req.uri().path();
            if let Some(key) = path.strip_prefix("/local-asset/") {
                if let Some((bytes, mime)) = proto_assets.get_blob(key) {
                    return http::Response::builder()
                        .header(http::header::CONTENT_TYPE, mime)
                        .body(std::borrow::Cow::Owned(bytes.to_vec()))
                        .unwrap();
                }
            }
            let (body, mime): (&[u8], &str) = match path {
                "/" | "/index.html" => (INDEX_HTML, "text/html; charset=utf-8"),
                "/app.css" => (APP_CSS, "text/css; charset=utf-8"),
                "/app.js" => (APP_JS, "application/javascript; charset=utf-8"),
                "/assets/mouse_superlight.jpg" => (IMG_MOUSE, "image/jpeg"),
                "/assets/mouse_superlight_white.jpg" => (IMG_MOUSE_WHITE, "image/jpeg"),
                "/assets/keyboard_k70_tkl.jpg" => (IMG_KEYBOARD, "image/jpeg"),
                "/assets/motherboard_b650a.jpg" => (IMG_MOTHERBOARD, "image/jpeg"),
                "/assets/headset_prox.jpg" => (IMG_HEADSET, "image/jpeg"),
                "/assets/aio_silentloop3.jpg" => (IMG_AIO, "image/jpeg"),
                _ => (b"Not found", "text/plain"),
            };
            http::Response::builder()
                .header(http::header::CONTENT_TYPE, mime)
                .body(std::borrow::Cow::Borrowed(body))
                .unwrap()
        })
        .with_ipc_handler(move |req| {
            if let Ok(msg) = serde_json::from_str::<IpcRequest>(req.body()) {
                let _ = ipc_tx.send(msg);
            }
        })
        .with_url("http://opengear.localhost/")
        .build(window)
        .map_err(|e| anyhow::anyhow!("Impossible d'initialiser WebView2 : {e}"))
}

pub fn run_gui(cli_minimized: bool) -> Result<()> {
    // Single-instance guard: if OpenGear is already running in the Tray, wake it up and exit cleanly!
    if !acquire_single_instance() {
        info!("OpenGear est déjà en cours d'exécution — réveil de la fenêtre existante.");
        return Ok(());
    }

    // We hold the single-instance lock: any fan override marker left behind belongs to a dead
    // session (crash / forced kill) and must be handed back to the BIOS now.
    fan_watchdog::recover_stale_marker();

    let _ = ensure_ico_beside_exe();
    let initial_cfg = AppConfig::load(&AppConfig::default_config_path()).unwrap_or_default();
    MINIMIZE_TO_TRAY.store(initial_cfg.minimize_to_tray, Ordering::Relaxed);
    let start_hidden = cli_minimized || initial_cfg.start_minimized;
    UI_VISIBLE.store(!start_hidden, Ordering::SeqCst);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let mut wb = WindowBuilder::new()
        .with_title("OpenGear — Command Center")
        .with_inner_size(LogicalSize::new(1400.0, 880.0))
        .with_min_inner_size(LogicalSize::new(1120.0, 720.0))
        .with_visible(!start_hidden);
    if let Some(icon) = build_window_icon() {
        wb = wb.with_window_icon(Some(icon));
    }
    let window = wb
        .build(&event_loop)
        .map_err(|e| anyhow::anyhow!("Impossible de créer la fenêtre : {e}"))?;

    apply_dark_titlebar(&window);
    start_system_tray(proxy.clone(), UserEvent::Tray);

    // Discover local official vendor assets (G HUB depot + iCUE5 .rcc) at runtime without bundling them
    let local_assets = Arc::new(LocalAssetCatalog::discover());

    // Ultra-light adaptive hardware sensors watcher (LibreHardwareMonitor / OpenHardwareMonitor WMI):
    // - ZERO PowerShell spawns when hidden in System Tray (unless Thermal RGB needs live CPU temp)
    // - Automatic 60s backoff when LHM/OHM is not installed (instead of polling every 3s!)
    let shared_sensors = Arc::new(Mutex::new(HardwareSensorsSnapshot::default()));
    let sensors_bg = Arc::clone(&shared_sensors);
    std::thread::Builder::new()
        .name("opengear-sensors".into())
        .spawn(move || {
            let mut last_probe: Option<Instant> = None;
            let mut last_available = false;
            loop {
                let needed = UI_VISIBLE.load(Ordering::Relaxed)
                    || THERMAL_RGB_ACTIVE.load(Ordering::Relaxed);
                let interval = if last_available {
                    Duration::from_secs(4)
                } else {
                    Duration::from_secs(60)
                };
                if needed && last_probe.map(|t| t.elapsed() >= interval).unwrap_or(true) {
                    let snap = read_hardware_sensors();
                    last_available = snap.available;
                    last_probe = Some(Instant::now());
                    if let Ok(mut guard) = sensors_bg.lock() {
                        *guard = snap;
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .ok();

    let (tx_cmd, rx_cmd) = mpsc::channel::<IpcRequest>();
    // The backend thread owns every device handle: on exit it must restore hardware itself
    // (process exit would kill it without running any Drop), then acknowledge on this channel.
    let (shutdown_ack_tx, shutdown_ack_rx) = mpsc::channel::<()>();
    let worker_proxy = proxy.clone();
    let engine_assets = Arc::clone(&local_assets);
    let engine_sensors = Arc::clone(&shared_sensors);
    std::thread::Builder::new()
        .name("opengear-backend".into())
        .spawn(move || {
            let mut engine = BackendEngine::new(engine_assets, engine_sensors);
            engine.run_loop(rx_cmd, worker_proxy, shutdown_ack_tx);
        })
        .ok();

    // Ultra-Light Tray Mode: only instantiate WebView2 when the window is actually visible!
    let mut webview: Option<wry::WebView> = if start_hidden {
        trim_working_set();
        info!("OpenGear démarré en Mode Tray Ultra-Light (0 processus WebView2).");
        None
    } else {
        let wv = build_webview(&window, Arc::clone(&local_assets), tx_cmd.clone())?;
        info!("OpenGear WebView2 natif démarré.");
        Some(wv)
    };

    // Sleep / resume, session lock and device plug events -> backend (as internal commands).
    let sys_tx = tx_cmd.clone();
    system_events::start(move |ev| {
        let _ = sys_tx.send(IpcRequest {
            id: 0,
            cmd: "__sys_event".into(),
            payload: json!({ "event": ev.id() }),
        });
    });

    let tray_tx = tx_cmd.clone();
    let ui_assets = Arc::clone(&local_assets);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::EvalJs(script)) => {
                if let Some(wv) = webview.as_ref() {
                    if let Err(e) = wv.evaluate_script(&script) {
                        error!("evaluate_script error: {e}");
                    }
                }
            }
            Event::UserEvent(UserEvent::Tray(action)) => match action {
                TrayAction::ShowWindow => {
                    window.set_visible(true);
                    window.set_minimized(false);
                    window.set_focus();
                    if webview.is_none() {
                        match build_webview(&window, Arc::clone(&ui_assets), tray_tx.clone()) {
                            Ok(wv) => {
                                webview = Some(wv);
                                info!("WebView2 recréée à la volée depuis le System Tray.");
                            }
                            Err(e) => error!("Erreur recréation WebView2 : {e}"),
                        }
                    }
                    UI_VISIBLE.store(true, Ordering::SeqCst);
                }
                TrayAction::ToggleSocd => {
                    let _ = tray_tx.send(IpcRequest {
                        id: 0,
                        cmd: "tray_toggle_socd".into(),
                        payload: Value::Null,
                    });
                }
                TrayAction::ToggleSyncRgb => {
                    let _ = tray_tx.send(IpcRequest {
                        id: 0,
                        cmd: "tray_toggle_sync".into(),
                        payload: Value::Null,
                    });
                }
                TrayAction::Quit => {
                    UI_VISIBLE.store(false, Ordering::SeqCst);
                    webview = None;
                    shutdown_backend(&tray_tx, &shutdown_ack_rx);
                    stop_system_tray();
                    release_single_instance();
                    *control_flow = ControlFlow::Exit;
                }
            },
            Event::UserEvent(UserEvent::CloseApp) => {
                UI_VISIBLE.store(false, Ordering::SeqCst);
                webview = None;
                shutdown_backend(&tray_tx, &shutdown_ack_rx);
                stop_system_tray();
                release_single_instance();
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if MINIMIZE_TO_TRAY.load(Ordering::Relaxed) {
                    // ULTRA-LIGHT TRAY MODE: destroy WebView2 completely (frees ~440 MB RAM & 6 Chromium processes!)
                    UI_VISIBLE.store(false, Ordering::SeqCst);
                    webview = None;
                    window.set_visible(false);
                    trim_working_set();
                    info!("OpenGear réduit en Mode Tray Ultra-Light (WebView2 libérée).");
                } else {
                    UI_VISIBLE.store(false, Ordering::SeqCst);
                    webview = None;
                    shutdown_backend(&tray_tx, &shutdown_ack_rx);
                    stop_system_tray();
                    release_single_instance();
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

/// Internal IPC command asking the backend thread to restore all hardware and stop.
const SHUTDOWN_CMD: &str = "__shutdown";

/// Ask the backend to put every device back under firmware/BIOS control, and wait (bounded)
/// for it to finish before the process exits.
fn shutdown_backend(tx: &mpsc::Sender<IpcRequest>, ack_rx: &mpsc::Receiver<()>) {
    let sent = tx
        .send(IpcRequest {
            id: 0,
            cmd: SHUTDOWN_CMD.into(),
            payload: Value::Null,
        })
        .is_ok();
    if sent && ack_rx.recv_timeout(Duration::from_secs(5)).is_err() {
        error!("Restauration matérielle non confirmée avant la fermeture (délai dépassé).");
    }
}

fn compute_hid_signature() -> Vec<(u16, u16, u16)> {
    let mut sig: Vec<(u16, u16, u16)> = scan_hid_devices(false)
        .unwrap_or_default()
        .into_iter()
        .filter(|d| matches!(d.vid, 0x1B1C | 0x046D | 0x0B05))
        .map(|d| (d.vid, d.pid, d.usage_page))
        .collect();
    sig.sort_unstable();
    sig.dedup();
    sig
}

struct BackendEngine {
    config_path: PathBuf,
    config: AppConfig,
    /// Set when the profile file could not be loaded (it was moved aside): shown in the UI.
    config_warning: Option<String>,
    local_assets: Arc<LocalAssetCatalog>,
    shared_sensors: Arc<Mutex<HardwareSensorsSnapshot>>,
    // Universal multi-device collections (Plug & Play N simultaneous devices)
    corsair_devices: Vec<CorsairDevice>,
    asus_aura: Option<AuraMainboardController>,
    asus_err: Option<String>,
    logi_mice: Vec<LogitechMouse>,
    logi_keyboards: Vec<LogitechKeyboard>,
    logi_headsets: Vec<LogitechHeadset>,
    last_hid_sig: Vec<(u16, u16, u16)>,
    telemetry: TelemetryCollector,
    last_tel: SystemTelemetry,
    power_plans: Vec<PowerPlan>,
    cooling_status: AsusWmiCoolingStatus,
    cooling_channels: Vec<FanChannelConfig>,
    /// BIOS fan configuration read at startup (None if it could not be read): restored on exit
    /// for every channel Smart Cooling has overridden with a flat manual curve.
    initial_cooling_channels: Vec<Option<FanChannelConfig>>,
    ram_rgb: RamRgbStatus,
    input_engine: InputEngine,
    scanned_devices: Vec<DetectedDevice>,
    aura_hw_mode: AuraHwMode,
    aura_hw_color: Rgb,
    aura_shutdown_mode: AuraHwMode,
    aura_shutdown_color: Rgb,
    start_time: Instant,
    foreground_app: Option<String>,
    manual_profile_name: String,
    auto_switched: bool,
    last_ambilight: Option<AmbilightFrame>,
    audio_smooth_peak: f32,
    smart_fan_last_duty: [Option<u8>; 5],
    smart_fan_ref_temp: [f32; 5],
    superio: SuperIoSnapshot,
    last_selftest: Option<SelfTestReport>,
    /// Config snapshots taken before each undoable change (label, config), newest last.
    undo_stack: Vec<(String, AppConfig)>,
    /// `list_visible_apps()` enumerates every window: cached for a few seconds because the full
    /// state is rebuilt after every UI action.
    visible_apps_cache: Option<(Instant, Vec<String>)>,
    /// The fan watchdog process has been started for this session.
    watchdog_spawned: bool,
    /// System is going to sleep: no device I/O until resume.
    suspended: bool,
    /// Lights forced off because the session is locked / the user is inactive.
    blackout_lock: bool,
    blackout_idle: bool,
    /// A black frame has been pushed to the devices (so lights must be restored afterwards).
    blackout_applied: bool,
    /// Last low-battery level notified per device (15 or 5), reset when charging.
    battery_alerted: std::collections::HashMap<String, u8>,
}

/// Low-battery notification state machine. Returns `(new_last_alerted, level_to_notify)`:
/// notify once at <= 15 %, once more at <= 5 %, and re-arm when charging or back above 20 %.
pub(crate) fn battery_alert_step(last: Option<u8>, pct: u8, charging: bool) -> (Option<u8>, Option<u8>) {
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

fn battery_is_charging(state: BatteryState) -> bool {
    matches!(
        state,
        BatteryState::Charging | BatteryState::SlowCharging | BatteryState::AlmostFull | BatteryState::Full
    )
}

/// Maximum number of undo steps kept in memory.
const UNDO_DEPTH: usize = 20;

/// Commands whose effect is a configuration change that `undo_last` can revert, with the label
/// shown in the UI. Hardware-only writes (DPI, flash, EEPROM…) are not in this list.
fn undo_label_for(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "set_rgb_config" => "Réglage de l'éclairage",
        "set_per_key_color" => "Couleur de touche",
        "clear_per_key" => "Effacement des couleurs par touche",
        "set_sync_all" | "tray_toggle_sync" => "Synchronisation de l'éclairage",
        "switch_profile" => "Changement de profil",
        "create_profile" => "Création de profil",
        "set_azerty" => "Disposition du clavier",
        "input_set_winlock" => "Verrou de la touche Windows",
        "input_set_binding" => "Assignation de touche",
        "input_remove_binding" => "Suppression d'assignation",
        "input_set_esports" | "tray_toggle_socd" => "Réglages e-sport",
        "profile_set_linked_apps" => "Applications liées au profil",
        "profile_import_json" => "Import de profil",
        "apply_preset" => "Préréglage",
        "asus_set_led_count" => "Nombre de LED",
        "cooling_sl3_argb" => "Éclairage du waterblock",
        _ => return None,
    })
}

/// One-click presets shown on the home screen.
const PRESETS: &[(&str, &str, &str)] = &[
    (
        "silent",
        "Silencieux",
        "Ventilateurs en profil silencieux, éclairage doux et tamisé, plan d'alimentation équilibré.",
    ),
    (
        "gaming",
        "Jeu",
        "Ventilateurs en profil turbo, éclairage arc-en-ciel, plan d'alimentation hautes performances.",
    ),
    (
        "office",
        "Bureau",
        "Ventilateurs en profil standard, éclairage blanc statique, plan d'alimentation équilibré.",
    ),
    (
        "lights_off",
        "Tout éteint",
        "Coupe l'éclairage de tous les appareils sans toucher aux ventilateurs.",
    ),
];

const POWER_PLAN_BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
const POWER_PLAN_HIGH_PERF: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const POWER_PLAN_ULTIMATE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

impl BackendEngine {
    fn new(
        local_assets: Arc<LocalAssetCatalog>,
        shared_sensors: Arc<Mutex<HardwareSensorsSnapshot>>,
    ) -> Self {
        let config_path = AppConfig::default_config_path();
        // A corrupt file is moved aside (never overwritten by the next save).
        let (mut config, config_warning) = AppConfig::load_or_recover(&config_path);
        let scanned_devices = scan_hid_devices(false).unwrap_or_default();
        let last_hid_sig = compute_hid_signature();
        let power_plans = list_power_plans();

        let corsair_devices = CorsairDevice::connect_all();
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
        let cooling_status = probe_asus_wmi_cooling();
        let ram_rgb = probe_ram_rgb();
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
        let cooling_channels: Vec<FanChannelConfig> = FanHeaderKind::ALL
            .iter()
            .zip(&initial_cooling_channels)
            .map(|(&h, read)| read.clone().unwrap_or_else(|| FanChannelConfig::default_for(h)))
            .collect();

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
            superio: read_superio_snapshot(),
            last_selftest: None,
            undo_stack: Vec::new(),
            visible_apps_cache: None,
            watchdog_spawned: false,
            suspended: false,
            blackout_lock: false,
            blackout_idle: false,
            blackout_applied: false,
            battery_alerted: std::collections::HashMap::new(),
        };
        eng.manual_profile_name = eng.config.active_profile.clone();
        eng.sync_input_rules();
        // Auto-start low-latency input engine if user saved active bindings / SOCD / chatter / mouse debounce
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

    /// Save the profiles, logging (instead of silently dropping) any failure.
    fn persist_config(&mut self) {
        if let Err(e) = self.config.save(&self.config_path) {
            error!("Sauvegarde des profils impossible : {e:#}");
        }
    }

    fn sync_input_rules(&mut self) {
        let prof = self.config.active_profile_mut().clone();
        self.input_engine.update_rules(
            prof.win_lock,
            &prof.bindings,
            &prof.socd,
            prof.key_chatter_ms,
            prof.mouse_debounce_ms,
        );
    }

    fn latest_sensors(&self) -> HardwareSensorsSnapshot {
        self.shared_sensors
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Put every device back under firmware / BIOS control before the process exits.
    ///
    /// Runs on the backend thread (which owns all handles): Corsair keyboards back to Hardware
    /// mode, Aura out of Direct streaming, Smart Cooling flat curves replaced by the original BIOS
    /// configuration, and SuperIO channels returned to their saved BIOS mode.
    /// BIOS configuration to re-apply for Smart Cooling channel `idx` (the curve read at startup,
    /// else the BIOS automatic "Standard" profile).
    fn wmi_restore_config(&self, idx: usize) -> Option<FanChannelConfig> {
        let header = FanHeaderKind::ALL.get(idx).copied()?;
        Some(match self.initial_cooling_channels.get(idx).cloned().flatten() {
            Some(original) => original,
            None => {
                let mut auto = FanChannelConfig::default_for(header);
                auto.policy = FanPolicyMode::Standard;
                auto
            }
        })
    }

    /// Keep the crash-recovery marker in sync with the fans currently overridden, and make sure
    /// the watchdog process is running while any override is active.
    fn sync_fan_marker(&mut self) {
        let marker = fan_watchdog::FanOverrideMarker {
            pid: std::process::id(),
            wmi_restore: (0..self.smart_fan_last_duty.len())
                .filter(|&i| self.smart_fan_last_duty[i].is_some())
                .filter_map(|i| self.wmi_restore_config(i))
                .collect(),
            superio_restore: saved_superio_states(),
        };
        let active = !marker.is_empty();
        fan_watchdog::write_marker(&marker);
        if active && !self.watchdog_spawned {
            match fan_watchdog::spawn_watchdog() {
                Ok(()) => self.watchdog_spawned = true,
                Err(e) => error!("Watchdog ventilateurs non démarré : {e}"),
            }
        }
    }

    fn restore_hardware_on_exit(&mut self) {
        // 1. Smart Cooling: undo the flat manual curves written to the BIOS.
        for idx in 0..self.smart_fan_last_duty.len() {
            if self.smart_fan_last_duty[idx].is_none() {
                continue;
            }
            let Some(restore) = self.wmi_restore_config(idx) else {
                continue;
            };
            match apply_asus_wmi_curve(&restore) {
                Ok(_) => self.smart_fan_last_duty[idx] = None,
                Err(e) => error!("Restauration ventilateur {:?} échouée : {e}", restore.header),
            }
        }

        // 2. SuperIO direct PWM: back to the saved BIOS mode / command.
        for msg in restore_all_superio_fans_auto() {
            info!("{msg}");
        }
        // Whatever could not be restored stays in the marker: the watchdog retries after exit.
        self.sync_fan_marker();

        // 3. ASUS Aura: leave Direct streaming for the hardware effect.
        if let Some(aura) = self.asus_aura.as_mut() {
            if aura.is_in_direct_mode() {
                if let Err(e) = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false) {
                    error!("Restauration ASUS Aura échouée : {e}");
                }
            }
        }

        // 4. Corsair: dropping the devices here (on their owning thread) runs their Drop impl,
        //    which switches keyboards left in Software mode back to Hardware mode.
        for dev in &mut self.corsair_devices {
            if dev.info.kind == CorsairDeviceKind::Keyboard && dev.info.mode == OperatingMode::Software {
                let _ = dev.set_mode(OperatingMode::Hardware);
            }
        }
        self.corsair_devices.clear();
        info!("Matériel restauré (firmware / BIOS) avant fermeture.");
    }

    fn run_loop(
        &mut self,
        rx: mpsc::Receiver<IpcRequest>,
        proxy: tao::event_loop::EventLoopProxy<UserEvent>,
        shutdown_ack: mpsc::Sender<()>,
    ) {
        let mut last_tel_push = Instant::now();
        let mut last_rgb_push = Instant::now();
        let mut last_logi_poll = Instant::now();
        let mut last_hotplug_check = Instant::now();
        let mut last_fg_check = Instant::now();
        let mut last_smart_fan = Instant::now();
        let mut last_idle_check = Instant::now();
        let mut last_blackout_push = Instant::now();

        loop {
            let ui_open = UI_VISIBLE.load(Ordering::Relaxed);
            let blackout = self.blackout_lock || self.blackout_idle;
            let (live_rgb_active, thermal_rgb_active) = {
                let p = self.config.active_profile_mut();
                let lights_owned = !self.suspended && !blackout;
                (
                    lights_owned && (p.enable_rgb || p.sync_asus_aura),
                    lights_owned && p.silent_loop3_thermal_rgb,
                )
            };
            let smart_fan_active = self.cooling_status.wmi_available
                && self
                    .cooling_channels
                    .iter()
                    .any(|c| c.source != FanTempSource::CpuBios);
            THERMAL_RGB_ACTIVE.store(thermal_rgb_active || smart_fan_active, Ordering::Relaxed);

            // Adaptive wake interval: 33ms ONLY during active live RGB streaming; 250ms UI open; 500ms in Ultra-Light Tray!
            let wait_ms = if self.suspended {
                1000
            } else if live_rgb_active {
                33
            } else if ui_open {
                250
            } else {
                500
            };

            match rx.recv_timeout(Duration::from_millis(wait_ms)) {
                Ok(req) if req.cmd == SHUTDOWN_CMD => {
                    self.restore_hardware_on_exit();
                    let _ = shutdown_ack.send(());
                    break;
                }
                Ok(req) => {
                    let close = req.cmd == "cooling_relaunch_admin";
                    let res = self.handle_cmd(&req.cmd, &req.payload);
                    if req.id == 0 {
                        // Internal tray action: broadcast updated full state to UI only if visible
                        if UI_VISIBLE.load(Ordering::Relaxed) {
                            if let Ok(st) = res {
                                let _ = proxy.send_event(UserEvent::EvalJs(format!(
                                    "window.__ogHotplug && window.__ogHotplug({st});"
                                )));
                            }
                        }
                    } else {
                        let envelope = match res {
                            Ok(data) => json!({ "ok": true, "data": data }),
                            Err(err) => json!({ "ok": false, "error": err }),
                        };
                        let js = format!("window.__ogReply({}, {});", req.id, envelope);
                        let _ = proxy.send_event(UserEvent::EvalJs(js));
                        if close && envelope["ok"] == true {
                            std::thread::sleep(Duration::from_millis(120));
                            let _ = proxy.send_event(UserEvent::CloseApp);
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            // 0. Foreground app watcher every 750 ms (CS2 SOCD Anti-Kick Guard + Auto-Profile Switch)
            if last_fg_check.elapsed() >= Duration::from_millis(750) {
                let fg = get_foreground_exe_name();
                if fg != self.foreground_app {
                    self.foreground_app = fg.clone();
                    let mut state_changed = false;

                    // Automatic per-game profile switching
                    if self.config.auto_profile_switch {
                        if let Some(ref exe) = fg {
                            if exe != "opengear.exe" {
                                let matched = self
                                    .config
                                    .profiles
                                    .iter()
                                    .find(|pr| {
                                        pr.linked_apps
                                            .iter()
                                            .any(|a| a.eq_ignore_ascii_case(exe))
                                    })
                                    .map(|pr| pr.name.clone());

                                if let Some(target_prof) = matched {
                                    if self.config.active_profile != target_prof {
                                        if !self.auto_switched {
                                            self.manual_profile_name =
                                                self.config.active_profile.clone();
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
                        }
                    }

                    // Automatic SOCD anti-kick suspension (e.g., cs2.exe!)
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

            // 1. Live telemetry push: every 1s ONLY when UI is visible (or every 2.5s if Thermal RGB / Smart GPU Fan needs temp)
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

            // 1b. FanControl-style Smart Cooling (GPU / Mix max(CPU,GPU) with Hysteresis anti-yoyo)
            if smart_fan_active && last_smart_fan.elapsed() >= Duration::from_secs(3) {
                let mut marker_dirty = false;
                let sensors = self.latest_sensors();
                // Never guess a reassuring temperature: a missing sensor must make fans spin UP.
                let gpu_t: Option<f32> = self
                    .last_tel
                    .gpu
                    .edge_temp_c
                    .or(self.last_tel.gpu.hotspot_temp_c);
                let cpu_t: Option<f32> = sensors.cpu_temp_c;
                for (idx, ch) in self.cooling_channels.iter().enumerate() {
                    if ch.source == FanTempSource::CpuBios || idx >= 5 {
                        continue;
                    }
                    let measured = match ch.source {
                        FanTempSource::GpuLive => gpu_t,
                        FanTempSource::MixCpuGpu => match (cpu_t, gpu_t) {
                            (Some(c), Some(g)) => Some(c.max(g)),
                            (c, g) => c.or(g),
                        },
                        FanTempSource::CpuBios => continue,
                    };
                    let raw_t = measured.unwrap_or(SMART_FAN_FAILSAFE_TEMP_C);
                    // Hysteresis: ramp up immediately, only step down once cooled by >= hysteresis_c
                    let hyst = ch.hysteresis_c as f32;
                    let ref_t = self.smart_fan_ref_temp[idx];
                    let eff_t = if raw_t >= ref_t {
                        self.smart_fan_ref_temp[idx] = raw_t;
                        raw_t
                    } else if raw_t <= ref_t - hyst {
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

            // 2. USB/HID hot-plug. Instant path: WM_DEVICECHANGE (system_events) -> `__sys_event`.
            //    This slow poll is only a safety net in case an event was missed.
            let hotplug_interval = if ui_open {
                Duration::from_secs(30)
            } else {
                Duration::from_secs(60)
            };
            if !self.suspended && last_hotplug_check.elapsed() >= hotplug_interval {
                if self.check_hotplug() && ui_open {
                    let st = self.full_state_json();
                    let _ = proxy.send_event(UserEvent::EvalJs(format!(
                        "window.__ogHotplug && window.__ogHotplug({st});"
                    )));
                }
                last_hotplug_check = Instant::now();
            }

            // 3. Battery refresh (30 s with the UI open, 5 min in the tray) + low-battery alerts
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

            // 3b. Inactivity lights-off
            if last_idle_check.elapsed() >= Duration::from_secs(5) {
                let min = self.config.idle_lights_off_min;
                self.blackout_idle = min > 0
                    && idle_duration()
                        .map(|d| d >= Duration::from_secs(min as u64 * 60))
                        .unwrap_or(false);
                last_idle_check = Instant::now();
            }

            // 3c. Lights-off (session locked / inactivity): hold every controlled light black,
            //     then give the lights back exactly as they were.
            if !self.suspended {
                if blackout {
                    if !self.blackout_applied
                        || last_blackout_push.elapsed() >= Duration::from_secs(1)
                    {
                        self.push_blackout_frame();
                        last_blackout_push = Instant::now();
                    }
                } else if self.blackout_applied {
                    self.end_blackout();
                }
            }

            // 4. Live ~28 FPS RGB streaming ONLY when live RGB or Thermal RGB is actually enabled!
            let rgb_interval = if live_rgb_active {
                Duration::from_millis(35)
            } else {
                Duration::from_millis(500)
            };
            if (live_rgb_active || thermal_rgb_active) && last_rgb_push.elapsed() >= rgb_interval {
                let t = self.start_time.elapsed().as_secs_f32();
                let prof = self.config.active_profile_mut().clone();

                let needs_ambilight =
                    (prof.enable_rgb || prof.sync_asus_aura) && prof.rgb.kind == EffectKind::Ambilight;
                if needs_ambilight {
                    if let Some(frame) = sample_ambilight_screen(self.last_ambilight.as_ref()) {
                        self.last_ambilight = Some(frame);
                    }
                }

                let needs_audio_vis = (prof.enable_rgb || prof.sync_asus_aura)
                    && prof.rgb.kind == EffectKind::AudioVisualizer;
                if needs_audio_vis {
                    let raw = (read_output_peak() * 1.45).clamp(0.0, 1.0);
                    if raw > self.audio_smooth_peak {
                        self.audio_smooth_peak = self.audio_smooth_peak * 0.35 + raw * 0.65;
                    } else {
                        self.audio_smooth_peak *= 0.84;
                    }
                }

                let reactive_centers: Vec<(f32, f32, f32)> =
                    if (prof.enable_rgb || prof.sync_asus_aura)
                        && prof.rgb.kind == EffectKind::ReactiveTyping
                    {
                        if !self.input_engine.is_running() {
                            let _ = self.input_engine.start();
                        }
                        self.input_engine
                            .recent_key_ripples(1.1)
                            .into_iter()
                            .filter_map(|(kname, age)| {
                                K70_TKL_KEYS
                                    .iter()
                                    .find(|k| k.id.eq_ignore_ascii_case(&kname))
                                    .map(|k| (k.norm_x(), k.norm_y(), age))
                            })
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
                                        let dist = (nx - 0.5).abs() * 2.0; // 0 at center, 1 at edges
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
                        let counts: Vec<usize> =
                            aura.info.channels.iter().map(|c| c.led_count).collect();
                        for (ch_idx, count) in counts.into_iter().enumerate() {
                            let leds: Vec<Rgb> = (0..count)
                                .map(|i| {
                                    let nx = if count <= 1 {
                                        0.5
                                    } else {
                                        i as f32 / (count - 1) as f32
                                    };
                                    match prof.rgb.kind {
                                        EffectKind::Ambilight => self
                                            .last_ambilight
                                            .as_ref()
                                            .map(|a| a.sample_uv(nx, 0.5).scale(prof.rgb.brightness))
                                            .unwrap_or(prof.rgb.primary),
                                        EffectKind::ReactiveTyping => {
                                            let pulse = reactive_centers
                                                .iter()
                                                .map(|&(_, _, age)| (1.0 - age / 1.1).clamp(0.0, 1.0))
                                                .fold(0.0f32, f32::max);
                                            prof.rgb
                                                .secondary
                                                .scale(0.18)
                                                .lerp(prof.rgb.primary, pulse)
                                                .scale(prof.rgb.brightness)
                                        }
                                        EffectKind::AudioVisualizer => {
                                            let lvl = self.audio_smooth_peak;
                                            let dist = (nx - 0.5).abs() * 2.0;
                                            let bar = ((lvl - dist) * 4.5).clamp(0.0, 1.0);
                                            prof.rgb
                                                .secondary
                                                .scale(0.10)
                                                .lerp(prof.rgb.primary.lerp(prof.rgb.secondary, dist), bar)
                                                .scale(prof.rgb.brightness)
                                        }
                                        _ => prof.rgb.sample(i, nx, 0.5, t),
                                    }
                                })
                                .collect();
                            let _ = aura.send_channel_direct(ch_idx, &leds);
                        }
                    }
                } else if prof.silent_loop3_thermal_rgb {
                    let sensors = self.latest_sensors();
                    if let (Some(ch_idx), Some(aura)) =
                        (prof.silent_loop3_argb_channel, self.asus_aura.as_mut())
                    {
                        if let Some(ch) = aura.info.channels.get(ch_idx) {
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
                }
                last_rgb_push = Instant::now();
            }
        }
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

    fn logi_kb_mut(&mut self, p: &Value) -> std::result::Result<&mut LogitechKeyboard, String> {
        let idx = p["device_index"].as_u64().unwrap_or(0) as usize;
        self.logi_keyboards
            .get_mut(idx)
            .ok_or_else(|| "Clavier Logitech non connecté".to_string())
    }

    /// Run an IPC command, recording an undo snapshot when it changes the configuration.
    fn handle_cmd(&mut self, cmd: &str, p: &Value) -> std::result::Result<Value, String> {
        let Some(label) = undo_label_for(cmd) else {
            return self.handle_cmd_inner(cmd, p);
        };
        let before = self.config.clone();
        let before_json = serde_json::to_string(&before).unwrap_or_default();
        let mut res = self.handle_cmd_inner(cmd, p)?;
        let after_json = serde_json::to_string(&self.config).unwrap_or_default();
        if before_json != after_json {
            self.undo_stack.push((label.to_string(), before));
            if self.undo_stack.len() > UNDO_DEPTH {
                self.undo_stack.remove(0);
            }
            // The state was serialized before the snapshot was pushed: refresh the undo info.
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

    /// Reconnect everything if the set of HID devices changed. Returns `true` if it did.
    fn check_hotplug(&mut self) -> bool {
        let sig = compute_hid_signature();
        if sig == self.last_hid_sig {
            return false;
        }
        info!("Changement USB/HID détecté — reconnexion Plug & Play automatique…");
        self.last_hid_sig = sig;
        self.reconnect_all();
        self.reapply_runtime_state();
        true
    }

    fn profile_needs_input_engine(&mut self) -> bool {
        let p = self.config.active_profile_mut();
        p.socd.enabled
            || p.key_chatter_ms > 0
            || p.mouse_debounce_ms > 0
            || p.win_lock
            || !p.bindings.is_empty()
            || p.rgb.kind == EffectKind::ReactiveTyping
    }

    /// After a reconnection (hot-plug, resume from sleep), freshly opened devices know nothing of
    /// the current session: push back the runtime state they need.
    fn reapply_runtime_state(&mut self) {
        let (sync, thermal, counts) = {
            let p = self.config.active_profile_mut();
            (p.sync_asus_aura, p.silent_loop3_thermal_rgb, p.asus_argb_led_counts.clone())
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
        // New handles: the blackout frame (if any) must be pushed again.
        self.blackout_applied = false;
        // Windows may silently drop low-level hooks (sleep, hook timeout): restart if needed.
        if self.profile_needs_input_engine() && !self.input_engine.is_running() {
            if let Err(e) = self.input_engine.start() {
                error!("Moteur clavier non redémarré : {e}");
            }
        }
    }

    /// One black frame on every software-controllable light (Corsair keyboards, ASUS Aura).
    fn push_blackout_frame(&mut self) {
        for dev in &mut self.corsair_devices {
            if dev.info.kind == CorsairDeviceKind::Keyboard {
                let _ = dev.render_custom_frame(|_, _, _, _| Rgb::BLACK);
            }
        }
        if let Some(aura) = self.asus_aura.as_mut() {
            let _ = aura.enter_direct_mode();
            let counts: Vec<usize> = aura.info.channels.iter().map(|c| c.led_count).collect();
            for (ch, n) in counts.into_iter().enumerate() {
                let _ = aura.send_channel_direct(ch, &vec![Rgb::BLACK; n]);
            }
        }
        if !self.blackout_applied {
            info!("Éclairage coupé (session verrouillée ou inactivité)");
        }
        self.blackout_applied = true;
    }

    /// Give the lights back: live effects resume by themselves; hardware effects are re-enabled.
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
        if !(sync || thermal) {
            if let Some(aura) = self.asus_aura.as_mut() {
                let _ = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false);
            }
        }
        self.blackout_applied = false;
        info!("Éclairage rétabli");
    }

    /// Windows notification when a wireless device crosses 15 % then 5 % (once per discharge).
    fn check_battery_alerts(&mut self) {
        if !self.config.battery_alerts {
            return;
        }
        let mut readings: Vec<(String, String, u8, bool)> = Vec::new();
        for m in &self.logi_mice {
            if let Some(b) = &m.info.battery {
                if let Some(pct) = b.percent {
                    readings.push((format!("mouse-{:04X}-{}", m.info.pid, m.info.device_index), m.info.name.clone(), pct, battery_is_charging(b.state)));
                }
            }
        }
        for k in &self.logi_keyboards {
            if let Some(b) = &k.info.battery {
                if let Some(pct) = b.percent {
                    readings.push((format!("kb-{:04X}-{}", k.info.pid, k.info.device_index), k.info.name.clone(), pct, battery_is_charging(b.state)));
                }
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
                let title = if level <= 5 { "Batterie presque vide" } else { "Batterie faible" };
                if !tray_notify(title, &body) {
                    info!("{title} — {body}");
                }
            }
        }
    }

    /// Re-apply the hardware side effects of a configuration restored by `undo_last`.
    fn apply_restored_config(&mut self) {
        self.sync_input_rules();
        let (enable_rgb, sync_aura, thermal) = {
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
        if let Some(aura) = self.asus_aura.as_mut() {
            if sync_aura || thermal {
                let _ = aura.enter_direct_mode();
            } else if aura.is_in_direct_mode() {
                let _ = aura.set_all_hw_mode(self.aura_hw_mode, self.aura_hw_color, false);
            }
        }
        self.persist_config();
    }

    fn visible_apps_cached(&mut self) -> Vec<String> {
        if let Some((at, apps)) = &self.visible_apps_cache {
            if at.elapsed() < Duration::from_secs(5) {
                return apps.clone();
            }
        }
        let apps = list_visible_apps();
        self.visible_apps_cache = Some((Instant::now(), apps.clone()));
        apps
    }

    /// Apply a one-click preset. Never writes flash/EEPROM: lighting is a profile setting, fans use
    /// the BIOS policies (only on channels not driven by Smart Cooling), power plan if present.
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
            if self.cooling_status.wmi_available && is_elevated() {
                let mut applied = 0usize;
                let mut failed = 0usize;
                for ch in self.cooling_channels.iter_mut() {
                    if ch.source != FanTempSource::CpuBios {
                        continue; // Smart Cooling owns this channel
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
        self.persist_config();
        Ok(summary)
    }

    /// Human-readable health checks for the Diagnostic page.
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

        let (st, detail, action) = match pawnio_availability() {
            PawnIoAvailability::Ready => ("ok", "Pilote signé PawnIO actif (capteurs CPU, SMBus, SuperIO).".to_string(), Value::Null),
            PawnIoAvailability::NeedsAdmin => (
                "warn",
                "Le pilote PawnIO est installé mais nécessite les droits administrateur.".to_string(),
                Value::Null,
            ),
            PawnIoAvailability::NotInstalled => (
                "warn",
                "Le pilote signé PawnIO n'est pas installé : température CPU native, RAM RGB et ventilateurs directs indisponibles.".to_string(),
                json!({ "label": "Installer PawnIO", "cmd": "pawnio_install", "payload": {} }),
            ),
        };
        push("pawnio", "Pilote matériel PawnIO", st, detail, action);

        push(
            "asus_wmi",
            "Interface ventilateurs ASUS (WMI)",
            if self.cooling_status.wmi_available { "ok" } else { "warn" },
            self.cooling_status.message.clone(),
            Value::Null,
        );

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
            + usize::from(self.asus_aura.is_some());
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

        if let Some(err) = &self.asus_err {
            push(
                "aura",
                "Contrôleur ASUS Aura",
                "warn",
                format!("Non connecté : {err}"),
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

    /// Plain-text report to paste in a bug report: versions, machine, devices (no serial numbers),
    /// health checks and the end of the log. The user profile path is masked.
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
                "Corsair {} ({:04X}:{:04X}, {}) FW {} | mode {:?} | report {} o",
                d.info.product_name, d.info.vid, d.info.pid, d.info.kind.label(),
                d.info.firmware_version, d.info.mode, d.info.out_report_len
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
        match &self.asus_aura {
            Some(a) => {
                let _ = writeln!(r, "ASUS Aura {} FW {} | {} canaux", a.info.product_name, a.info.firmware_version, a.info.channels.len());
            }
            None => {
                let _ = writeln!(r, "ASUS Aura : {}", self.asus_err.as_deref().unwrap_or("absent"));
            }
        }
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
        let _ = writeln!(r, "\n--- Journal (200 dernières lignes) ---");
        r.push_str(&crate::app_log::tail(200));
        mask_user_profile(&r)
    }

    fn handle_cmd_inner(&mut self, cmd: &str, p: &Value) -> std::result::Result<Value, String> {
        match cmd {
            "get_state" => Ok(self.full_state_json()),
            // ================= UX: PREFERENCES, PRESETS, UNDO, DIAGNOSTICS, BACKUPS =================
            "set_ui_prefs" => {
                if let Some(mode) = p["ui_mode"].as_str() {
                    if mode != "simple" && mode != "expert" {
                        return Err(format!("Mode d'interface inconnu : {mode}"));
                    }
                    self.config.ui_mode = mode.to_string();
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
                let label = PRESETS
                    .iter()
                    .find(|(id, _, _)| *id == preset)
                    .map(|(_, l, _)| *l)
                    .unwrap_or(preset);
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(format!("Préréglage « {label} » appliqué — {}", summary.join(" • ")));
                Ok(st)
            }
            "undo_last" => {
                let (label, snapshot) = self.undo_stack.pop().ok_or("Rien à annuler")?;
                self.config = snapshot;
                self.apply_restored_config();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(format!("Annulé : {label}"));
                Ok(st)
            }
            // ================= WINDOWS SYSTEM EVENTS (system_events.rs) =================
            "__sys_event" => {
                match p["event"].as_str().unwrap_or("") {
                    "suspend" => {
                        info!("Mise en veille : pilotage des appareils suspendu");
                        self.suspended = true;
                    }
                    "resume" => {
                        info!("Sortie de veille : reconnexion des appareils…");
                        self.suspended = false;
                        // USB devices re-enumerate for a moment after wake-up.
                        std::thread::sleep(Duration::from_millis(1500));
                        self.reconnect_all();
                        self.reapply_runtime_state();
                    }
                    "lock" => {
                        if self.config.lights_off_on_lock {
                            self.blackout_lock = true;
                        }
                    }
                    "unlock" => self.blackout_lock = false,
                    "devices_changed" => {
                        if !self.suspended {
                            self.check_hotplug();
                        }
                    }
                    other => return Err(format!("Événement système inconnu : {other}")),
                }
                Ok(self.full_state_json())
            }
            "get_diagnostics" => Ok(self.diagnostics_json()),
            "get_diagnostic_report" => Ok(json!({ "text": self.diagnostic_report() })),
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
                Ok(self.full_state_json())
            }
            "set_power_plan" => {
                let guid = p["guid"].as_str().ok_or("GUID manquant")?;
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
                Ok(self.full_state_json())
            }
            "set_rgb_config" => {
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
                let en_now = prof.enable_rgb;
                if !en_now {
                    for dev in &mut self.corsair_devices {
                        if dev.info.kind == CorsairDeviceKind::Keyboard {
                            let _ = dev.set_mode(OperatingMode::Hardware);
                        }
                    }
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
                Ok(self.full_state_json())
            }
            "clear_per_key" => {
                self.config.active_profile_mut().rgb.per_key.clear();
                Ok(self.full_state_json())
            }
            // ================= CORSAIR BRAGI v2 UNIVERSAL (MULTI-DEVICE) =================
            "corsair_set_pollrate" => {
                let hz = p["hz"].as_u64().ok_or("hz manquant")? as u16;
                let rate = PollingRate::from_hz(hz).ok_or("Fréquence invalide")?;
                let dev = self.corsair_mut(p)?;
                dev.set_pollrate(rate).map_err(|e| e.to_string())?;
                self.config.active_profile_mut().polling_rate = rate;
                Ok(self.full_state_json())
            }
            "corsair_set_brightness" => {
                let pct = p["percent"].as_u64().ok_or("percent manquant")? as u8;
                let dev = self.corsair_mut(p)?;
                dev.set_hw_brightness_pct(pct).map_err(|e| e.to_string())?;
                self.config.active_profile_mut().hw_brightness_pct = pct.min(100);
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
                Ok(self.full_state_json())
            }
            "corsair_set_hw_winlock" => {
                let on = p["enabled"].as_bool().unwrap_or(false);
                let dev = self.corsair_mut(p)?;
                dev.set_hw_winlock(on).map_err(|e| e.to_string())?;
                Ok(self.full_state_json())
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
                let primary = p["primary"]
                    .as_str()
                    .and_then(Rgb::from_hex)
                    .unwrap_or(Rgb::CYAN);
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
                // Switch off live SW RGB streaming so the user immediately sees the flashed hardware effect!
                self.config.active_profile_mut().enable_rgb = false;
                Ok(self.full_state_json())
            }
            // ================= ASUS ROG AURA =================
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
                Ok(self.full_state_json())
            }
            "asus_set_hw_mode" => {
                let mode_str = p["mode"].as_str().unwrap_or("Static");
                let color = p["color"]
                    .as_str()
                    .and_then(Rgb::from_hex)
                    .unwrap_or(self.aura_hw_color);
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
                Ok(self.full_state_json())
            }
            "asus_set_shutdown_mode" => {
                let mode_str = p["mode"].as_str().unwrap_or("Off");
                let color = p["color"]
                    .as_str()
                    .and_then(Rgb::from_hex)
                    .unwrap_or(Rgb::BLACK);
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
                Ok(self.full_state_json())
            }
            // ================= RAM DDR5 RGB (ENE SMBus via ASUSManagement WMI) =================
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
                Ok(self.full_state_json())
            }
            // ================= LOGITECH G MULTI-DEVICE (MICE + KEYBOARDS + HEADSETS) =================
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
                let color = p["color"]
                    .as_str()
                    .and_then(Rgb::from_hex)
                    .unwrap_or(Rgb::CYAN);
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
                let color = p["color"]
                    .as_str()
                    .and_then(Rgb::from_hex)
                    .unwrap_or(Rgb::CYAN);
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
            // ================= COOLING & SILENT LOOP 3 =================
            "cooling_read_bios_curves" => {
                if !self.cooling_status.wmi_available {
                    return Err("Droits Admin + ASUSManagement WMI requis pour relire les courbes BIOS".into());
                }
                self.cooling_channels = FanHeaderKind::ALL
                    .iter()
                    .map(|&h| {
                        read_asus_wmi_channel(h).unwrap_or_else(|| FanChannelConfig::default_for(h))
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
                    ch.source = FanTempSource::from_str_id(src);
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
                    ch.curve = FanChannelConfig::silent_loop3_preset(preset);
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
                Ok(self.full_state_json())
            }
            "cooling_relaunch_admin" => {
                release_single_instance();
                relaunch_as_admin().map_err(|e| e.to_string())?;
                Ok(json!({ "relaunching": true }))
            }
            // ================= INPUT REMAPPER & MACROS =================
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
                Ok(self.full_state_json())
            }
            "input_remove_binding" => {
                let key = p["key"].as_str().ok_or("key manquante")?;
                self.config.active_profile_mut().bindings.remove(key);
                self.sync_input_rules();
                Ok(self.full_state_json())
            }
            // ================= ESPORTS OPTIMIZATIONS (SOCD / SNAP TAP, ANTI-CHATTER, MOUSE DEBOUNCE, LIVE RECORDER) =================
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
            // ================= SYSTEM TRAY, AUTOSTART, SHORTCUTS & AUTO-PROFILE SWITCH =================
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
            "tray_toggle_socd" => {
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
                Ok(self.full_state_json())
            }
            "tray_toggle_sync" => {
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
                Ok(self.full_state_json())
            }
            // ================= SUPERIO LIVE PWM (PAWNIO LPCIO) & HARDWARE SELF-TEST =================
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
            "pawnio_install" => {
                let msg = install_official_pawnio().map_err(|e| e.to_string())?;
                std::thread::sleep(std::time::Duration::from_millis(800));
                self.superio = read_superio_snapshot();
                self.ram_rgb = probe_ram_rgb();
                let mut st = self.full_state_json();
                st["last_action_msg"] = json!(msg);
                Ok(st)
            }
            "run_selftest" => {
                let rep = run_full_selftest();
                self.superio = read_superio_snapshot();
                self.ram_rgb = probe_ram_rgb();
                self.last_selftest = Some(rep);
                Ok(self.full_state_json())
            }
            // ================= WINDOWS CORE AUDIO (WASAPI) & PROFILE IMPORT/EXPORT =================
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
            _ => Err(format!("Commande IPC inconnue : {cmd}")),
        }
    }

    fn reconnect_all(&mut self) {
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

        self.cooling_status = probe_asus_wmi_cooling();
        self.power_plans = list_power_plans();
        self.scanned_devices = scan_hid_devices(false).unwrap_or_default();
        self.last_hid_sig = compute_hid_signature();
    }

    fn compute_mouse_power_model(&self, m: &LogitechMouse) -> Option<BatteryPowerModel> {
        let pct = m.info.battery.as_ref()?.percent?;
        let hz = m
            .info
            .report_rate
            .as_ref()
            .map(|r| r.current_hz)
            .unwrap_or(1000);
        if let Some(bm) = self.local_assets.find_battery_model(m.info.pid, &m.info.name) {
            Some(BatteryPowerModel::from_params(
                pct,
                bm.capacity_mwh,
                bm.sensor_mw,
                bm.mcu_mw,
                bm.radio_mw_for_hz(hz),
            ))
        } else {
            Some(BatteryPowerModel::for_pro_x_wireless(pct, hz))
        }
    }

    fn telemetry_json(&self) -> Value {
        let sensors = self.latest_sensors();
        let logi_bat = self.logi_mice.first().and_then(|m| {
            let b = m.info.battery.as_ref()?;
            let pm = self.compute_mouse_power_model(m);
            Some(json!({
                "percent": b.percent,
                "state": b.state.label(),
                "power_model": pm,
            }))
        });

        json!({
            "cpu_usage_pct": self.last_tel.cpu_usage_pct,
            "ram_used_gb": self.last_tel.ram_used_gb,
            "ram_total_gb": self.last_tel.ram_total_gb,
            "ram_usage_pct": self.last_tel.ram_usage_pct,
            "gpu": {
                "available": self.last_tel.gpu.available,
                "temp_c": self.last_tel.gpu.edge_temp_c.or(self.last_tel.gpu.hotspot_temp_c),
                "hotspot_c": self.last_tel.gpu.hotspot_temp_c,
                "activity_pct": self.last_tel.gpu.activity_pct,
                "fan_rpm": self.last_tel.gpu.fan_rpm,
                "core_clock_mhz": self.last_tel.gpu.core_clock_mhz,
                "power_watts": self.last_tel.gpu.power_watts,
            },
            "sensors": sensors,
            "audio": read_audio_state(),
            "logi_battery": logi_bat,
        })
    }

    fn serialize_corsair(&self, dev: &CorsairDevice, idx: usize) -> Value {
        let i = &dev.info;
        let hwp_profiles: Vec<Value> = i
            .hwp_profiles
            .iter()
            .map(|hp| {
                let lighting = hp.lighting.as_ref().map(|l| {
                    let cols: Vec<String> = l.colors.iter().map(|c| c.to_hex()).collect();
                    json!({
                        "file_id": format!("0x{:04X}", l.file_id),
                        "kind": l.kind.id(),
                        "kind_label": l.kind.label(),
                        "speed": l.speed,
                        "direction": l.direction,
                        "colors": cols,
                        "led_count": l.led_indices.len(),
                    })
                });
                json!({
                    "slot_index": hp.slot_index,
                    "profile_file_id": format!("0x{:04X}", hp.profile_file_id),
                    "name": hp.name,
                    "assignment_count": hp.assignment_count,
                    "property_count": hp.property_count,
                    "lighting": lighting,
                })
            })
            .collect();
        json!({
            "index": idx,
            "id": i.id,
            "product_name": i.product_name,
            "kind": format!("{:?}", i.kind),
            "kind_label": i.kind.label(),
            "vid": format!("{:04X}", i.vid),
            "pid": format!("{:04X}", i.pid),
            "subdevice_id": i.subdevice_id,
            "serial": i.serial,
            "firmware": i.firmware_version,
            "bootloader": i.bootloader_version,
            "layout": i.layout.label(),
            "mode": if i.mode == OperatingMode::Software { "sw" } else { "hw" },
            "pollrate_hz": i.pollrate.map(|p| p.hz()),
            "max_pollrate_hz": i.max_pollrate.map(|p| p.hz()),
            "hw_brightness_pct": dev.hw_brightness_pct(),
            "out_report_len": i.out_report_len,
            "battery_pct": i.battery_pct,
            "hw_winlock": i.hw_winlock,
            "winlock_shortcuts_mask": i.winlock_shortcuts_mask,
            "debounce_enabled": i.debounce_enabled,
            "debounce_timeout_ms": i.debounce_timeout_ms,
            "esports_switch_on": i.esports_switch_on,
            "active_hw_profile": i.active_hw_profile,
            "storage_total_kb": i.storage_total_kb,
            "storage_free_kb": i.storage_free_kb,
            "flashtap_supported": i.flashtap_supported,
            "rapid_trigger_supported": i.rapid_trigger_supported,
            "hwp_profiles": hwp_profiles,
            "mouse": i.mouse,
            "headset": i.headset,
            "cooler": i.cooler,
        })
    }

    fn serialize_logi_mouse(&self, m: &LogitechMouse, idx: usize) -> Value {
        let i = &m.info;
        let power_model = self.compute_mouse_power_model(m);
        let profiles: Vec<Value> = i
            .onboard_profiles
            .iter()
            .map(|p| {
                let btns: Vec<Value> = p
                    .buttons
                    .iter()
                    .map(|b| {
                        let raw = b.encode();
                        json!({
                            "hex": format!("{:02X}{:02X}{:02X}{:02X}", raw[0], raw[1], raw[2], raw[3]),
                            "label": b.label(),
                        })
                    })
                    .collect();
                json!({
                    "slot_index": p.slot_index,
                    "sector_addr": format!("0x{:04X}", p.sector_addr),
                    "from_rom": p.from_rom,
                    "enabled": p.enabled,
                    "crc_valid": p.crc_valid,
                    "name": p.name,
                    "report_rate_hz": p.report_rate_hz,
                    "default_dpi_idx": p.default_dpi_idx,
                    "dpi_stages": p.dpi_stages,
                    "buttons": btns,
                })
            })
            .collect();
        json!({
            "index": idx,
            "name": i.name,
            "vid": format!("{:04X}", i.vid),
            "pid": format!("{:04X}", i.pid),
            "device_index": format!("0x{:02X}", i.device_index),
            "proto_version": i.proto_version,
            "firmware": i.firmware,
            "battery": i.battery.as_ref().map(|b| json!({
                "percent": b.percent,
                "voltage_mv": b.voltage_mv,
                "state": b.state.label(),
            })),
            "power_model": power_model,
            "dpi": i.dpi,
            "report_rate": i.report_rate,
            "onboard": i.onboard.as_ref().map(|o| json!({
                "mode": match o.mode { OnboardMode::Host => "Host", _ => "Onboard" },
                "mode_label": o.mode.label(),
                "profile_count": o.profile_count,
                "button_count": o.button_count,
                "active_profile": o.active_profile,
                "active_dpi_index": o.active_dpi_index,
            })),
            "onboard_profiles": profiles,
            "angle_snapping": i.angle_snapping,
            "led_zones": i.led_zones,
            "features": i.features.iter().map(|f| json!({
                "index": f.index,
                "id": format!("0x{:04X}", f.id),
                "name": f.name,
            })).collect::<Vec<_>>(),
        })
    }

    fn serialize_logi_headset(&self, h: &LogitechHeadset, idx: usize) -> Value {
        let i = &h.info;
        json!({
            "index": idx,
            "name": i.name,
            "vid": format!("{:04X}", i.vid),
            "pid": format!("{:04X}", i.pid),
            "proto_version": i.proto_version,
            "firmware": i.firmware,
            "supports_sidetone": i.supports_sidetone,
            "sidetone_pct": i.sidetone_pct,
            "equalizer": i.equalizer,
            "presets": EQ_PRESETS.iter().map(|p| json!({
                "name": p.name,
                "gains": p.gains_5,
            })).collect::<Vec<_>>(),
        })
    }

    fn full_state_json(&mut self) -> Value {
        let active_profile = self.config.active_profile_mut().clone();
        let profile_names: Vec<String> = self.config.profiles.iter().map(|p| p.name.clone()).collect();

        let corsair_list: Vec<Value> = self
            .corsair_devices
            .iter()
            .enumerate()
            .map(|(i, d)| self.serialize_corsair(d, i))
            .collect();
        let corsair_primary = corsair_list.first().cloned();

        let asus_json = self.asus_aura.as_ref().map(|a| {
            let i = &a.info;
            let channels: Vec<Value> = i
                .channels
                .iter()
                .map(|c| {
                    json!({
                        "index": c.index,
                        "name": c.name,
                        "kind": if c.kind == AuraChannelKind::AddressableGen2 { "argb" } else { "mainboard" },
                        "led_count": c.led_count,
                        "default_led_count": c.default_led_count,
                    })
                })
                .collect();
            json!({
                "product_name": i.product_name,
                "vid": format!("{:04X}", i.vid),
                "pid": format!("{:04X}", i.pid),
                "firmware": i.firmware_version,
                "mainboard_fixed_leds": i.mainboard_fixed_leds,
                "rgb_12v_headers": i.rgb_12v_headers,
                "argb_5v_headers": i.argb_5v_headers,
                "channels": channels,
                "hw_mode": format!("{:?}", self.aura_hw_mode),
                "hw_color": self.aura_hw_color.to_hex(),
                "shutdown_mode": format!("{:?}", self.aura_shutdown_mode),
                "shutdown_color": self.aura_shutdown_color.to_hex(),
            })
        });

        let logi_mice_list: Vec<Value> = self
            .logi_mice
            .iter()
            .enumerate()
            .map(|(i, m)| self.serialize_logi_mouse(m, i))
            .collect();
        let logi_kbs_list: Vec<Value> = self
            .logi_keyboards
            .iter()
            .enumerate()
            .map(|(idx, k)| {
                let i = &k.info;
                json!({
                    "index": idx,
                    "id": i.id,
                    "name": i.name,
                    "vid": format!("{:04X}", i.vid),
                    "pid": format!("{:04X}", i.pid),
                    "device_index": format!("0x{:02X}", i.device_index),
                    "proto_version": i.proto_version,
                    "firmware": i.firmware,
                    "battery": i.battery.as_ref().map(|b| json!({
                        "percent": b.percent,
                        "state": b.state.label(),
                    })),
                    "supports_rgb_effects": i.supports_rgb_effects,
                    "supports_brightness": i.supports_brightness,
                    "brightness_pct": i.brightness_pct,
                    "supports_game_mode": i.supports_game_mode,
                })
            })
            .collect();
        let logi_headsets_list: Vec<Value> = self
            .logi_headsets
            .iter()
            .enumerate()
            .map(|(i, h)| self.serialize_logi_headset(h, i))
            .collect();

        let button_presets: Vec<Value> = MouseButtonAction::PRESET_ACTIONS
            .iter()
            .map(|(lbl, act)| {
                let raw = act.encode();
                json!({
                    "label": lbl,
                    "hex": format!("{:02X}{:02X}{:02X}{:02X}", raw[0], raw[1], raw[2], raw[3]),
                })
            })
            .collect();

        let keys_layout: Vec<Value> = K70_TKL_KEYS
            .iter()
            .map(|k| {
                json!({
                    "id": k.id,
                    "qwerty": k.label_qwerty,
                    "azerty": k.label_azerty,
                    "led_index": k.led_index,
                    "x": k.x,
                    "y": k.y,
                    "w": k.width,
                    "h": k.height,
                    "norm_x": k.norm_x(),
                    "norm_y": k.norm_y(),
                })
            })
            .collect();

        let effects: Vec<Value> = EffectKind::ALL
            .iter()
            .map(|&e| json!({ "id": format!("{:?}", e), "label": e.label() }))
            .collect();

        let aura_modes: Vec<Value> = AuraHwMode::HARDWARE_MODES
            .iter()
            .map(|&m| {
                json!({
                    "id": format!("{:?}", m),
                    "label": m.label(),
                    "uses_color": m.uses_color(),
                })
            })
            .collect();

        let ram_rgb_modes: Vec<Value> = EneRamMode::ALL
            .iter()
            .map(|&m| {
                json!({
                    "id": m.id(),
                    "label": m.label(),
                    "uses_color": m.uses_color(),
                })
            })
            .collect();

        let cooling_channels: Vec<Value> = self
            .cooling_channels
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                json!({
                    "index": idx,
                    "header": format!("{:?}", c.header),
                    "label": c.header.label(),
                    "short_label": c.header.short_label(),
                    "is_pump": c.header == FanHeaderKind::AioPump,
                    "policy": format!("{:?}", c.policy),
                    "source": format!("{:?}", c.source),
                    "hysteresis_c": c.hysteresis_c,
                    "curve": c.curve,
                })
            })
            .collect();

        let devices: Vec<Value> = self
            .scanned_devices
            .iter()
            .map(|d| {
                let is_logi = d.vid == 0x046D
                    && ((d.usage_page == 0xFF00 && d.usage == 0x0002)
                        || (d.usage_page == 0xFF43 && d.usage == 0x0202));
                let is_asus = d.vid == 0x0B05 && d.pid == 0x19AF && d.usage_page == 0xFF72;
                let status = if is_asus {
                    "Supporté (ASUS Aura USB)".to_string()
                } else if is_logi {
                    "Supporté (Logitech HID++ 2.0)".to_string()
                } else {
                    d.driver_status.clone()
                };
                json!({
                    "vendor": d.vendor.name(),
                    "vid_pid": format!("{:04X}:{:04X}", d.vid, d.pid),
                    "interface": d.interface_number,
                    "usage_page": format!("0x{:04X}", d.usage_page),
                    "usage": format!("0x{:04X}", d.usage),
                    "product": d.product_name,
                    "status": status,
                    "active": status.starts_with("Supporté"),
                })
            })
            .collect();

        let (chatter_blocked, mouse_dblclick_blocked) = self.input_engine.stats();
        let visible_apps = self.visible_apps_cached();

        let mut fw_inventory: Vec<Value> = vec![json!({
            "device": self.telemetry.static_info.motherboard,
            "vendor": "ASUS ROG",
            "version": format!("BIOS {}", self.telemetry.static_info.bios_version),
            "official_url": "https://rog.asus.com/support/",
            "status": "Lecture BIOS directe"
        })];
        if let Some(aura) = self.asus_aura.as_ref() {
            fw_inventory.push(json!({
                "device": "ASUS AURA LED Controller (0B05:19AF)",
                "vendor": "ASUS ROG",
                "version": aura.info.firmware_version,
                "official_url": "https://rog.asus.com/support/",
                "status": "MCU USB vérifié"
            }));
        }
        for d in &self.corsair_devices {
            fw_inventory.push(json!({
                "device": format!("{} ({:04X}:{:04X})", d.info.product_name, d.info.vid, d.info.pid),
                "vendor": "Corsair",
                "version": d.info.firmware_version,
                "official_url": "https://www.corsair.com/web-hub/",
                "status": "Bragi v2 vérifié"
            }));
        }
        for m in &self.logi_mice {
            fw_inventory.push(json!({
                "device": format!("{} ({:04X}:{:04X})", m.info.name, m.info.vid, m.info.pid),
                "vendor": "Logitech G",
                "version": if m.info.firmware.is_empty() { "HID++ 2.0".to_string() } else { m.info.firmware.join(" • ") },
                "official_url": "https://support.logi.com/hc/articles/360035037273",
                "status": "HID++ 2.0 vérifié"
            }));
        }
        for h in &self.logi_headsets {
            fw_inventory.push(json!({
                "device": format!("{} ({:04X}:{:04X})", h.info.name, h.info.vid, h.info.pid),
                "vendor": "Logitech G",
                "version": if h.info.firmware.is_empty() { "USB Audio".to_string() } else { h.info.firmware.join(" • ") },
                "official_url": "https://support.logi.com/",
                "status": "HID++ Audio vérifié"
            }));
        }

        json!({
            "static_info": {
                "motherboard": self.telemetry.static_info.motherboard,
                "bios_version": self.telemetry.static_info.bios_version,
                "cpu_name": self.telemetry.static_info.cpu_name,
                "gpu_name": self.telemetry.static_info.gpu_name,
            },
            "app_version": env!("CARGO_PKG_VERSION"),
            "config_warning": self.config_warning,
            "ui": {
                "mode": self.config.ui_mode,
                "onboarding_done": self.config.onboarding_done,
                "can_undo": !self.undo_stack.is_empty(),
                "undo_label": self.undo_stack.last().map(|(l, _)| l.clone()),
            },
            "presets": PRESETS.iter().map(|(id, label, description)| json!({
                "id": id,
                "label": label,
                "description": description,
            })).collect::<Vec<_>>(),
            "system_prefs": {
                "autostart": is_autostart_enabled(),
                "minimize_to_tray": self.config.minimize_to_tray,
                "start_minimized": self.config.start_minimized,
                "auto_profile_switch": self.config.auto_profile_switch,
                "lights_off_on_lock": self.config.lights_off_on_lock,
                "idle_lights_off_min": self.config.idle_lights_off_min,
                "battery_alerts": self.config.battery_alerts,
                "lights_blackout": self.blackout_lock || self.blackout_idle,
                "foreground_app": self.foreground_app,
                "auto_switched": self.auto_switched,
                "socd_suspended": self.input_engine.is_socd_suspended(),
                "chatter_blocked": chatter_blocked,
                "mouse_dblclick_blocked": mouse_dblclick_blocked,
                "macro_recording": self.input_engine.is_recording_macro(),
                "visible_apps": visible_apps,
            },
            "firmware_inventory": fw_inventory,
            "local_assets": self.local_assets.summary(),
            "telemetry": self.telemetry_json(),
            "config": {
                "active_profile": self.config.active_profile,
                "profiles": profile_names,
                "use_azerty_labels": self.config.use_azerty_labels,
                "profile": {
                    "name": active_profile.name,
                    "polling_rate_hz": active_profile.polling_rate.hz(),
                    "hw_brightness_pct": active_profile.hw_brightness_pct,
                    "enable_rgb": active_profile.enable_rgb,
                    "sync_asus_aura": active_profile.sync_asus_aura,
                    "win_lock": active_profile.win_lock,
                    "bindings": active_profile.bindings,
                    "socd": active_profile.socd,
                    "key_chatter_ms": active_profile.key_chatter_ms,
                    "mouse_debounce_ms": active_profile.mouse_debounce_ms,
                    "linked_apps": active_profile.linked_apps,
                    "silent_loop3_argb_channel": active_profile.silent_loop3_argb_channel,
                    "silent_loop3_thermal_rgb": active_profile.silent_loop3_thermal_rgb,
                    "rgb": {
                        "kind": format!("{:?}", active_profile.rgb.kind),
                        "primary": active_profile.rgb.primary.to_hex(),
                        "secondary": active_profile.rgb.secondary.to_hex(),
                        "speed": active_profile.rgb.speed,
                        "brightness": active_profile.rgb.brightness,
                        "reverse": active_profile.rgb.reverse,
                        "per_key": active_profile.rgb.per_key.iter().map(|(k, v)| (k.to_string(), v.to_hex())).collect::<std::collections::BTreeMap<_, _>>(),
                    }
                }
            },
            "input_running": self.input_engine.is_running(),
            "bindable_keys": ALL_BINDABLE_KEYS,
            "keys_layout": keys_layout,
            "effects": effects,
            "hwp_lighting_kinds": HwpLightingKind::ALL_SUPPORTED.iter().map(|&k| json!({
                "id": k.id(),
                "label": k.label(),
                "color_count": k.color_count(),
            })).collect::<Vec<_>>(),
            "aura_modes": aura_modes,
            "ram_rgb": self.ram_rgb,
            "ram_rgb_modes": ram_rgb_modes,
            "button_presets": button_presets,
            "power_plans": self.power_plans.iter().map(|p| json!({
                "guid": p.guid,
                "name": p.name,
                "active": p.active,
            })).collect::<Vec<_>>(),
            "corsair": corsair_primary,
            "corsair_devices": corsair_list,
            "corsair_err": if self.corsair_devices.is_empty() { Some("Aucun périphérique Corsair Bragi v2 détecté") } else { None },
            "asus": asus_json,
            "asus_err": self.asus_err,
            "logi_mouse": logi_mice_list.first().cloned(),
            "logi_mice": logi_mice_list,
            "logi_keyboards": logi_kbs_list,
            "logi_headset": logi_headsets_list.first().cloned(),
            "logi_headsets": logi_headsets_list,
            "logi_mouse_err": if self.logi_mice.is_empty() { Some("Aucune souris Logitech détectée (en veille ?)") } else { None },
            "logi_headset_err": if self.logi_headsets.is_empty() { Some("Aucun casque Logitech détecté") } else { None },
            "cooling": {
                "is_admin": self.cooling_status.is_admin,
                "wmi_available": self.cooling_status.wmi_available,
                "message": self.cooling_status.message,
                "channels": cooling_channels,
                "superio": self.superio,
            },
            "selftest": self.last_selftest,
            "devices": devices,
        })
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
    // Byte-indexed slicing below: reject non-ASCII input instead of panicking the backend thread.
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

fn thermal_rgb(temp_c: f32) -> Rgb {
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
        Rgb::new(
            255,
            (185.0 - f * 155.0) as u8,
            (30.0 + f * 40.0) as u8,
        )
    }
}

/// Replace the user profile folder (which contains the Windows user name) by `%USERPROFILE%`.
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
    use super::{battery_alert_step, mask_user_profile};

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
        // ...and does not then fire the 15 % alert
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
}
