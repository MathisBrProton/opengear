//! OpenRGB SDK (`ORGB`, localhost TCP) server bridge so third-party tools (SignalRGB, Artemis,
//! Aurora, Wallpaper Engine…) can discover and animate OpenGear lighting devices over loopback.
//!
//! Disabled by default (opt-in only), listens exclusively on `127.0.0.1`, and implements the
//! documented binary protocol from scratch cleanly without GPL code.

use crate::{
    MAX_PACKET_LEN, PKT_REQUEST_CONTROLLER_COUNT, PKT_REQUEST_CONTROLLER_DATA, PKT_SET_CLIENT_NAME,
    PKT_SET_CUSTOM_MODE, PKT_UPDATE_LEDS, parse_header,
};
use anyhow::{Context, Result};
use opengear_core::Rgb;
use opengear_core::driver::{DeviceInfo, DeviceKind};
use opengear_core::lighting::{Zone, ZoneKind};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, info, warn};

const PKT_REQUEST_PROTOCOL_VERSION: u32 = 40;
const PKT_RESIZE_ZONE: u32 = 1000;
const PKT_UPDATE_ZONE_LEDS: u32 = 1051;
const PKT_UPDATE_SINGLE_LED: u32 = 1052;
const PKT_UPDATE_MODE: u32 = 1101;
const SUPPORTED_PROTOCOL_VERSION: u32 = 3;

/// LED controller exposed to external OpenRGB SDK clients.
#[derive(Debug, Clone)]
pub struct SdkController {
    pub device_id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: DeviceKind,
    pub zones: Vec<Zone>,
    pub colors: Vec<Rgb>,
}

impl SdkController {
    /// Build an OpenRGB-exposed controller snapshot from OpenGear's device metadata & zones.
    pub fn from_device(info: &DeviceInfo, zones: Vec<Zone>) -> Self {
        let total = zones.iter().map(|z| z.leds.len()).sum::<usize>();
        Self {
            device_id: info.id.clone(),
            name: info.model.clone(),
            vendor: info.vendor.clone(),
            version: info.firmware.clone().unwrap_or_else(|| "1.0".into()),
            kind: info.kind,
            zones,
            colors: vec![Rgb::BLACK; total],
        }
    }

    pub fn total_leds(&self) -> usize {
        self.zones.iter().map(|z| z.leds.len()).sum()
    }
}

/// Event emitted to the backend loop when an external OpenRGB SDK client pushes a frame.
#[derive(Debug, Clone)]
pub enum OpenRgbServerEvent {
    SetCustomMode { device_id: String },
    UpdateLeds { device_id: String, colors: Vec<Rgb> },
}

#[derive(Default)]
struct SharedState {
    controllers: Vec<SdkController>,
}

/// Running localhost TCP OpenRGB SDK server (`127.0.0.1`). Stopped automatically on drop.
pub struct OpenRgbSdkServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    state: Arc<Mutex<SharedState>>,
    rx: Receiver<OpenRgbServerEvent>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OpenRgbSdkServer {
    /// Start listening on `bind_addr` (must be loopback, e.g. `127.0.0.1:6742` or `127.0.0.1:0`).
    pub fn start(bind_addr: &str, controllers: Vec<SdkController>) -> Result<Self> {
        let addr: SocketAddr = bind_addr
            .parse()
            .with_context(|| format!("Adresse OpenRGB SDK invalide : {bind_addr}"))?;
        anyhow::ensure!(
            addr.ip().is_loopback(),
            "Le serveur OpenRGB SDK n'écoute que sur loopback (127.0.0.1)"
        );
        let listener = TcpListener::bind(addr).with_context(|| {
            format!("Impossible d'ouvrir le serveur OpenRGB SDK sur {addr} (port occupé ?)")
        })?;
        listener
            .set_nonblocking(true)
            .context("Configuration non-bloquante TCP OpenRGB SDK")?;
        let local_addr = listener.local_addr().unwrap_or(addr);
        let stop = Arc::new(AtomicBool::new(false));
        let clients = Arc::new(AtomicUsize::new(0));
        let state = Arc::new(Mutex::new(SharedState { controllers }));
        let (tx, rx) = mpsc::sync_channel::<OpenRgbServerEvent>(64);

        let stop_bg = Arc::clone(&stop);
        let clients_bg = Arc::clone(&clients);
        let state_bg = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name("opengear-openrgb-sdk".into())
            .spawn(move || accept_loop(listener, stop_bg, clients_bg, state_bg, tx))
            .context("Démarrage thread serveur OpenRGB SDK")?;

        info!(%local_addr, "Serveur OpenRGB SDK démarré (127.0.0.1)");
        Ok(Self {
            addr: local_addr,
            stop,
            clients,
            state,
            rx,
            thread: Some(thread),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn client_count(&self) -> usize {
        self.clients.load(Ordering::Relaxed)
    }

    /// Replace the exposed controller list (e.g. after hotplug or zone resize).
    pub fn set_controllers(&self, controllers: Vec<SdkController>) {
        if let Ok(mut guard) = self.state.lock() {
            guard.controllers = controllers;
        }
    }

    /// Non-blocking drain of pending LED updates pushed by external SDK clients.
    pub fn poll_events(&self) -> Vec<OpenRgbServerEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }
}

impl Drop for OpenRgbSdkServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        info!("Serveur OpenRGB SDK arrêté");
    }
}

fn accept_loop(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    state: Arc<Mutex<SharedState>>,
    tx: SyncSender<OpenRgbServerEvent>,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let stop_c = Arc::clone(&stop);
                let clients_c = Arc::clone(&clients);
                let state_c = Arc::clone(&state);
                let tx_c = tx.clone();
                clients_c.fetch_add(1, Ordering::Relaxed);
                let _ = std::thread::Builder::new()
                    .name("opengear-openrgb-client".into())
                    .spawn(move || {
                        debug!(%peer, "Client OpenRGB SDK connecté");
                        handle_client(stream, stop_c, state_c, tx_c);
                        clients_c.fetch_sub(1, Ordering::Relaxed);
                        debug!(%peer, "Client OpenRGB SDK déconnecté");
                    });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => {
                warn!(?e, "Erreur acceptation serveur OpenRGB SDK");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn handle_client(
    mut stream: TcpStream,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<SharedState>>,
    tx: SyncSender<OpenRgbServerEvent>,
) {
    // Inherited sockets from a non-blocking TcpListener stay non-blocking on Windows.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

    while !stop.load(Ordering::Acquire) {
        let mut hdr = [0u8; 16];
        match stream.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => break,
        }
        let Ok((dev_idx, pkt_id, len)) = parse_header(&hdr) else {
            break;
        };
        let mut payload = vec![0u8; len];
        if len > 0 && stream.read_exact(&mut payload).is_err() {
            break;
        }
        match pkt_id {
            PKT_SET_CLIENT_NAME => {
                let name = String::from_utf8_lossy(&payload)
                    .trim_end_matches('\0')
                    .to_string();
                info!(client = %name, "Client OpenRGB SDK enregistré");
            }
            PKT_REQUEST_PROTOCOL_VERSION => {
                let client_ver = payload
                    .get(0..4)
                    .and_then(|s| s.try_into().ok())
                    .map(u32::from_le_bytes)
                    .unwrap_or(SUPPORTED_PROTOCOL_VERSION);
                let reply = client_ver.min(SUPPORTED_PROTOCOL_VERSION).to_le_bytes();
                if send_packet(&mut stream, 0, PKT_REQUEST_PROTOCOL_VERSION, &reply).is_err() {
                    break;
                }
            }
            PKT_REQUEST_CONTROLLER_COUNT => {
                let n = state
                    .lock()
                    .map(|g| g.controllers.len() as u32)
                    .unwrap_or(0)
                    .to_le_bytes();
                if send_packet(&mut stream, 0, PKT_REQUEST_CONTROLLER_COUNT, &n).is_err() {
                    break;
                }
            }
            PKT_REQUEST_CONTROLLER_DATA => {
                let body = state
                    .lock()
                    .ok()
                    .and_then(|g| g.controllers.get(dev_idx as usize).cloned())
                    .map(|c| encode_controller_data(&c))
                    .unwrap_or_default();
                if send_packet(&mut stream, dev_idx, PKT_REQUEST_CONTROLLER_DATA, &body).is_err() {
                    break;
                }
            }
            PKT_SET_CUSTOM_MODE => {
                if let Some(dev_id) = state.lock().ok().and_then(|g| {
                    g.controllers
                        .get(dev_idx as usize)
                        .map(|c| c.device_id.clone())
                }) {
                    let _ = tx.try_send(OpenRgbServerEvent::SetCustomMode { device_id: dev_id });
                }
            }
            PKT_UPDATE_LEDS => {
                if let Some(ev) = apply_led_packet(&state, dev_idx as usize, None, &payload) {
                    let _ = tx.try_send(ev);
                }
            }
            PKT_UPDATE_ZONE_LEDS => {
                // Header inside PKT_UPDATE_ZONE_LEDS: data_size(u32), zone_idx(u32), count(u16), colors...
                if payload.len() >= 10
                    && let Ok(zbytes) = payload[4..8].try_into()
                {
                    let zone_idx = u32::from_le_bytes(zbytes) as usize;
                    if let Some(ev) =
                        apply_led_packet(&state, dev_idx as usize, Some(zone_idx), &payload[8..])
                    {
                        let _ = tx.try_send(ev);
                    }
                }
            }
            PKT_UPDATE_SINGLE_LED => {
                // led_idx(i32) + R,G,B,0
                if payload.len() >= 8
                    && let Ok(ibytes) = payload[0..4].try_into()
                {
                    let led_idx = i32::from_le_bytes(ibytes);
                    if led_idx >= 0
                        && let Ok(mut guard) = state.lock()
                        && let Some(ctrl) = guard.controllers.get_mut(dev_idx as usize)
                        && let Some(slot) = ctrl.colors.get_mut(led_idx as usize)
                    {
                        *slot = Rgb::new(payload[4], payload[5], payload[6]);
                        let _ = tx.try_send(OpenRgbServerEvent::UpdateLeds {
                            device_id: ctrl.device_id.clone(),
                            colors: ctrl.colors.clone(),
                        });
                    }
                }
            }
            PKT_RESIZE_ZONE | PKT_UPDATE_MODE => {}
            other => {
                debug!(pkt_id = other, dev_idx, "Paquet OpenRGB SDK ignoré");
            }
        }
    }
}

fn apply_led_packet(
    state: &Arc<Mutex<SharedState>>,
    dev_idx: usize,
    zone_idx: Option<usize>,
    slice: &[u8],
) -> Option<OpenRgbServerEvent> {
    // `UPDATE_LEDS`: data_size(4) + count(2) + colors; `UPDATE_ZONE_LEDS` tail passed here starts at count(2).
    let (count_off, data_off) = if zone_idx.is_none() { (4, 6) } else { (0, 2) };
    let count_bytes: [u8; 2] = slice.get(count_off..count_off + 2)?.try_into().ok()?;
    let count = u16::from_le_bytes(count_bytes) as usize;
    let color_bytes = slice.get(data_off..)?;
    let n = count.min(color_bytes.len() / 4);

    let mut guard = state.lock().ok()?;
    let ctrl = guard.controllers.get_mut(dev_idx)?;
    let offset = match zone_idx {
        Some(zi) => ctrl.zones.iter().take(zi).map(|z| z.leds.len()).sum(),
        None => 0,
    };
    let max_len = ctrl.colors.len().saturating_sub(offset);
    for (i, slot) in ctrl.colors[offset..offset + n.min(max_len)]
        .iter_mut()
        .enumerate()
    {
        let b = &color_bytes[i * 4..i * 4 + 4];
        *slot = Rgb::new(b[0], b[1], b[2]);
    }
    Some(OpenRgbServerEvent::UpdateLeds {
        device_id: ctrl.device_id.clone(),
        colors: ctrl.colors.clone(),
    })
}

fn send_packet(
    stream: &mut TcpStream,
    dev_idx: u32,
    pkt_id: u32,
    payload: &[u8],
) -> std::io::Result<()> {
    let mut hdr = [0u8; 16];
    hdr[0..4].copy_from_slice(b"ORGB");
    hdr[4..8].copy_from_slice(&dev_idx.to_le_bytes());
    hdr[8..12].copy_from_slice(&pkt_id.to_le_bytes());
    hdr[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    stream.write_all(&hdr)?;
    if !payload.is_empty() {
        stream.write_all(payload)?;
    }
    Ok(())
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    out.extend_from_slice(&(bytes.len() as u16 + 1).to_le_bytes());
    out.extend_from_slice(bytes);
    out.push(0);
}

fn openrgb_type(kind: DeviceKind) -> u32 {
    match kind {
        DeviceKind::Motherboard => 0,
        DeviceKind::Memory => 1,
        DeviceKind::Cooler => 3,
        DeviceKind::LedController => 4,
        DeviceKind::Keyboard | DeviceKind::Laptop => 5,
        DeviceKind::Mouse => 6,
        DeviceKind::Mousepad => 7,
        DeviceKind::Headset => 8,
        DeviceKind::Controller | DeviceKind::Display | DeviceKind::Other => 14,
    }
}

/// Encode `SdkController` in the OpenRGB wire payload format (parsed cleanly by `parse_controller`).
pub fn encode_controller_data(c: &SdkController) -> Vec<u8> {
    let mut out = vec![0u8; 4]; // placeholder for total size
    out.extend_from_slice(&openrgb_type(c.kind).to_le_bytes());
    push_str(&mut out, &c.name);
    push_str(&mut out, &c.vendor);
    push_str(&mut out, "OpenGear unified lighting device");
    push_str(&mut out, &c.version);
    push_str(&mut out, ""); // never expose hardware serial over TCP
    push_str(&mut out, &c.device_id);

    // Modes: 1 mode ("Direct", PER_LED)
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // active_mode = 0
    push_str(&mut out, "Direct");
    // 12 * u32 fields: value, flags (PER_LED = 0x80), 9 zeros, color_mode = 1 (MODE_COLORS_PER_LED)
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0x80u32.to_le_bytes());
    for _ in 0..9 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // mode colors count

    // Zones
    out.extend_from_slice(&(c.zones.len() as u16).to_le_bytes());
    for z in &c.zones {
        push_str(&mut out, &z.name);
        let ztype = match z.kind {
            ZoneKind::Single => 0u32,
            ZoneKind::Linear => 1u32,
            ZoneKind::Matrix => 2u32,
        };
        let n = z.leds.len() as u32;
        let (min_l, max_l) = z
            .resizable
            .map(|(lo, hi)| (lo as u32, hi as u32))
            .unwrap_or((n, n));
        out.extend_from_slice(&ztype.to_le_bytes());
        out.extend_from_slice(&min_l.to_le_bytes());
        out.extend_from_slice(&max_l.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // matrix_len = 0
    }

    // LEDs
    let total = c.total_leds();
    out.extend_from_slice(&(total as u16).to_le_bytes());
    let mut led_idx = 0u32;
    for z in &c.zones {
        for led in &z.leds {
            push_str(&mut out, &led.name);
            out.extend_from_slice(&led_idx.to_le_bytes());
            led_idx += 1;
        }
    }

    // Current colors
    out.extend_from_slice(&(total as u16).to_le_bytes());
    for i in 0..total {
        let col = c.colors.get(i).copied().unwrap_or(Rgb::BLACK);
        out.extend_from_slice(&[col.r, col.g, col.b, 0]);
    }

    // Safety cap check cleanly
    let total_len = (out.len() as u32).min(MAX_PACKET_LEN);
    out[0..4].copy_from_slice(&total_len.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrgbClient, parse_controller};
    use opengear_core::driver::Connection;
    use opengear_core::lighting::Led;

    #[test]
    fn rejects_non_loopback_bind_address() {
        assert!(OpenRgbSdkServer::start("0.0.0.0:6742", Vec::new()).is_err());
    }

    #[test]
    fn sdk_server_and_client_roundtrip_controller_and_leds() {
        let info = DeviceInfo {
            id: "test:kb0".into(),
            vendor: "OpenGear".into(),
            model: "Test RGB Board".into(),
            kind: DeviceKind::Keyboard,
            connection: Connection::Usb,
            firmware: Some("1.2".into()),
        };
        let zone = Zone {
            id: "z0".into(),
            name: "Touches".into(),
            kind: ZoneKind::Linear,
            leds: vec![Led::at("A", 0.0, 0.5), Led::at("B", 1.0, 0.5)],
            resizable: None,
        };
        let ctrl = SdkController::from_device(&info, vec![zone]);
        let server = OpenRgbSdkServer::start("127.0.0.1:0", vec![ctrl]).unwrap();
        let addr = server.local_addr().to_string();

        let mut client = OrgbClient::connect_addr(&addr).expect("loopback connect");
        assert_eq!(client.controller_count(), Some(1));
        let raw = client.controller_data(0).expect("controller data");
        let parsed = parse_controller(&raw).expect("valid ORGB wire payload");
        assert_eq!(parsed.name, "Test RGB Board");
        assert_eq!(parsed.vendor, "OpenGear");
        assert_eq!(parsed.total_leds, 2);

        client.set_custom_mode(0).unwrap();
        client.update_leds(0, &[Rgb::RED, Rgb::CYAN]).unwrap();

        // Wait briefly for the server client thread to queue the frame.
        let mut events = Vec::new();
        for _ in 0..20 {
            events.extend(server.poll_events());
            if events
                .iter()
                .any(|e| matches!(e, OpenRgbServerEvent::UpdateLeds { .. }))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(events.iter().any(|e| {
            matches!(
                e,
                OpenRgbServerEvent::UpdateLeds { device_id, colors }
                    if device_id == "test:kb0" && colors == &[Rgb::RED, Rgb::CYAN]
            )
        }));
    }
}
