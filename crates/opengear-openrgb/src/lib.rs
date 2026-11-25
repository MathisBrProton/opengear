//! Bridge to a running OpenRGB through its network SDK protocol (`127.0.0.1:6742`).
//!
//! Own implementation of the documented wire protocol: no OpenRGB (GPL) code is used. OpenRGB stays
//! a separate program the user runs; OpenGear only talks to its SDK server, which works without
//! administrator rights.

use anyhow::{Context, Result};
use opengear_core::Rgb;
use opengear_core::driver::{Connection, Device, DeviceInfo, DeviceKind, Driver};
use opengear_core::lighting::{Led, Lighting, Zone, ZoneKind};
use opengear_mainboard::ram::{EneRamMode, EneRamModule, RamEffect, RamProbe, RamRgbBackend};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const SDK_ADDR: &str = "127.0.0.1:6742";
const MAX_PACKET_LEN: u32 = 262_144;
const TAG: &str = "openrgb_sdk";

const PKT_REQUEST_CONTROLLER_COUNT: u32 = 0;
const PKT_REQUEST_CONTROLLER_DATA: u32 = 1;
const PKT_SET_CLIENT_NAME: u32 = 50;
const PKT_UPDATE_LEDS: u32 = 1050;
const PKT_SET_CUSTOM_MODE: u32 = 1100;
/// `device_type` of DRAM controllers in OpenRGB.
const DEVICE_TYPE_DRAM: u32 = 1;

/// Parse a 16-byte packet header: `(device index, packet id, payload length)`.
pub fn parse_header(hdr: &[u8; 16]) -> std::io::Result<(u32, u32, usize)> {
    if &hdr[0..4] != b"ORGB" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid ORGB magic",
        ));
    }
    let word = |i: usize| u32::from_le_bytes([hdr[i], hdr[i + 1], hdr[i + 2], hdr[i + 3]]);
    let len = word(12);
    if len > MAX_PACKET_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Oversized OpenRGB packet ({len} bytes)"),
        ));
    }
    Ok((word(4), word(8), len as usize))
}

/// One SDK connection.
pub struct OrgbClient {
    stream: TcpStream,
}

impl OrgbClient {
    /// Connect quickly (the server is local or absent) and announce the client name.
    pub fn connect() -> Option<Self> {
        let addr: SocketAddr = SDK_ADDR.parse().ok()?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(120)).ok()?;
        let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
        let mut client = Self { stream };
        let _ = client.send(0, PKT_SET_CLIENT_NAME, b"OpenGear\0");
        Some(client)
    }

    fn send(&mut self, dev_idx: u32, pkt_id: u32, payload: &[u8]) -> std::io::Result<()> {
        let mut hdr = [0u8; 16];
        hdr[0..4].copy_from_slice(b"ORGB");
        hdr[4..8].copy_from_slice(&dev_idx.to_le_bytes());
        hdr[8..12].copy_from_slice(&pkt_id.to_le_bytes());
        hdr[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        self.stream.write_all(&hdr)?;
        if !payload.is_empty() {
            self.stream.write_all(payload)?;
        }
        Ok(())
    }

    fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        let mut hdr = [0u8; 16];
        self.stream.read_exact(&mut hdr)?;
        let (_, _, len) = parse_header(&hdr)?;
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn controller_count(&mut self) -> Option<u32> {
        self.send(0, PKT_REQUEST_CONTROLLER_COUNT, &[]).ok()?;
        let buf = self.recv().ok()?;
        Some(u32::from_le_bytes(buf.get(0..4)?.try_into().ok()?).min(32))
    }

    fn controller_data(&mut self, idx: u32) -> Option<Vec<u8>> {
        self.send(idx, PKT_REQUEST_CONTROLLER_DATA, &[]).ok()?;
        self.recv().ok()
    }

    /// Put controller `idx` in custom (direct) mode and set every LED to `color`.
    pub fn set_all_leds(&mut self, idx: u32, led_count: usize, color: Rgb) -> Result<()> {
        let _ = self.send(idx, PKT_SET_CUSTOM_MODE, &[]);
        let colors = vec![color; led_count.clamp(1, 32)];
        self.update_leds(idx, &colors)
    }

    pub fn set_custom_mode(&mut self, idx: u32) -> Result<()> {
        self.send(idx, PKT_SET_CUSTOM_MODE, &[])
            .context("Échec passage en mode direct OpenRGB SDK")
    }

    /// Encode and transmit a full `PKT_UPDATE_LEDS` frame.
    pub fn update_leds(&mut self, idx: u32, colors: &[Rgb]) -> Result<()> {
        let payload = encode_update_leds(colors);
        self.send(idx, PKT_UPDATE_LEDS, &payload)
            .context("Échec envoi couleurs OpenRGB SDK")
    }
}

pub(crate) fn encode_update_leds(colors: &[Rgb]) -> Vec<u8> {
    let max_leds = ((MAX_PACKET_LEN as usize).saturating_sub(6) / 4).min(u16::MAX as usize);
    let n = colors.len().clamp(1, max_leds);
    let size = (6 + n * 4) as u32;
    let mut payload = Vec::with_capacity(size as usize);
    payload.extend_from_slice(&size.to_le_bytes());
    payload.extend_from_slice(&(n as u16).to_le_bytes());
    for i in 0..n {
        let c = colors.get(i).copied().unwrap_or(Rgb::BLACK);
        payload.extend_from_slice(&[c.r, c.g, c.b, 0]);
    }
    payload
}

/// Little-endian reader over a controller-data payload.
struct Reader<'a> {
    buf: &'a [u8],
    off: usize,
}

impl Reader<'_> {
    fn u16(&mut self) -> Option<u16> {
        let v = u16::from_le_bytes(self.buf.get(self.off..self.off + 2)?.try_into().ok()?);
        self.off += 2;
        Some(v)
    }

    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_le_bytes(self.buf.get(self.off..self.off + 4)?.try_into().ok()?);
        self.off += 4;
        Some(v)
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.off = self.off.checked_add(n)?;
        (self.off <= self.buf.len()).then_some(())
    }

    fn string(&mut self) -> Option<String> {
        let len = self.u16()? as usize;
        let s = self.buf.get(self.off..self.off + len)?;
        self.off += len;
        Some(
            String::from_utf8_lossy(s)
                .trim_end_matches('\0')
                .to_string(),
        )
    }
}

/// Parsed OpenRGB controller metadata and zones (beyond DRAM).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedController {
    pub dev_type: u32,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub zones: Vec<Zone>,
    pub total_leds: usize,
}

fn map_openrgb_device_kind(dev_type: u32) -> DeviceKind {
    match dev_type {
        0 => DeviceKind::Motherboard,
        1 => DeviceKind::Memory,
        2 => DeviceKind::Other, // GPU
        3 => DeviceKind::Cooler,
        4 | 11 | 15 => DeviceKind::LedController,
        5 | 18 => DeviceKind::Keyboard,
        6 => DeviceKind::Mouse,
        7 => DeviceKind::Mousepad,
        8 | 9 => DeviceKind::Headset,
        _ => DeviceKind::Other,
    }
}

/// Parse an OpenRGB controller-data payload into structured metadata and positioned LED zones.
pub fn parse_controller(data: &[u8]) -> Option<ParsedController> {
    let mut r = Reader { buf: data, off: 4 };
    let dev_type = r.u32()?;
    let name = r.string()?;
    let vendor = r.string().unwrap_or_default();
    let _description = r.string();
    let version = r.string().unwrap_or_default();
    let _serial = r.string();
    let _location = r.string();

    let modes = r.u16()? as usize;
    let _active_mode = r.u32()?;
    for _ in 0..modes.min(64) {
        let _mode_name = r.string()?;
        r.skip(4 * 12)?;
        let colors = r.u16()? as usize;
        r.skip(colors * 4)?;
    }

    let zone_count = r.u16()? as usize;
    let mut parsed_zones = Vec::new();
    for zi in 0..zone_count.min(32) {
        let zname = r.string()?;
        let ztype = r.u32()?; // 0 = Single, 1 = Linear, 2 = Matrix
        let _leds_min = r.u32()?;
        let _leds_max = r.u32()?;
        let leds_count = (r.u32()? as usize).min(2048);
        let matrix_size = r.u16()? as usize;
        let mut matrix_dims = None;
        if ztype == 2 && matrix_size >= 8 {
            let h = r.u32()? as usize;
            let w = r.u32()? as usize;
            let remaining = matrix_size - 8;
            let mut map = Vec::with_capacity(h.saturating_mul(w).min(2048));
            let entries = (remaining / 4).min(h.saturating_mul(w));
            for _ in 0..entries {
                map.push(r.u32()?);
            }
            if entries * 4 < remaining {
                r.skip(remaining - entries * 4)?;
            }
            matrix_dims = Some((h, w, map));
        } else if matrix_size > 0 {
            r.skip(matrix_size)?;
        }

        let zid = format!("orgb_z{zi}");
        let zone = if let Some((h, w, map)) = matrix_dims
            && h > 0
            && w > 0
            && leds_count > 0
        {
            let mut coords = vec![(0.5f32, 0.5f32); leds_count];
            for row in 0..h {
                for col in 0..w {
                    if let Some(&idx) = map.get(row * w + col)
                        && (idx as usize) < leds_count
                    {
                        let x = if w <= 1 {
                            0.5
                        } else {
                            col as f32 / (w - 1) as f32
                        };
                        let y = if h <= 1 {
                            0.5
                        } else {
                            row as f32 / (h - 1) as f32
                        };
                        coords[idx as usize] = (x, y);
                    }
                }
            }
            let leds = coords
                .into_iter()
                .enumerate()
                .map(|(i, (x, y))| Led::at(format!("{zname} {}", i + 1), x, y))
                .collect();
            Zone {
                id: zid,
                name: zname,
                kind: ZoneKind::Matrix,
                leds,
                resizable: None,
            }
        } else if ztype == 0 || leds_count <= 1 {
            Zone::single(
                &zid,
                &zname,
                0.5,
                (zi as f32 + 0.5) / zone_count.max(1) as f32,
            )
        } else {
            Zone::linear(
                &zid,
                &zname,
                leds_count,
                (zi as f32 + 0.5) / zone_count.max(1) as f32,
            )
        };
        if !zone.leds.is_empty() {
            parsed_zones.push(zone);
        }
    }

    let reported_leds = r.u16().map(|v| v as usize).unwrap_or(0);
    let zone_sum: usize = parsed_zones.iter().map(|z| z.leds.len()).sum();
    let total_leds = if zone_sum > 0 {
        zone_sum
    } else {
        reported_leds
    };
    if total_leds == 0 {
        return None;
    }
    if parsed_zones.is_empty() {
        parsed_zones.push(Zone::linear("orgb_z0", &name, total_leds.min(512), 0.5));
    }
    Some(ParsedController {
        dev_type,
        name,
        vendor,
        version,
        zones: parsed_zones,
        total_leds,
    })
}

/// Optional bridge exposing any non-DRAM OpenRGB SDK controller as an OpenGear `Device + Lighting`.
/// Marked `experimental` (opt-in) so users enable it when bridging MSI/Gigabyte/Lian Li/NZXT gear.
pub struct OpenRgbDriver;

impl Driver for OpenRgbDriver {
    fn name(&self) -> &'static str {
        "openrgb"
    }

    fn display_name(&self) -> &'static str {
        "OpenRGB SDK (pont réseau 127.0.0.1:6742)"
    }

    fn experimental(&self) -> bool {
        true
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        let Some(mut client) = OrgbClient::connect() else {
            return Vec::new();
        };
        let count = client.controller_count().unwrap_or(0);
        let mut out: Vec<Box<dyn Device>> = Vec::new();
        for idx in 0..count {
            let Some(data) = client.controller_data(idx) else {
                continue;
            };
            let Some(parsed) = parse_controller(&data) else {
                continue;
            };
            // DRAM controllers are already bridged cleanly by `OpenRgbRamBackend`.
            if parsed.dev_type == DEVICE_TYPE_DRAM {
                continue;
            }
            out.push(Box::new(OpenRgbBridgeDevice {
                idx,
                parsed,
                client: None,
                custom_mode_active: false,
            }));
        }
        out
    }
}

pub struct OpenRgbBridgeDevice {
    idx: u32,
    parsed: ParsedController,
    client: Option<OrgbClient>,
    custom_mode_active: bool,
}

impl OpenRgbBridgeDevice {
    fn ensure_client(&mut self) -> Result<&mut OrgbClient> {
        if self.client.is_none() {
            self.client = OrgbClient::connect();
            self.custom_mode_active = false;
        }
        self.client
            .as_mut()
            .context("Serveur OpenRGB SDK (127.0.0.1:6742) indisponible")
    }
}

impl Device for OpenRgbBridgeDevice {
    fn info(&self) -> DeviceInfo {
        let slug: String = self
            .parsed
            .name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect();
        DeviceInfo {
            id: format!("openrgb:{}:{}", self.idx, slug.to_ascii_lowercase()),
            vendor: if self.parsed.vendor.is_empty() {
                "OpenRGB".to_string()
            } else {
                self.parsed.vendor.clone()
            },
            model: self.parsed.name.clone(),
            kind: map_openrgb_device_kind(self.parsed.dev_type),
            connection: Connection::Usb,
            firmware: (!self.parsed.version.is_empty()).then(|| self.parsed.version.clone()),
        }
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        Some(self)
    }

    fn diagnostic_lines(&self) -> Vec<String> {
        vec![format!(
            "OpenRGB SDK contrôleur #{} : {} zone(s), {} LEDs",
            self.idx,
            self.parsed.zones.len(),
            self.parsed.total_leds
        )]
    }
}

impl Lighting for OpenRgbBridgeDevice {
    fn zones(&self) -> Vec<Zone> {
        self.parsed.zones.clone()
    }

    fn max_fps(&self) -> u8 {
        20
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        let idx = self.idx;
        let need_mode = !self.custom_mode_active;
        let res = {
            let client = self.ensure_client()?;
            if need_mode {
                let _ = client.set_custom_mode(idx);
            }
            client.update_leds(idx, colors)
        };
        if res.is_err() {
            self.client = None;
            self.custom_mode_active = false;
            return res;
        }
        self.custom_mode_active = true;
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        self.custom_mode_active = false;
        self.client = None;
        Ok(())
    }
}

/// `(name, version, led_count)` of a DRAM controller, `None` for other device types.
fn parse_dram(data: &[u8]) -> Option<(String, String, u8)> {
    let mut r = Reader { buf: data, off: 4 };
    if r.u32()? != DEVICE_TYPE_DRAM {
        return None;
    }
    let name = r.string()?;
    let _description = r.string();
    let version = r.string().unwrap_or_default();
    let _serial = r.string();
    let _location = r.string();
    let leds = (|| {
        let modes = r.u16()? as usize;
        let _active_mode = r.u32()?;
        for _ in 0..modes.min(64) {
            let _name = r.string()?;
            r.skip(4 * 12)?; // value, flags, speed/brightness/colour ranges, speed, brightness, dir, colour mode
            let colors = r.u16()? as usize;
            r.skip(colors * 4)?;
        }
        let zones = r.u16()? as usize;
        for _ in 0..zones.min(32) {
            let _name = r.string()?;
            r.skip(4 * 4)?; // type, min, max, count
            let matrix = r.u16()? as usize;
            r.skip(matrix)?;
        }
        Some((r.u16()? as usize).clamp(1, 32) as u8)
    })()
    .unwrap_or(8);
    Some((name, version, leds))
}

/// RAM RGB through a running OpenRGB (no administrator rights needed).
pub struct OpenRgbRamBackend;

impl RamRgbBackend for OpenRgbRamBackend {
    fn label(&self) -> &'static str {
        "OpenRGB SDK"
    }

    fn handles(&self, backend: &str) -> bool {
        backend == TAG
    }

    fn requires_admin(&self) -> bool {
        false
    }

    fn probe(&self) -> RamProbe {
        let Some(mut client) = OrgbClient::connect() else {
            return RamProbe::default();
        };
        let count = client.controller_count().unwrap_or(0);
        let modules = (0..count)
            .filter_map(|idx| {
                let data = client.controller_data(idx)?;
                let (name, version, led_count) = parse_dram(&data)?;
                Some(EneRamModule {
                    address_7bit: idx as u8,
                    bus_arg: idx as u16,
                    firmware: if version.is_empty() {
                        name
                    } else {
                        format!("{name} ({version})")
                    },
                    is_v2: true,
                    led_count,
                    current_mode: 1,
                    backend: TAG.to_string(),
                })
            })
            .collect();
        RamProbe {
            available: true,
            modules,
        }
    }

    fn apply(&self, modules: &[&EneRamModule], effect: &RamEffect) -> Result<String> {
        let color = if effect.mode == EneRamMode::Off {
            Rgb::BLACK
        } else {
            effect.color
        };
        for m in modules {
            let mut client = OrgbClient::connect()
                .context("Serveur OpenRGB SDK (127.0.0.1:6742) indisponible")?;
            client.set_all_leds(m.bus_arg as u32, m.led_count as usize, color)?;
        }
        Ok(format!("OK OpenRGB SDK ({} DIMM)", modules.len()))
    }

    fn set_modes(&self, _modules: &[(&EneRamModule, u8)]) -> Result<usize> {
        // Raw ENE modes do not exist through OpenRGB; lights-off goes through `apply`.
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_rejects_bad_magic_and_oversized_packets() {
        let mut hdr = [0u8; 16];
        assert!(parse_header(&hdr).is_err());
        hdr[0..4].copy_from_slice(b"ORGB");
        hdr[12..16].copy_from_slice(&(MAX_PACKET_LEN + 1).to_le_bytes());
        assert!(parse_header(&hdr).is_err());
        hdr[4..8].copy_from_slice(&3u32.to_le_bytes());
        hdr[12..16].copy_from_slice(&128u32.to_le_bytes());
        assert_eq!(parse_header(&hdr).unwrap(), (3, 0, 128));
    }

    #[test]
    fn dram_controller_data_is_parsed() {
        let s = |v: &mut Vec<u8>, text: &str| {
            v.extend_from_slice(&(text.len() as u16 + 1).to_le_bytes());
            v.extend_from_slice(text.as_bytes());
            v.push(0);
        };
        let mut d = vec![0u8; 4];
        d.extend_from_slice(&DEVICE_TYPE_DRAM.to_le_bytes());
        for t in ["Trident Z5", "desc", "1.0", "sn", "loc"] {
            s(&mut d, t);
        }
        d.extend_from_slice(&0u16.to_le_bytes()); // modes
        d.extend_from_slice(&0u32.to_le_bytes()); // active mode
        d.extend_from_slice(&0u16.to_le_bytes()); // zones
        d.extend_from_slice(&10u16.to_le_bytes()); // leds
        assert_eq!(
            parse_dram(&d),
            Some(("Trident Z5".into(), "1.0".into(), 10))
        );
        d[4] = 2; // not DRAM
        assert_eq!(parse_dram(&d), None);
    }

    #[test]
    fn full_controller_and_update_packet_are_parsed_and_encoded() {
        let s = |v: &mut Vec<u8>, text: &str| {
            v.extend_from_slice(&(text.len() as u16 + 1).to_le_bytes());
            v.extend_from_slice(text.as_bytes());
            v.push(0);
        };
        let mut d = vec![0u8; 4];
        d.extend_from_slice(&4u32.to_le_bytes()); // Ledstrip
        for t in [
            "Lian Li Strimer",
            "ARGB Controller",
            "desc",
            "2.1",
            "sn",
            "USB",
        ] {
            s(&mut d, t);
        }
        d.extend_from_slice(&0u16.to_le_bytes()); // modes
        d.extend_from_slice(&0u32.to_le_bytes()); // active mode
        d.extend_from_slice(&1u16.to_le_bytes()); // zones
        s(&mut d, "Strip A");
        d.extend_from_slice(&1u32.to_le_bytes()); // Linear
        d.extend_from_slice(&1u32.to_le_bytes()); // min
        d.extend_from_slice(&60u32.to_le_bytes()); // max
        d.extend_from_slice(&8u32.to_le_bytes()); // count
        d.extend_from_slice(&0u16.to_le_bytes()); // matrix size
        d.extend_from_slice(&8u16.to_le_bytes()); // leds
        let parsed = parse_controller(&d).unwrap();
        assert_eq!(parsed.name, "Lian Li Strimer");
        assert_eq!(parsed.vendor, "ARGB Controller");
        assert_eq!(parsed.total_leds, 8);
        assert_eq!(parsed.zones.len(), 1);
        assert_eq!(parsed.zones[0].kind, ZoneKind::Linear);

        let pkt = encode_update_leds(&[Rgb::RED, Rgb::CYAN]);
        assert_eq!(u32::from_le_bytes(pkt[0..4].try_into().unwrap()), 14);
        assert_eq!(u16::from_le_bytes(pkt[4..6].try_into().unwrap()), 2);
        assert_eq!(&pkt[6..14], &[255, 0, 0, 0, 0, 210, 255, 0]);
    }
}
