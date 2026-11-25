//! Wire protocol for Razer HID devices (`docs/protocols/razer.md`).
//!
//! Covers:
//! - Generic 90-byte HID feature reports (`Standard` class `0x03`, `Extended` class `0x0F`,
//!   `Linear`, `ExtendedArgb` commands, `Custom` models, class `0x00` info queries, wireless
//!   receiver slot enumeration `0x00/0xBF`).
//! - Addressable RGB Controller (`1532:0F1F`) 320-byte feature report frames on interface 1.
//! - Kraken 1st generation 37-byte output/input reports (`0x04` / `0x05`).
//!
//! Safety invariants:
//! - Storage byte is ALWAYS `VARSTORE_NONE` (`0x00`). Volatile updates only; no flash writes.
//! - Checksum in 90-byte reports is XOR over payload offsets `2..=87`.

use anyhow::{Result, bail};
use opengear_core::Rgb;
use std::thread;
use std::time::Duration;

/// USB Vendor ID shared by Razer HID devices.
pub const VID_RAZER: u16 = 0x1532;

/// Inter-application mutex used on Windows to serialize feature report transfers.
pub const RAZER_MUTEX: &str = r"Global\RazerLinkReadWriteGuardMutex";

/// Volatile-only storage byte (no persistent flash write).
pub const VARSTORE_NONE: u8 = 0x00;

/// 90-byte generic payload length (excluding leading HID report ID byte).
pub const REPORT90_PAYLOAD_LEN: usize = 90;
/// 91-byte hidapi feature buffer length (report ID + 90-byte payload).
pub const REPORT90_WIRE_LEN: usize = 91;
/// Maximum argument bytes inside a 90-byte report (`offsets 8..=87`).
pub const REPORT90_MAX_ARGS: usize = 80;

/// 320-byte ARGB frame payload length (excluding leading HID report ID byte).
pub const ARGB_PAYLOAD_LEN: usize = 320;
/// 321-byte hidapi feature buffer length for ARGB channel frames.
pub const ARGB_WIRE_LEN: usize = 321;
/// Maximum LEDs per ARGB controller channel supported by the wire protocol.
pub const ARGB_CHANNEL_LEDS: usize = 80;

/// 37-byte Kraken 1st-gen output/input report length (including report ID `0x04` / `0x05`).
pub const KRAKEN_REPORT_LEN: usize = 37;

/// Response status codes inside a 90-byte Razer report (`payload[0]`).
pub mod status {
    pub const NEW: u8 = 0x00;
    pub const BUSY: u8 = 0x01;
    pub const SUCCESS: u8 = 0x02;
    pub const FAILURE: u8 = 0x03;
    pub const TIMEOUT: u8 = 0x04;
    pub const NOT_SUPPORTED: u8 = 0x05;
}

/// Logical LED IDs passed to Razer effect and brightness commands.
pub mod led_id {
    pub const ZERO: u8 = 0x00;
    pub const SCROLL_WHEEL: u8 = 0x01;
    pub const BATTERY: u8 = 0x03;
    pub const LOGO: u8 = 0x04;
    pub const BACKLIGHT: u8 = 0x05;
    pub const MACRO: u8 = 0x07;
    pub const GAME_MODE: u8 = 0x08;
    pub const RIGHT_SIDE: u8 = 0x10;
    pub const LEFT_SIDE: u8 = 0x11;
    pub const ARGB_BASE: u8 = 0x1A; // 0x1A..=0x1F for channels 1..=6
}

/// Direction values for Razer wave effects (`Extended` and `Standard`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveDirection {
    Right = 1,
    Left = 2,
}

impl WaveDirection {
    pub fn from_reverse(reverse: bool) -> Self {
        if reverse { Self::Left } else { Self::Right }
    }
}

/// Reactive / Starlight duration (`1 = short`, `2 = medium`, `3 = long`).
pub fn duration_from_speed(speed: Option<u8>) -> u8 {
    match speed.unwrap_or(2) {
        0 | 1 => 1,
        2 => 2,
        _ => 3,
    }
}

/// Abstraction over HID I/O so every protocol path can be unit tested deterministically.
pub trait RazerIo: Send {
    fn send_feature_report(&mut self, data: &[u8]) -> Result<()>;
    fn get_feature_report(&mut self, buf: &mut [u8]) -> Result<usize>;
    fn write(&mut self, data: &[u8]) -> Result<usize>;
    fn read_timeout(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize>;
    fn sleep(&self, dur: Duration);
}

/// Real `hidapi` transport guarded by the cross-app Razer mutex on Windows.
pub struct HidRazerIo {
    dev: hidapi::HidDevice,
}

impl HidRazerIo {
    pub fn new(dev: hidapi::HidDevice) -> Self {
        Self { dev }
    }
}

impl RazerIo for HidRazerIo {
    fn send_feature_report(&mut self, data: &[u8]) -> Result<()> {
        let _guard = opengear_platform::named_mutex::NamedMutexGuard::acquire(RAZER_MUTEX, 1500);
        self.dev
            .send_feature_report(data)
            .map_err(|e| anyhow::anyhow!("HID SendFeatureReport Razer: {e}"))
    }

    fn get_feature_report(&mut self, buf: &mut [u8]) -> Result<usize> {
        let _guard = opengear_platform::named_mutex::NamedMutexGuard::acquire(RAZER_MUTEX, 1500);
        self.dev
            .get_feature_report(buf)
            .map_err(|e| anyhow::anyhow!("HID GetFeatureReport Razer: {e}"))
    }

    fn write(&mut self, data: &[u8]) -> Result<usize> {
        self.dev
            .write(data)
            .map_err(|e| anyhow::anyhow!("HID Write Razer: {e}"))
    }

    fn read_timeout(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
        self.dev
            .read_timeout(buf, timeout_ms)
            .map_err(|e| anyhow::anyhow!("HID Read Razer: {e}"))
    }

    fn sleep(&self, dur: Duration) {
        if !dur.is_zero() {
            thread::sleep(dur);
        }
    }
}

/// A 90-byte generic Razer command/response payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RzReport {
    pub status: u8,
    pub tx_id: u8,
    pub data_size: u8,
    pub cmd_class: u8,
    pub cmd_id: u8,
    pub args: [u8; REPORT90_MAX_ARGS],
}

impl RzReport {
    pub fn new(tx_id: u8, data_size: u8, cmd_class: u8, cmd_id: u8) -> Self {
        Self {
            status: status::NEW,
            tx_id,
            data_size,
            cmd_class,
            cmd_id,
            args: [0u8; REPORT90_MAX_ARGS],
        }
    }

    pub fn with_args(mut self, slice: &[u8]) -> Self {
        let n = slice.len().min(REPORT90_MAX_ARGS);
        self.args[..n].copy_from_slice(&slice[..n]);
        self
    }

    /// Compute XOR checksum over payload bytes `2..=87` (remaining_packets..=args).
    pub fn compute_checksum(payload: &[u8; REPORT90_PAYLOAD_LEN]) -> u8 {
        payload[2..=87].iter().fold(0u8, |acc, &b| acc ^ b)
    }

    /// Serialize into a 91-byte HID feature report (`[report_id, 90B payload...]`).
    pub fn encode(&self, report_id: u8) -> [u8; REPORT90_WIRE_LEN] {
        let mut payload = [0u8; REPORT90_PAYLOAD_LEN];
        payload[0] = self.status;
        payload[1] = self.tx_id;
        // payload[2..=3] remaining_packets = 0x0000
        // payload[4] protocol_type = 0x00
        payload[5] = self.data_size;
        payload[6] = self.cmd_class;
        payload[7] = self.cmd_id;
        payload[8..88].copy_from_slice(&self.args);
        payload[88] = Self::compute_checksum(&payload);
        payload[89] = 0x00;

        let mut wire = [0u8; REPORT90_WIRE_LEN];
        wire[0] = report_id;
        wire[1..].copy_from_slice(&payload);
        wire
    }

    /// Parse a 91-byte HID feature response (with leading report ID) or 90-byte payload.
    pub fn decode(raw: &[u8]) -> Result<Self> {
        let payload: &[u8; REPORT90_PAYLOAD_LEN] = if raw.len() >= REPORT90_WIRE_LEN {
            raw[1..REPORT90_WIRE_LEN]
                .try_into()
                .map_err(|_| anyhow::anyhow!("Trame Razer invalide"))?
        } else if raw.len() == REPORT90_PAYLOAD_LEN {
            raw.try_into()
                .map_err(|_| anyhow::anyhow!("Trame Razer invalide"))?
        } else {
            bail!(
                "Réponse Razer trop courte : {} octet(s) reçus (90 ou 91 attendus)",
                raw.len()
            );
        };

        // The sheet does not establish that responses carry a valid checksum: do not reject a
        // response on it (requests always carry one, see `encode`).

        let mut args = [0u8; REPORT90_MAX_ARGS];
        args.copy_from_slice(&payload[8..88]);
        Ok(Self {
            status: payload[0],
            tx_id: payload[1],
            data_size: payload[5],
            cmd_class: payload[6],
            cmd_id: payload[7],
            args,
        })
    }
}

/// Send a 90-byte command and reliably poll for its response (`docs/protocols/razer.md` §2.3).
///
/// - Normal devices: up to 3 attempts, each polling up to 25 reads spaced by 2 ms.
/// - Wireless receiver relayed devices: active wait of up to 50 reads spaced by 2 ms per attempt
///   until `status != NEW && status != BUSY`.
pub fn transfer_reliable(
    io: &mut dyn RazerIo,
    report_id: u8,
    req: &RzReport,
    wireless_relay: bool,
) -> Result<RzReport> {
    let wire = req.encode(report_id);
    let max_reads = if wireless_relay { 50 } else { 25 };

    for _attempt in 0..3 {
        io.sleep(Duration::from_millis(2));
        io.send_feature_report(&wire)?;

        for _poll in 0..max_reads {
            io.sleep(Duration::from_millis(2));
            let mut buf = [0u8; REPORT90_WIRE_LEN];
            buf[0] = report_id;
            let n = io.get_feature_report(&mut buf)?;
            if let Ok(resp) = RzReport::decode(&buf[..n.max(REPORT90_WIRE_LEN)])
                && resp.tx_id == req.tx_id
                && resp.cmd_class == req.cmd_class
                && resp.cmd_id == req.cmd_id
                && resp.status != status::NEW
                && resp.status != status::BUSY
            {
                return Ok(resp);
            }
        }
    }

    bail!(
        "Aucune réponse Razer valide (classe {:#04X}, cmd {:#04X}, tx {:#04X})",
        req.cmd_class,
        req.cmd_id,
        req.tx_id
    )
}

/// Send a 90-byte command that does not expect returned payload, waiting if relayed wirelessly.
pub fn send_command(
    io: &mut dyn RazerIo,
    report_id: u8,
    req: &RzReport,
    wireless_relay: bool,
) -> Result<()> {
    if wireless_relay {
        // Relayed devices require active wait so the dongle does not drop subsequent packets.
        let resp = transfer_reliable(io, report_id, req, true)?;
        if resp.status == status::FAILURE {
            bail!(
                "Échec Razer sur commande {:#04X}/{:#04X} (statut {:#04X})",
                req.cmd_class,
                req.cmd_id,
                resp.status
            );
        }
        return Ok(());
    }
    let wire = req.encode(report_id);
    io.send_feature_report(&wire)
}

// ---------- Class 0x00 info & wireless receiver commands (§2.4 & §2.8) ----------

pub fn cmd_get_firmware(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x02, 0x00, 0x81)
}

pub fn parse_firmware(resp: &RzReport) -> String {
    format!("v{}.{}", resp.args[0], resp.args[1])
}

pub fn cmd_get_serial(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x16, 0x00, 0x82)
}

pub fn parse_serial(resp: &RzReport) -> String {
    resp.args[..22]
        .iter()
        .map(|&b| {
            if (0x1E..=0x7E).contains(&b) {
                b as char
            } else {
                ' '
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn cmd_get_device_mode(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x02, 0x00, 0x84)
}

/// Mode write (`0x00/0x04`). Caller must restrict software mode (`0x03`) to PIDs `0F26/0F07/0068`.
pub fn cmd_set_device_mode(tx_id: u8, mode: u8) -> RzReport {
    RzReport::new(tx_id, 0x02, 0x00, 0x04).with_args(&[mode, 0x00])
}

pub fn cmd_get_keyboard_info(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x00, 0x00, 0x86)
}

/// Physical keyboard layout class inferred from `0x00/0x86` `arg0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalLayout {
    Ansi,
    Iso,
    Jis,
}

pub fn parse_keyboard_layout(layout_code: u8) -> PhysicalLayout {
    match layout_code {
        1 | 5 | 8 | 10 | 11 | 20 => PhysicalLayout::Ansi,
        12 => PhysicalLayout::Jis,
        // All European ISO/QWERTZ/AZERTY and regional ISO layout codes (2..=4, 6, 7, 9, 13..=19).
        2..=4 | 6 | 7 | 9 | 13..=19 => PhysicalLayout::Iso,
        _ => PhysicalLayout::Ansi,
    }
}

pub fn cmd_get_wireless_paired(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x50, 0x00, 0xBF)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairedWirelessDevice {
    pub connected: bool,
    pub pid: u16,
}

/// Parse `0x00/0xBF` wireless receiver slot records (`[count, (connected, pid_hi, pid_lo)*]`).
pub fn parse_wireless_paired(resp: &RzReport) -> Vec<PairedWirelessDevice> {
    if resp.status != status::SUCCESS {
        return Vec::new();
    }
    let count = (resp.args[0] as usize).min((REPORT90_MAX_ARGS - 1) / 3);
    let mut out = Vec::new();
    for i in 0..count {
        let base = 1 + 3 * i;
        let connected = resp.args[base] == 1;
        let pid = u16::from_be_bytes([resp.args[base + 1], resp.args[base + 2]]);
        if pid != 0xFFFF && pid != 0x0000 {
            out.push(PairedWirelessDevice { connected, pid });
        }
    }
    out
}

// ---------- Extended Matrix (Class 0x0F) builders (§2.6) ----------

pub fn cmd_ext_frame_row(tx_id: u8, row: u8, start_col: u8, colors: &[Rgb]) -> RzReport {
    debug_assert!(!colors.is_empty() && colors.len() <= 25);
    let n = colors.len().min(25);
    let end_col = start_col + (n as u8) - 1;
    let size = (5 + 3 * n) as u8;
    let mut r = RzReport::new(tx_id, size, 0x0F, 0x03);
    r.args[0] = 0x00;
    r.args[1] = 0x00;
    r.args[2] = row;
    r.args[3] = start_col;
    r.args[4] = end_col;
    for (i, rgb) in colors.iter().take(n).enumerate() {
        let off = 5 + i * 3;
        r.args[off] = rgb.r;
        r.args[off + 1] = rgb.g;
        r.args[off + 2] = rgb.b;
    }
    r
}

pub fn cmd_ext_enable_custom_frame(tx_id: u8) -> RzReport {
    // §2.6: LED = 0x00 always when activating custom frame (`a0=0, a1=0, a2=0x08`).
    RzReport::new(tx_id, 0x0C, 0x0F, 0x02).with_args(&[VARSTORE_NONE, led_id::ZERO, 0x08])
}

pub fn cmd_ext_off(tx_id: u8, led: u8) -> RzReport {
    RzReport::new(tx_id, 0x06, 0x0F, 0x02).with_args(&[VARSTORE_NONE, led, 0x00, 0, 0, 0])
}

pub fn cmd_ext_static(tx_id: u8, led: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x09, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x01,
        0x00,
        0x00,
        0x01,
        c.r,
        c.g,
        c.b,
    ])
}

pub fn cmd_ext_breath_random(tx_id: u8, led: u8) -> RzReport {
    RzReport::new(tx_id, 0x06, 0x0F, 0x02).with_args(&[VARSTORE_NONE, led, 0x02, 0x00, 0x00, 0x00])
}

pub fn cmd_ext_breath_1(tx_id: u8, led: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x09, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x02,
        0x01,
        0x00,
        0x01,
        c.r,
        c.g,
        c.b,
    ])
}

pub fn cmd_ext_breath_2(tx_id: u8, led: u8, c1: Rgb, c2: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x0C, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x02,
        0x02,
        0x00,
        0x02,
        c1.r,
        c1.g,
        c1.b,
        c2.r,
        c2.g,
        c2.b,
    ])
}

pub fn cmd_ext_spectrum(tx_id: u8, led: u8) -> RzReport {
    RzReport::new(tx_id, 0x06, 0x0F, 0x02).with_args(&[VARSTORE_NONE, led, 0x03, 0x00, 0x00, 0x00])
}

pub fn cmd_ext_wave(tx_id: u8, led: u8, dir: WaveDirection) -> RzReport {
    RzReport::new(tx_id, 0x06, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x04,
        dir as u8,
        0x28,
        0x00,
    ])
}

pub fn cmd_ext_reactive(tx_id: u8, led: u8, duration: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x09, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x05,
        0x00,
        duration.clamp(1, 3),
        0x01,
        c.r,
        c.g,
        c.b,
    ])
}

pub fn cmd_ext_ripple(tx_id: u8, led: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x09, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x06,
        0x00,
        0x1E,
        0x01,
        c.r,
        c.g,
        c.b,
    ])
}

pub fn cmd_ext_starlight_random(tx_id: u8, led: u8, duration: u8) -> RzReport {
    RzReport::new(tx_id, 0x06, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x07,
        0x00,
        duration.clamp(1, 3),
        0x00,
    ])
}

pub fn cmd_ext_starlight_1(tx_id: u8, led: u8, duration: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x09, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x07,
        0x01,
        duration.clamp(1, 3),
        0x01,
        c.r,
        c.g,
        c.b,
    ])
}

pub fn cmd_ext_starlight_2(tx_id: u8, led: u8, duration: u8, c1: Rgb, c2: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x0C, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x07,
        0x02,
        duration.clamp(1, 3),
        0x02,
        c1.r,
        c1.g,
        c1.b,
        c2.r,
        c2.g,
        c2.b,
    ])
}

pub fn cmd_ext_fire(tx_id: u8, led: u8) -> RzReport {
    // §2.6: firmware rejects fire without color; reference software sends red.
    RzReport::new(tx_id, 0x09, 0x0F, 0x02).with_args(&[
        VARSTORE_NONE,
        led,
        0x09,
        0x00,
        0x00,
        0x01,
        0xFF,
        0x00,
        0x00,
    ])
}

pub fn cmd_ext_brightness(tx_id: u8, led: u8, brightness: u8) -> RzReport {
    RzReport::new(tx_id, 0x03, 0x0F, 0x04).with_args(&[VARSTORE_NONE, led, brightness])
}

pub fn cmd_ext_list_regions(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x50, 0x0F, 0x80)
}

pub fn parse_ext_regions(resp: &RzReport) -> Vec<u8> {
    if resp.status != status::SUCCESS {
        return Vec::new();
    }
    let mut regions = Vec::new();
    for chunk in resp.args.as_chunks::<5>().0 {
        if chunk[0] == 0x00 {
            break;
        }
        regions.push(chunk[0]);
    }
    regions
}

pub fn cmd_ext_list_effects(tx_id: u8, led: u8) -> RzReport {
    RzReport::new(tx_id, 0x50, 0x0F, 0x81).with_args(&[led])
}

pub fn parse_ext_effects(resp: &RzReport) -> Vec<u8> {
    if resp.status != status::SUCCESS {
        return Vec::new();
    }
    let end = (resp.data_size as usize).min(REPORT90_MAX_ARGS);
    if end <= 1 {
        return Vec::new();
    }
    resp.args[1..end].to_vec()
}

pub fn cmd_ext_argb_sizes(tx_id: u8, sizes: &[u8; 6]) -> RzReport {
    let mut args = [0u8; 13];
    args[0] = 0x06;
    for (i, &size) in sizes.iter().enumerate() {
        let sz = size.min(ARGB_CHANNEL_LEDS as u8);
        args[1 + 2 * i] = if sz > 0 { 0x19 } else { (i + 1) as u8 };
        args[1 + 2 * i + 1] = sz;
    }
    RzReport::new(tx_id, 0x0D, 0x0F, 0x08).with_args(&args)
}

// ---------- Standard / Linear Matrix (Class 0x03) builders (§2.7) ----------

pub fn cmd_std_frame_row(tx_id: u8, row: u8, start_col: u8, colors: &[Rgb]) -> RzReport {
    debug_assert!(!colors.is_empty() && colors.len() <= 25);
    let n = colors.len().min(25);
    let end_col = start_col + (n as u8) - 1;
    let size = (4 + 3 * n) as u8;
    let mut r = RzReport::new(tx_id, size, 0x03, 0x0B);
    r.args[0] = 0xFF;
    r.args[1] = row;
    r.args[2] = start_col;
    r.args[3] = end_col;
    for (i, rgb) in colors.iter().take(n).enumerate() {
        let off = 4 + i * 3;
        r.args[off] = rgb.r;
        r.args[off + 1] = rgb.g;
        r.args[off + 2] = rgb.b;
    }
    r
}

pub fn cmd_linear_frame_row(tx_id: u8, start_col: u8, colors: &[Rgb]) -> RzReport {
    // §2.7: fixed data_size = 0x32 (max 25 triplets cleanly fit inside 80 args after 2 header bytes).
    debug_assert!(!colors.is_empty() && colors.len() <= 25);
    let n = colors.len().min(25);
    let end_col = start_col + (n as u8) - 1;
    let mut r = RzReport::new(tx_id, 0x32, 0x03, 0x0C);
    r.args[0] = start_col;
    r.args[1] = end_col;
    for (i, rgb) in colors.iter().take(n).enumerate() {
        let off = 2 + i * 3;
        r.args[off] = rgb.r;
        r.args[off + 1] = rgb.g;
        r.args[off + 2] = rgb.b;
    }
    r
}

pub fn cmd_std_brightness(tx_id: u8, led: u8, brightness: u8) -> RzReport {
    RzReport::new(tx_id, 0x03, 0x03, 0x03).with_args(&[VARSTORE_NONE, led, brightness])
}

pub fn cmd_std_led_color(tx_id: u8, led: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x05, 0x03, 0x01).with_args(&[VARSTORE_NONE, led, c.r, c.g, c.b])
}

/// Legacy single-LED effect (`0 = static`, `2 = breath`, `4 = spectrum`, capped at `5`).
pub fn cmd_std_led_effect(tx_id: u8, led: u8, effect: u8) -> RzReport {
    RzReport::new(tx_id, 0x03, 0x03, 0x02).with_args(&[VARSTORE_NONE, led, effect.min(5)])
}

pub fn cmd_std_matrix_enable_frame(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x02, 0x03, 0x0A).with_args(&[0x05, VARSTORE_NONE])
}

pub fn cmd_std_matrix_off(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x01, 0x03, 0x0A).with_args(&[0x00])
}

pub fn cmd_std_matrix_wave(tx_id: u8, dir: WaveDirection) -> RzReport {
    RzReport::new(tx_id, 0x02, 0x03, 0x0A).with_args(&[0x01, dir as u8])
}

pub fn cmd_std_matrix_breath_1(tx_id: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x08, 0x03, 0x0A).with_args(&[0x03, 0x01, c.r, c.g, c.b])
}

pub fn cmd_std_matrix_breath_2(tx_id: u8, c1: Rgb, c2: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x08, 0x03, 0x0A)
        .with_args(&[0x03, 0x02, c1.r, c1.g, c1.b, c2.r, c2.g, c2.b])
}

pub fn cmd_std_matrix_breath_random(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x08, 0x03, 0x0A).with_args(&[0x03, 0x03])
}

pub fn cmd_std_matrix_spectrum(tx_id: u8) -> RzReport {
    RzReport::new(tx_id, 0x01, 0x03, 0x0A).with_args(&[0x04])
}

pub fn cmd_std_matrix_static(tx_id: u8, c: Rgb) -> RzReport {
    RzReport::new(tx_id, 0x04, 0x03, 0x0A).with_args(&[0x06, c.r, c.g, c.b])
}

// ---------- ARGB Controller 320-byte frame report (§6) ----------

/// Build the 321-byte feature report (`0x00` report ID + 320-byte ARGB frame) for channel `0..=5`.
pub fn build_argb_channel_report(channel: u8, leds: &[Rgb]) -> [u8; ARGB_WIRE_LEN] {
    let mut wire = [0u8; ARGB_WIRE_LEN];
    wire[0] = 0x00; // HID report ID
    wire[1] = if channel >= 5 { 0x84 } else { 0x04 };
    wire[2] = channel;
    wire[3] = channel;
    wire[4] = 0x00;
    wire[5] = (ARGB_CHANNEL_LEDS - 1) as u8; // always 79 per protocol note
    for (i, rgb) in leds.iter().take(ARGB_CHANNEL_LEDS).enumerate() {
        let off = 6 + i * 3;
        wire[off] = rgb.r;
        wire[off + 1] = rgb.g;
        wire[off + 2] = rgb.b;
    }
    wire
}

// ---------- Kraken 1st Generation 37-byte Output/Input reports (§7) ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KrakenAddresses {
    pub mode: u16,
    pub custom: u16,
    pub breath_1: u16,
    pub breath_2: Option<u16>,
    pub breath_3: Option<u16>,
}

pub const KRAKEN_CHROMA_ADDRS: KrakenAddresses = KrakenAddresses {
    mode: 0x1008,
    custom: 0x1189,
    breath_1: 0x15DE,
    breath_2: None,
    breath_3: None,
};

pub const KRAKEN_V2_ADDRS: KrakenAddresses = KrakenAddresses {
    mode: 0x172D,
    custom: 0x1189,
    breath_1: 0x1741,
    breath_2: Some(0x1745),
    breath_3: Some(0x174D),
};

pub mod kraken_mode_bits {
    pub const OFF: u8 = 0x00;
    pub const ON_STATIC: u8 = 0x01;
    pub const SPECTRUM: u8 = 0x05; // bits 0, 2
    pub const BREATH_1: u8 = 0x0B; // bits 0, 1, 3
    pub const BREATH_2: u8 = 0x19; // bits 0, 3, 4
    pub const BREATH_3: u8 = 0x29; // bits 0, 3, 5
}

pub fn kraken_write_report(addr: u16, data: &[u8]) -> [u8; KRAKEN_REPORT_LEN] {
    let mut buf = [0u8; KRAKEN_REPORT_LEN];
    let len = data.len().min(32);
    buf[0] = 0x04;
    buf[1] = 0x40;
    buf[2] = len as u8;
    buf[3] = (addr >> 8) as u8;
    buf[4] = (addr & 0xFF) as u8;
    buf[5..5 + len].copy_from_slice(&data[..len]);
    buf
}

pub fn kraken_read_report(addr: u16, len: u8) -> [u8; KRAKEN_REPORT_LEN] {
    let mut buf = [0u8; KRAKEN_REPORT_LEN];
    buf[0] = 0x04;
    buf[1] = 0x20;
    buf[2] = len.min(32);
    buf[3] = (addr >> 8) as u8;
    buf[4] = (addr & 0xFF) as u8;
    buf
}

pub fn kraken_read_firmware(io: &mut dyn RazerIo) -> Result<Option<String>> {
    io.write(&kraken_read_report(0x0030, 2))?;
    let mut buf = [0u8; KRAKEN_REPORT_LEN];
    let n = io.read_timeout(&mut buf, 100)?;
    if n >= 4 && buf[0] == 0x05 {
        Ok(Some(format!("v{}.{}", buf[2], buf[3])))
    } else {
        Ok(None)
    }
}

pub fn kraken_read_serial(io: &mut dyn RazerIo) -> Result<Option<String>> {
    io.write(&kraken_read_report(0x7F00, 0x16))?;
    let mut buf = [0u8; KRAKEN_REPORT_LEN];
    let n = io.read_timeout(&mut buf, 100)?;
    if n >= 23 && buf[0] == 0x05 {
        let s: String = buf[1..23]
            .iter()
            .map(|&b| {
                if (0x1E..=0x7E).contains(&b) {
                    b as char
                } else {
                    ' '
                }
            })
            .collect();
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed))
        }
    } else {
        Ok(None)
    }
}
