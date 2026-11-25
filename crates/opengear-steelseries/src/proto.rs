//! SteelSeries wire framing helpers and HID I/O abstraction (clean-room from `steelseries.md`).

use anyhow::Result;
use hidapi::HidDevice;
use opengear_core::Rgb;

pub trait SsIo: Send {
    fn write_output(&mut self, data: &[u8]) -> Result<()>;
    fn send_feature(&mut self, data: &[u8]) -> Result<()>;
    fn read_input(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize>;
}

pub struct HidSsIo(pub HidDevice);

impl SsIo for HidSsIo {
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
}

/// §3.1 Rival 3 color packet (8 bytes, `zone` in 1..=4).
pub fn rival3_color_pkt(zone: u8, color: Rgb, brightness: u8) -> [u8; 8] {
    [0x00, 0x05, 0x00, zone, color.r, color.g, color.b, brightness.min(100)]
}

/// §3.1 Rival 3 effect packet (4 bytes; excludes unsafe 0x06 disco mode).
pub fn rival3_effect_pkt(effect_byte: u8) -> [u8; 4] {
    [0x00, 0x06, 0x00, effect_byte]
}

/// §3.2 Aerox 3 / 5 wired colors (65-byte output report).
pub fn aerox_wired_color_pkt(colors: &[Rgb], aerox5: bool) -> [u8; 65] {
    let mut buf = [0u8; 65];
    let mut mask = 0u8;
    let count = colors.len().min(3);
    for (z, c) in colors.iter().take(count).enumerate() {
        mask |= 1 << z;
        let off = 3 + z * 3;
        buf[off] = c.r;
        buf[off + 1] = c.g;
        buf[off + 2] = c.b;
    }
    let _ = aerox5; // zones 0..2 share offsets 3/6/9 with bitmask
    buf[1] = 0x21;
    buf[2] = mask;
    buf
}

/// §3.2/§3.3 Reactive LED command (zone 3 on Aerox 5/9).
pub fn aerox_reactive_pkt(color: Rgb, is_24g: bool) -> [u8; 65] {
    let mut buf = [0u8; 65];
    buf[1] = 0x26 | if is_24g { 0x40 } else { 0 };
    buf[2] = 0x01;
    buf[3] = 0x00;
    buf[4] = color.r;
    buf[5] = color.g;
    buf[6] = color.b;
    buf
}

/// §3.3 Aerox wireless zone color packet (64 bytes, sets bit 0x40 on 2.4 GHz).
pub fn aerox_wireless_zone_pkt(zone: u8, color: Rgb, is_24g: bool) -> [u8; 64] {
    let mut buf = [0u8; 64];
    buf[1] = 0x21 | if is_24g { 0x40 } else { 0 };
    buf[2] = 0x01;
    buf[3] = zone;
    buf[4] = color.r;
    buf[5] = color.g;
    buf[6] = color.b;
    buf
}

/// §3.3 Rival 3 Wireless color packet (64 bytes, single LED, no 0x40 bit).
pub fn rival3_wireless_color_pkt(color: Rgb) -> [u8; 64] {
    let mut buf = [0u8; 64];
    buf[1] = 0x13;
    buf[2] = 0x01;
    buf[3] = color.r;
    buf[4] = color.g;
    buf[5] = color.b;
    buf
}

/// §3.4 Rival 5 10-LED feature packet (64 bytes).
pub fn rival5_colors_pkt(colors: &[Rgb]) -> [u8; 64] {
    let mut buf = [0u8; 64];
    buf[1] = 0x21;
    buf[2] = 0xFF;
    buf[3] = 0x03;
    for (i, c) in colors.iter().take(10).enumerate() {
        let off = 4 + i * 3;
        buf[off] = c.r;
        buf[off + 1] = c.g;
        buf[off + 2] = c.b;
    }
    buf
}

/// §4.1 Apex G1/G2/G3 direct frame feature report (exact 643 or 513 byte length).
pub fn apex_direct_frame_pkt(report_id: u8, short_frame: bool, entries: &[(u8, Rgb)]) -> Vec<u8> {
    let total_len = if short_frame { 513 } else { 643 };
    let max_entries = (total_len - 3) / 4;
    let count = entries.len().min(max_entries);
    let mut buf = vec![0u8; total_len];
    buf[1] = report_id;
    buf[2] = count as u8;
    for (i, &(code, c)) in entries.iter().take(count).enumerate() {
        let off = 3 + i * 4;
        buf[off] = code;
        buf[off + 1] = c.r;
        buf[off + 2] = c.g;
        buf[off + 3] = c.b;
    }
    buf
}

/// §4.1 Apex return to onboard hardware mode (0x3B for G1/G2, 0x41 for G3).
pub fn apex_onboard_pkt(g3: bool) -> [u8; 65] {
    let mut buf = [0u8; 65];
    buf[1] = if g3 { 0x41 } else { 0x3B };
    buf
}

/// §4.4 Apex 3 10-zone output color report (33 bytes).
pub fn apex3_colors_pkt(colors: &[Rgb]) -> [u8; 33] {
    let mut buf = [0u8; 33];
    buf[1] = 0x0B;
    for (i, c) in colors.iter().take(10).enumerate() {
        let off = 3 + i * 3;
        buf[off] = c.r;
        buf[off + 1] = c.g;
        buf[off + 2] = c.b;
    }
    buf
}

/// §4.5 Apex 3 TKL 8-zone output color report (65 bytes).
pub fn apex3_tkl_colors_pkt(colors: &[Rgb]) -> [u8; 65] {
    let mut buf = [0u8; 65];
    buf[1] = 0x21;
    buf[2] = 0xFF;
    for (i, c) in colors.iter().take(8).enumerate() {
        let off = 3 + i * 3;
        buf[off] = c.r;
        buf[off + 1] = c.g;
        buf[off + 2] = c.b;
    }
    buf
}

/// §6 QCK Prism 2-LED feature report (525 bytes).
pub fn qck_prism_frame_pkt(bottom: Rgb, top: Rgb) -> Vec<u8> {
    let mut buf = vec![0u8; 525];
    buf[1] = 0x0E;
    buf[3] = 0x02;
    buf[5..8].copy_from_slice(&[bottom.r, bottom.g, bottom.b]);
    buf[8] = 0xFF;
    buf[9] = 0x32;
    buf[10] = 0xC8;
    buf[14] = 0x01;
    buf[17..20].copy_from_slice(&[top.r, top.g, top.b]);
    buf[20] = 0xFF;
    buf[21] = 0x32;
    buf[22] = 0xC8;
    buf[25] = 0x01;
    buf[26] = 0x01;
    buf[28] = 0x01;
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rival3_packet_matches_sheet() {
        let pkt = rival3_color_pkt(2, Rgb::new(10, 20, 30), 80);
        assert_eq!(pkt, [0x00, 0x05, 0x00, 2, 10, 20, 30, 80]);
        assert_eq!(rival3_effect_pkt(0x04), [0x00, 0x06, 0x00, 0x04]);
    }

    #[test]
    fn aerox_wireless_sets_24g_bit_and_r3w_does_not() {
        let p24 = aerox_wireless_zone_pkt(1, Rgb::RED, true);
        assert_eq!(&p24[0..7], &[0x00, 0x61, 0x01, 1, 255, 0, 0]);
        let r3w = rival3_wireless_color_pkt(Rgb::CYAN);
        assert_eq!(&r3w[0..6], &[0x00, 0x13, 0x01, 0, 210, 255]);
    }

    #[test]
    fn apex_direct_frames_keep_exact_lengths() {
        let entries = [(0x04, Rgb::RED), (0x29, Rgb::BLUE)];
        let full = apex_direct_frame_pkt(0x3A, false, &entries);
        assert_eq!(full.len(), 643);
        assert_eq!(full[1], 0x3A);
        assert_eq!(full[2], 2);
        assert_eq!(&full[3..11], &[0x04, 255, 0, 0, 0x29, 0, 0, 255]);

        let short = apex_direct_frame_pkt(0x40, true, &entries);
        assert_eq!(short.len(), 513);
        assert_eq!(short[1], 0x40);
        assert_eq!(apex_onboard_pkt(false)[1], 0x3B);
        assert_eq!(apex_onboard_pkt(true)[1], 0x41);
    }

    #[test]
    fn qck_prism_525_bytes_match_sheet() {
        let pkt = qck_prism_frame_pkt(Rgb::RED, Rgb::GREEN);
        assert_eq!(pkt.len(), 525);
        assert_eq!(pkt[1], 0x0E);
        assert_eq!(pkt[3], 0x02);
        assert_eq!(&pkt[5..8], &[255, 0, 0]);
        assert_eq!(&pkt[17..20], &[0, 255, 0]);
    }
}
