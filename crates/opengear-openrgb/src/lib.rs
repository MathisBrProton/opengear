//! Bridge to a running OpenRGB through its network SDK protocol (`127.0.0.1:6742`).
//!
//! Own implementation of the documented wire protocol: no OpenRGB (GPL) code is used. OpenRGB stays
//! a separate program the user runs; OpenGear only talks to its SDK server, which works without
//! administrator rights.

use anyhow::{Context, Result};
use opengear_core::Rgb;
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
        let n = led_count.clamp(1, 32);
        let size = (6 + n * 4) as u32;
        let mut payload = Vec::with_capacity(size as usize);
        payload.extend_from_slice(&size.to_le_bytes());
        payload.extend_from_slice(&(n as u16).to_le_bytes());
        for _ in 0..n {
            payload.extend_from_slice(&[color.r, color.g, color.b, 0]);
        }
        self.send(idx, PKT_UPDATE_LEDS, &payload)
            .context("Échec envoi couleurs OpenRGB SDK")
    }
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
}
