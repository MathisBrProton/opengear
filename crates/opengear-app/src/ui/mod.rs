pub(crate) mod backend;
pub(crate) mod commands;
pub(crate) mod devices;
pub(crate) mod endpoint_tests;
pub(crate) mod keyboard_view;
pub(crate) mod state;

use crate::fan_watchdog;
use crate::local_assets::LocalAssetCatalog;
use crate::selftest::SelfTestReport;
use crate::system_events::{self, SysEvent};
use crate::tray_icon::{
    TrayAction, acquire_single_instance, build_window_icon, ensure_ico_beside_exe,
    release_single_instance, start_system_tray, stop_system_tray, trim_working_set,
};
use anyhow::Result;
use opengear_core::AppConfig;
use opengear_mainboard::{HardwareSensorsSnapshot, read_hardware_sensors};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use tracing::{error, info};
use wry::{WebViewBuilder, http};

const INDEX_HTML: &[u8] = include_bytes!("../../ui/index.html");
const APP_CSS: &[u8] = include_bytes!("../../ui/app.css");
const JS_MAIN: &[u8] = include_bytes!("../../ui/js/main.js");
const JS_UTILS: &[u8] = include_bytes!("../../ui/js/utils.js");
const JS_IPC: &[u8] = include_bytes!("../../ui/js/ipc.js");
const JS_MODALS: &[u8] = include_bytes!("../../ui/js/modals.js");
const JS_STATE: &[u8] = include_bytes!("../../ui/js/state.js");
const JS_COMPONENTS: &[u8] = include_bytes!("../../ui/js/components.js");
const JS_RGB_PREVIEW: &[u8] = include_bytes!("../../ui/js/rgb-preview.js");
const JS_VIEW_HOME: &[u8] = include_bytes!("../../ui/js/views/home.js");
const JS_VIEW_LIGHTING: &[u8] = include_bytes!("../../ui/js/views/lighting.js");
const JS_VIEW_PERFORMANCE: &[u8] = include_bytes!("../../ui/js/views/performance.js");
const JS_VIEW_INPUT: &[u8] = include_bytes!("../../ui/js/views/input.js");
const JS_VIEW_PROFILES: &[u8] = include_bytes!("../../ui/js/views/profiles.js");
const JS_VIEW_DEVICES: &[u8] = include_bytes!("../../ui/js/views/devices.js");
const JS_VIEW_SYSTEM: &[u8] = include_bytes!("../../ui/js/views/system.js");

const IMG_MOUSE: &[u8] = include_bytes!("../../assets/mouse.webp");
const IMG_MOUSE_WHITE: &[u8] = include_bytes!("../../assets/mouse_white.webp");
const IMG_KEYBOARD: &[u8] = include_bytes!("../../assets/keyboard.webp");
const IMG_MOTHERBOARD: &[u8] = include_bytes!("../../assets/motherboard.webp");
const IMG_HEADSET: &[u8] = include_bytes!("../../assets/headset.webp");
const IMG_AIO: &[u8] = include_bytes!("../../assets/aio.webp");
const IMG_RAM: &[u8] = include_bytes!("../../assets/ram.webp");
const IMG_LAPTOP: &[u8] = include_bytes!("../../assets/laptop.webp");
const IMG_GENERIC: &[u8] = include_bytes!("../../assets/generic_device.webp");

const CSP_HEADER: &str = "default-src 'self' http://opengear.localhost https://opengear.localhost; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: http://opengear.localhost https://opengear.localhost; font-src 'self' data:; connect-src 'none'; object-src 'none'; frame-ancestors 'none'";

pub(crate) static UI_VISIBLE: AtomicBool = AtomicBool::new(true);
pub(crate) static MINIMIZE_TO_TRAY: AtomicBool = AtomicBool::new(true);
pub(crate) static THERMAL_RGB_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static SMART_FAN_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
pub(crate) struct IpcRequest {
    pub id: u64,
    pub cmd: String,
    #[serde(default)]
    pub payload: Value,
}

pub(crate) enum BackendMsg {
    Rpc(IpcRequest),
    SysEvent(SysEvent),
    TrayToggleSocd,
    TrayToggleSync,
    TraySwitchProfile(String),
    TrayApplyPreset(String),
    AsyncWorkerDone {
        id: u64,
        action_msg: Option<String>,
        selftest: Option<SelfTestReport>,
        refresh_hw: bool,
        error: Option<String>,
    },
    Shutdown(mpsc::Sender<()>),
}

#[derive(Debug)]
pub(crate) enum UserEvent {
    Reply(u64, String),
    EvalJs(String),
    Tray(TrayAction),
    SysEvent(SysEvent),
    WakeUp,
    Quit,
}

pub(crate) fn is_allowed_origin(url: &str) -> bool {
    url.starts_with("http://opengear.localhost/")
        || url == "http://opengear.localhost"
        || url.starts_with("https://opengear.localhost/")
        || url == "https://opengear.localhost"
        || url.starts_with("opengear://")
}

pub(crate) fn resolve_static_asset(path: &str) -> Option<(&'static [u8], &'static str, bool)> {
    match path {
        "/" | "/index.html" => Some((INDEX_HTML, "text/html; charset=utf-8", true)),
        "/app.css" => Some((APP_CSS, "text/css; charset=utf-8", false)),
        "/js/main.js" => Some((JS_MAIN, "application/javascript; charset=utf-8", false)),
        "/js/utils.js" => Some((JS_UTILS, "application/javascript; charset=utf-8", false)),
        "/js/ipc.js" => Some((JS_IPC, "application/javascript; charset=utf-8", false)),
        "/js/modals.js" => Some((JS_MODALS, "application/javascript; charset=utf-8", false)),
        "/js/state.js" => Some((JS_STATE, "application/javascript; charset=utf-8", false)),
        "/js/components.js" => Some((
            JS_COMPONENTS,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/js/rgb-preview.js" => Some((
            JS_RGB_PREVIEW,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/js/views/home.js" => Some((JS_VIEW_HOME, "application/javascript; charset=utf-8", false)),
        "/js/views/lighting.js" => Some((
            JS_VIEW_LIGHTING,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/js/views/performance.js" => Some((
            JS_VIEW_PERFORMANCE,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/js/views/input.js" => Some((
            JS_VIEW_INPUT,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/js/views/profiles.js" => Some((
            JS_VIEW_PROFILES,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/js/views/devices.js" => Some((
            JS_VIEW_DEVICES,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/js/views/system.js" => Some((
            JS_VIEW_SYSTEM,
            "application/javascript; charset=utf-8",
            false,
        )),
        "/assets/mouse.webp" | "/assets/mouse_superlight.jpg" => {
            Some((IMG_MOUSE, "image/webp", false))
        }
        "/assets/mouse_white.webp" | "/assets/mouse_superlight_white.jpg" => {
            Some((IMG_MOUSE_WHITE, "image/webp", false))
        }
        "/assets/keyboard.webp" | "/assets/keyboard_k70_tkl.jpg" => {
            Some((IMG_KEYBOARD, "image/webp", false))
        }
        "/assets/motherboard.webp" | "/assets/motherboard_b650a.jpg" => {
            Some((IMG_MOTHERBOARD, "image/webp", false))
        }
        "/assets/headset.webp" | "/assets/headset_prox.jpg" => {
            Some((IMG_HEADSET, "image/webp", false))
        }
        "/assets/aio.webp" | "/assets/aio_silentloop3.jpg" => Some((IMG_AIO, "image/webp", false)),
        "/assets/ram.webp" => Some((IMG_RAM, "image/webp", false)),
        "/assets/laptop.webp" => Some((IMG_LAPTOP, "image/webp", false)),
        "/assets/generic_device.webp" => Some((IMG_GENERIC, "image/webp", false)),
        _ => None,
    }
}

#[cfg(windows)]
fn apply_dark_titlebar(window: &tao::window::Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    let hwnd = window.hwnd();
    if hwnd != 0 {
        let val: u32 = 1;
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd as HWND,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                (&val as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}

#[cfg(not(windows))]
fn apply_dark_titlebar(_window: &tao::window::Window) {}

fn build_webview(
    window: &tao::window::Window,
    proto_assets: Arc<LocalAssetCatalog>,
    ipc_tx: mpsc::Sender<BackendMsg>,
) -> Result<wry::WebView> {
    WebViewBuilder::new()
        .with_custom_protocol("opengear".into(), move |_webview_id, req| {
            let path = req.uri().path();
            if let Some(key) = path.strip_prefix("/local-asset/")
                && let Some((bytes, mime)) = proto_assets.get_blob(key)
            {
                return http::Response::builder()
                    .header(http::header::CONTENT_TYPE, mime)
                    .body(std::borrow::Cow::Owned(bytes.to_vec()))
                    .unwrap();
            }
            if let Some((body, mime, is_html)) = resolve_static_asset(path) {
                let mut builder =
                    http::Response::builder().header(http::header::CONTENT_TYPE, mime);
                if is_html {
                    builder = builder.header("Content-Security-Policy", CSP_HEADER);
                }
                return builder.body(std::borrow::Cow::Borrowed(body)).unwrap();
            }
            http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(std::borrow::Cow::Borrowed(b"Not found" as &[u8]))
                .unwrap()
        })
        .with_navigation_handler(|url| is_allowed_origin(&url))
        .with_new_window_req_handler(|_| false)
        .with_ipc_handler(move |req| {
            let origin = req.uri().to_string();
            if !is_allowed_origin(&origin) {
                return;
            }
            if let Ok(msg) = serde_json::from_str::<IpcRequest>(req.body()) {
                let _ = ipc_tx.send(BackendMsg::Rpc(msg));
            }
        })
        .with_url("http://opengear.localhost/")
        .build(window)
        .map_err(|e| anyhow::anyhow!("Impossible d'initialiser WebView2 : {e}"))
}

pub fn run_gui(cli_minimized: bool) -> Result<()> {
    if !acquire_single_instance() {
        info!("OpenGear est déjà en cours d'exécution — réveil de la fenêtre existante.");
        return Ok(());
    }

    fan_watchdog::recover_stale_marker();
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);

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

    let local_assets = Arc::new(LocalAssetCatalog::discover());

    let shared_sensors = Arc::new(Mutex::new(HardwareSensorsSnapshot::default()));
    let sensors_bg = Arc::clone(&shared_sensors);
    std::thread::Builder::new()
        .name("opengear-sensors".into())
        .spawn(move || {
            let mut last_probe: Option<Instant> = None;
            let mut last_available = false;
            while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                let needed = UI_VISIBLE.load(Ordering::Relaxed)
                    || THERMAL_RGB_ACTIVE.load(Ordering::Relaxed);
                let smart = SMART_FAN_ACTIVE.load(Ordering::Relaxed);
                if needed || smart {
                    let interval = if last_available {
                        Duration::from_secs(3)
                    } else {
                        Duration::from_secs(60)
                    };
                    if last_probe.is_none_or(|t| t.elapsed() >= interval) {
                        let snap = read_hardware_sensors();
                        last_available = snap.available;
                        last_probe = Some(Instant::now());
                        if let Ok(mut g) = sensors_bg.lock() {
                            *g = snap;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .ok();

    let (ipc_tx, ipc_rx) = mpsc::channel::<BackendMsg>();
    let backend_assets = Arc::clone(&local_assets);
    let backend_sensors = Arc::clone(&shared_sensors);
    let backend_proxy = proxy.clone();
    let backend_tx = ipc_tx.clone();
    std::thread::Builder::new()
        .name("opengear-backend".into())
        .spawn(move || {
            let mut engine = backend::BackendEngine::new(backend_assets, backend_sensors);
            engine.run_loop(ipc_rx, backend_proxy, backend_tx);
        })
        .map_err(|e| anyhow::anyhow!("Impossible de démarrer le moteur : {e}"))?;

    let ev_proxy = proxy.clone();
    system_events::start(move |ev| {
        let _ = ev_proxy.send_event(UserEvent::SysEvent(ev));
    });

    #[cfg(windows)]
    {
        let wake_proxy = proxy.clone();
        std::thread::Builder::new()
            .name("opengear-wake".into())
            .spawn(move || {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, TranslateMessage,
                };
                unsafe {
                    let mut msg: MSG = std::mem::zeroed();
                    PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                    while !SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
                        && GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0
                    {
                        if msg.message == crate::tray_icon::wake_message_id() {
                            let _ = wake_proxy.send_event(UserEvent::WakeUp);
                        }
                        TranslateMessage(&msg);
                    }
                }
            })
            .ok();
    }

    let mut webview = if start_hidden {
        None
    } else {
        Some(build_webview(
            &window,
            Arc::clone(&local_assets),
            ipc_tx.clone(),
        )?)
    };

    if start_hidden {
        trim_working_set();
    }

    let mut tray_mode = start_hidden;
    let mut exit_started = false;
    let show_assets = Arc::clone(&local_assets);
    let show_ipc = ipc_tx.clone();
    let show_window =
        move |win: &tao::window::Window, wv: &mut Option<wry::WebView>, tm: &mut bool| {
            if wv.is_none() {
                match build_webview(win, Arc::clone(&show_assets), show_ipc.clone()) {
                    Ok(v) => *wv = Some(v),
                    Err(e) => error!("Création WebView2 impossible : {e}"),
                }
            }
            win.set_visible(true);
            win.set_minimized(false);
            win.set_focus();
            UI_VISIBLE.store(true, Ordering::SeqCst);
            *tm = false;
        };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if !exit_started && MINIMIZE_TO_TRAY.load(Ordering::Relaxed) {
                    window.set_visible(false);
                    UI_VISIBLE.store(false, Ordering::SeqCst);
                    webview = None;
                    tray_mode = true;
                    trim_working_set();
                } else if !exit_started {
                    exit_started = true;
                    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                    system_events::stop();
                    shutdown_backend(&ipc_tx);
                    stop_system_tray();
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Tray(action)) => match action {
                TrayAction::ShowWindow => {
                    show_window(&window, &mut webview, &mut tray_mode);
                }
                TrayAction::ToggleSocd => {
                    let _ = ipc_tx.send(BackendMsg::TrayToggleSocd);
                }
                TrayAction::ToggleSyncRgb => {
                    let _ = ipc_tx.send(BackendMsg::TrayToggleSync);
                }
                TrayAction::SwitchProfile(name) => {
                    let _ = ipc_tx.send(BackendMsg::TraySwitchProfile(name));
                }
                TrayAction::ApplyPreset(id) => {
                    let _ = ipc_tx.send(BackendMsg::TrayApplyPreset(id));
                }
                TrayAction::Quit => {
                    if !exit_started {
                        exit_started = true;
                        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                        system_events::stop();
                        shutdown_backend(&ipc_tx);
                        stop_system_tray();
                        *control_flow = ControlFlow::Exit;
                    }
                }
            },
            Event::UserEvent(UserEvent::WakeUp) => {
                show_window(&window, &mut webview, &mut tray_mode);
            }
            Event::UserEvent(UserEvent::Quit) => {
                if !exit_started {
                    exit_started = true;
                    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                    system_events::stop();
                    shutdown_backend(&ipc_tx);
                    stop_system_tray();
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::SysEvent(ev)) => {
                let _ = ipc_tx.send(BackendMsg::SysEvent(ev));
            }
            Event::UserEvent(UserEvent::Reply(id, json_payload)) => {
                if let Some(ref wv) = webview {
                    let _ = wv.evaluate_script(&format!(
                        "window.__ogReply && window.__ogReply({}, {});",
                        id, json_payload
                    ));
                }
            }
            Event::UserEvent(UserEvent::EvalJs(script)) => {
                if !tray_mode && let Some(ref wv) = webview {
                    let _ = wv.evaluate_script(&script);
                }
            }
            _ => {}
        }
    });
}

fn shutdown_backend(tx: &mpsc::Sender<BackendMsg>) {
    let (ack_tx, ack_rx) = mpsc::channel();
    if tx.send(BackendMsg::Shutdown(ack_tx)).is_ok() {
        let _ = ack_rx.recv_timeout(Duration::from_secs(3));
    }
    release_single_instance();
}

pub(crate) fn ok_reply(data: Value) -> String {
    json!({ "ok": true, "data": data }).to_string()
}

pub(crate) fn err_reply(msg: &str) -> String {
    json!({ "ok": false, "error": msg }).to_string()
}

pub(crate) fn send_reply(
    proxy: &EventLoopProxy<UserEvent>,
    id: u64,
    res: std::result::Result<Value, String>,
) {
    let payload = match res {
        Ok(data) => ok_reply(data),
        Err(e) => err_reply(&e),
    };
    let _ = proxy.send_event(UserEvent::Reply(id, payload));
}
