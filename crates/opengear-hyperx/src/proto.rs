//! HyperX HID packet builders (clean-room from `docs/protocols/hyperx.md`).

use crate::tables::{
    ALLOY_ELITE_EXT_OFFSETS, ALLOY_ELITE_KEY_OFFSETS, ORIGINS_CORE_KEYS, origins_core_offsets,
};
use anyhow::Result;
use hidapi::HidDevice;
use opengear_core::Rgb;

pub trait HxIo: Send {
    fn write_output(&mut self, data: &[u8]) -> Result<()>;
    fn send_feature(&mut self, data: &[u8]) -> Result<()>;
    fn read_input(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize>;
    fn get_feature(&mut self, _buf: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

pub struct HidHxIo(pub HidDevice);

impl HxIo for HidHxIo {
    fn write_output(&mut self, data: &[u8]) -> Result<()> {
        self.0.write(data)?;
        Ok(())
    }
    fn send_feature(&mut self, data: &[u8]) -> Result<()> {
        self.0.send_feature_report(data)?;
        Ok(())
    }
    fn read_input(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
        Ok(self.0.read_timeout(buf, timeout_ms)?)
    }
    fn get_feature(&mut self, buf: &mut [u8]) -> Result<usize> {
        Ok(self.0.get_feature_report(buf)?)
    }
}

/// §3.1/§3.2 Feature report 0x07 (264 bytes) for one channel (`1=R, 2=G, 3=B`).
pub fn report_0x07_key_channel(channel: u8, colors: &[Rgb]) -> [u8; 264] {
    let mut buf = [0u8; 264];
    buf[0] = 0x07;
    buf[1] = 0x16;
    buf[2] = channel;
    buf[3] = 0xA0;
    for (i, &(off, _, _, _)) in ALLOY_ELITE_KEY_OFFSETS.iter().enumerate() {
        let c = colors.get(i).copied().unwrap_or(Rgb::BLACK);
        buf[off] = match channel {
            1 => c.r,
            2 => c.g,
            _ => c.b,
        };
    }
    buf
}

/// §3.1 Extended channel 4 (lightbar + media keys) for Alloy Elite RGB.
pub fn alloy_elite_ext_channel(ext_colors: &[Rgb]) -> [u8; 264] {
    let mut buf = [0u8; 264];
    buf[0] = 0x07;
    buf[1] = 0x16;
    buf[2] = 0x04;
    buf[3] = 0xA0;
    for (i, &(or, og, ob, _, _)) in ALLOY_ELITE_EXT_OFFSETS.iter().enumerate() {
        let c = ext_colors.get(i).copied().unwrap_or(Rgb::BLACK);
        buf[or] = c.r;
        buf[og] = c.g;
        buf[ob] = c.b;
    }
    buf
}

/// §3.3 / §5 / §4.5 Feature report 0x04 0xF2 header (65 bytes).
pub fn frame_0x81_header(byte8: u8, byte9: u8) -> [u8; 65] {
    let mut buf = [0u8; 65];
    buf[1] = 0x04;
    buf[2] = 0xF2;
    buf[8] = byte8;
    buf[9] = byte9;
    buf
}

/// §3.3 Packet of up to 16 `0x81 R G B` slots (65 bytes).
pub fn frame_0x81_chunk(slots: &[Rgb]) -> [u8; 65] {
    let mut buf = [0u8; 65];
    for i in 0..16 {
        let c = slots.get(i).copied().unwrap_or(Rgb::BLACK);
        let off = 1 + i * 4;
        buf[off] = 0x81;
        buf[off + 1] = c.r;
        buf[off + 2] = c.g;
        buf[off + 3] = c.b;
    }
    buf
}

/// §3.4 Alloy Origins Core 380-byte interleaved buffer and 7 `0xA2` packets.
pub fn origins_core_packets(colors: &[Rgb]) -> Vec<Vec<u8>> {
    let mut img = [0u8; 380];
    for (i, &(v, _, _, _)) in ORIGINS_CORE_KEYS.iter().enumerate() {
        let c = colors.get(i).copied().unwrap_or(Rgb::BLACK);
        let (og, or, ob) = origins_core_offsets(v);
        img[og] = c.g;
        img[or] = c.r;
        img[ob] = c.b;
    }
    img.chunks(60)
        .enumerate()
        .map(|(seq, chunk)| {
            let mut pkt = vec![0u8; 5 + chunk.len()];
            pkt[1] = 0xA2;
            pkt[2] = seq as u8;
            pkt[4] = chunk.len() as u8;
            pkt[5..].copy_from_slice(chunk);
            pkt
        })
        .collect()
}

/// §3.5 / §6.2 Report 0x44 start packet + color packets (20 LEDs per packet).
pub fn report_0x44_packets(
    report_len: usize,
    start_arg: u8,
    colors: &[Rgb],
    total_slots: usize,
) -> Vec<Vec<u8>> {
    let mut start = vec![0u8; report_len];
    start[0] = 0x44;
    start[1] = 0x01;
    start[2] = start_arg;
    let mut out = vec![start];
    let pkt_count = total_slots.div_ceil(20).max(1);
    for p in 0..pkt_count {
        let mut pkt = vec![0u8; report_len];
        pkt[0] = 0x44;
        pkt[1] = 0x02;
        pkt[2] = p as u8;
        let base = p * 20;
        let count = (total_slots - base).min(20);
        for i in 0..count {
            let c = colors.get(base + i).copied().unwrap_or(Rgb::BLACK);
            let off = 4 + i * 3;
            if off + 3 <= pkt.len() {
                pkt[off] = c.r;
                pkt[off + 1] = c.g;
                pkt[off + 2] = c.b;
            }
        }
        out.push(pkt);
    }
    out
}

/// §4.3 Pulsefire Dart 65-byte output packet.
pub fn pulsefire_dart_pkt(zone: u8, mode: u8, c: Rgb, brightness: u8, speed: u8) -> [u8; 65] {
    let mut buf = [0u8; 65];
    buf[1] = 0xD2;
    buf[2] = zone;
    buf[3] = mode;
    buf[4] = 0x08;
    buf[5..8].copy_from_slice(&[c.r, c.g, c.b]);
    buf[8..11].copy_from_slice(&[c.r, c.g, c.b]);
    buf[11] = brightness.min(100);
    buf[12] = speed.min(0x64);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_core_interleaved_offsets_match_table_8_7() {
        assert_eq!(origins_core_offsets(0), (0, 16, 32)); // Escape
        assert_eq!(origins_core_offsets(18), (56, 72, 88)); // KeyQ
        assert_eq!(origins_core_offsets(48), (192, 208, 224)); // F8
        assert_eq!(origins_core_offsets(92), (346, 362, 378)); // ArrowRight

        let pkts = origins_core_packets(&[Rgb::new(10, 20, 30)]);
        assert_eq!(pkts.len(), 7);
        assert_eq!(pkts[0].len(), 65);
        assert_eq!(pkts[6].len(), 25); // 5 + 20 remaining bytes
        assert_eq!(pkts[0][5], 20); // G of Escape at img[0]
        assert_eq!(pkts[0][5 + 16], 10); // R of Escape at img[16]
        assert_eq!(pkts[0][5 + 32], 30); // B of Escape at img[32]
    }

    #[test]
    fn pulsefire_dart_and_0x44_packets_match_sheet() {
        let dart = pulsefire_dart_pkt(0x10, 0x00, Rgb::RED, 80, 0);
        assert_eq!(
            &dart[1..13],
            &[0xD2, 0x10, 0x00, 0x08, 255, 0, 0, 255, 0, 0, 80, 0]
        );

        let pkts = report_0x44_packets(65, 0x04, &[Rgb::CYAN, Rgb::RED], 74);
        assert_eq!(pkts.len(), 5); // 1 start + 4 color chunks
        assert_eq!(&pkts[0][0..3], &[0x44, 0x01, 0x04]);
        assert_eq!(
            &pkts[1][0..10],
            &[0x44, 0x02, 0x00, 0x00, 0, 210, 255, 255, 0, 0]
        );
    }
}
