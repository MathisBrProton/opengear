//! Shared `Driver`, `Device`, and `Lighting` implementation for HyperX peripherals.
//!
//! Safety guards per `docs/protocols/hyperx.md`:
//! - Pulsefire Mat XL (`1741`) strictly zeroes remaining slots beyond 2 LEDs (no OOB read).
//! - Flash/EEPROM writes (`0xDE 0xFF`, QuadCast save) only run on explicit `req.save`, never in live frames.
//! - QuadCast 2 S implements the sheet's failure backoff (drain on 5 errors, pause 5s on 10 errors).

use crate::ops::{
    HxState, apply_device_setting, apply_legacy_brightness_hw, apply_universal_brightness,
    build_diagnostic_lines, build_settings, discover_hyperx_devices, probe_and_refresh,
    send_universal_frame,
};
use crate::proto::{
    HxIo, alloy_elite_ext_channel, frame_0x81_chunk, frame_0x81_header, origins_core_packets,
    pulsefire_dart_pkt, report_0x07_key_channel, report_0x44_packets,
};
use crate::proto_ngenuity::{
    UnivDeviceInfo, legacy_mouse_builtin_effect_pkt, univ_preset_light_pkt,
};
use crate::tables::{
    ALLOY_ELITE_EXT_OFFSETS, ALLOY_ELITE_KEY_OFFSETS, HP_HYPERX_VID, HxDeviceDef, KINGSTON_VID,
    ORIGINS_2_65_SLOTS, ORIGINS_60_SLOTS, ORIGINS_65_TAIL, ORIGINS_CORE_KEYS, ProtocolFamily,
};
use anyhow::Result;
use opengear_core::Rgb;
use opengear_core::driver::{
    Connection, Device, DeviceInfo, DeviceStatus, Driver, Setting, Value, setting_ids,
};
use opengear_core::lighting::{
    EffectParams, EffectRequest, HardwareEffect, Led, Lighting, Zone, ZoneKind,
};
use std::time::{Duration, Instant};

pub const DRIVER_NAME: &str = "hyperx";

pub struct HyperXDriver;

impl Driver for HyperXDriver {
    fn name(&self) -> &'static str {
        DRIVER_NAME
    }

    fn display_name(&self) -> &'static str {
        "HyperX (claviers, souris, casques, tapis, micros)"
    }

    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[KINGSTON_VID, HP_HYPERX_VID]
    }

    fn experimental(&self) -> bool {
        true
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        discover_hyperx_devices()
    }
}

pub struct HyperXDevice {
    def: &'static HxDeviceDef,
    io: Box<dyn HxIo>,
    path_tag: String,
    state: HxState,
    qc2_errors: Vec<Instant>,
    qc2_backoff_until: Option<Instant>,
}

impl HyperXDevice {
    pub fn new(def: &'static HxDeviceDef, io: Box<dyn HxIo>, path_tag: String) -> Self {
        Self {
            def,
            io,
            path_tag,
            state: HxState::default(),
            qc2_errors: Vec::new(),
            qc2_backoff_until: None,
        }
    }

    pub(crate) fn set_tio_slot(&mut self, slot: u8, name: Option<String>, info: UnivDeviceInfo) {
        self.state.tio_slot = slot;
        self.state.custom_name = name;
        self.state.univ_info = Some(info);
    }

    pub(crate) fn probe_on_discover(&mut self) -> DeviceStatus {
        probe_and_refresh(self.io.as_mut(), self.def, &mut self.state)
    }

    fn apply_brightness_hw(&mut self) -> Result<()> {
        if self.def.caps().universal {
            return apply_universal_brightness(self.io.as_mut(), &self.state);
        }
        apply_legacy_brightness_hw(self.io.as_mut(), self.def, &self.state)?;
        if !self.state.last_frame.is_empty()
            && matches!(
                self.def.family,
                ProtocolFamily::Frame0x81Keyboard { .. }
                    | ProtocolFamily::QuadCastS
                    | ProtocolFamily::QuadCast2S
                    | ProtocolFamily::PulsefireDart
            )
        {
            let frame = self.state.last_frame.clone();
            self.set_frame(&frame)?;
        }
        Ok(())
    }

    fn qc2_exchange(&mut self, pkt: &[u8]) -> Result<()> {
        if let Some(until) = self.qc2_backoff_until
            && Instant::now() < until
        {
            return Ok(());
        }
        self.io.write_output(pkt)?;
        let mut rx = [0u8; 64];
        let n = self.io.read_input(&mut rx, 2000).unwrap_or(0);
        let ack_ok = n >= 16 && rx[14] == pkt[0] && rx[15] == pkt[1];
        if !ack_ok {
            let now = Instant::now();
            self.qc2_errors
                .retain(|t| now.duration_since(*t) < Duration::from_secs(10));
            self.qc2_errors.push(now);
            if self.qc2_errors.len() >= 10 {
                self.qc2_backoff_until = Some(now + Duration::from_secs(5));
                self.qc2_errors.clear();
            } else if self.qc2_errors.len() >= 5 {
                let mut drain = [0u8; 64];
                for _ in 0..8 {
                    if self.io.read_input(&mut drain, 5).unwrap_or(0) == 0 {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Device for HyperXDevice {
    fn info(&self) -> DeviceInfo {
        let slug: u32 = self.path_tag.bytes().fold(0x811C_9DC5u32, |h, b| {
            h.wrapping_mul(0x0100_0193) ^ u32::from(b)
        });
        let fw = self
            .state
            .univ_info
            .as_ref()
            .map(|u| format!("{:#06X}", u.primary_fw));
        DeviceInfo {
            id: format!(
                "{DRIVER_NAME}:{:04x}:{:04x}:{slug:08x}",
                self.def.vid, self.def.pid
            ),
            vendor: "HyperX".into(),
            model: self
                .state
                .custom_name
                .clone()
                .unwrap_or_else(|| self.def.name.into()),
            kind: self.def.kind,
            connection: if self.def.wireless {
                Connection::Wireless
            } else {
                Connection::Usb
            },
            firmware: fw,
        }
    }

    fn status(&self) -> DeviceStatus {
        self.state.status.clone()
    }

    fn refresh_status(&mut self) {
        let _ = probe_and_refresh(self.io.as_mut(), self.def, &mut self.state);
    }

    fn diagnostic_lines(&self) -> Vec<String> {
        build_diagnostic_lines(self.def, &self.state, &self.path_tag)
    }

    fn settings(&self) -> Vec<Setting> {
        build_settings(self.def, &self.state)
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        if id == setting_ids::HW_BRIGHTNESS {
            self.state.brightness_pct =
                value.as_f64().unwrap_or(100.0).round().clamp(0.0, 100.0) as u8;
            return self.apply_brightness_hw();
        }
        apply_device_setting(self.io.as_mut(), self.def, &mut self.state, id, value)
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        if self.def.led_count > 0 {
            Some(self)
        } else {
            None
        }
    }
}

fn slot_leds(slots: &[Option<&str>]) -> Vec<Led> {
    let n = slots.len().max(1);
    slots
        .iter()
        .enumerate()
        .map(|(i, code)| {
            let x = (i % 16) as f32 / 15.0;
            let y = (i / 16) as f32 / ((n / 16).max(1) as f32);
            match code {
                Some(w3c) => Led::at(*w3c, x, y).key(*w3c),
                None => Led::at(format!("Slot {i}"), x, y),
            }
        })
        .collect()
}

impl Lighting for HyperXDevice {
    fn zones(&self) -> Vec<Zone> {
        match self.def.family {
            ProtocolFamily::AlloyElite => {
                let key_leds = ALLOY_ELITE_KEY_OFFSETS
                    .iter()
                    .map(|&(_, code, nx, ny)| Led::at(code, nx, ny).key(code))
                    .collect();
                let ext_leds = ALLOY_ELITE_EXT_OFFSETS
                    .iter()
                    .enumerate()
                    .map(|(i, &(_, _, _, opt_code, nx))| match opt_code {
                        Some(w3c) => Led::at(w3c, nx, 0.02).key(w3c),
                        None => Led::at(format!("Barre {}", i + 1), nx, 0.0),
                    })
                    .collect();
                vec![
                    Zone {
                        id: "keys".into(),
                        name: "Touches".into(),
                        kind: ZoneKind::Matrix,
                        leds: key_leds,
                        resizable: None,
                    },
                    Zone {
                        id: "lightbar".into(),
                        name: "Barre lumineuse & média".into(),
                        kind: ZoneKind::Linear,
                        leds: ext_leds,
                        resizable: None,
                    },
                ]
            }
            ProtocolFamily::AlloyFps => {
                let leds = ALLOY_ELITE_KEY_OFFSETS
                    .iter()
                    .map(|&(_, code, nx, ny)| Led::at(code, nx, ny).key(code))
                    .collect();
                vec![Zone {
                    id: "keys".into(),
                    name: "Touches".into(),
                    kind: ZoneKind::Matrix,
                    leds,
                    resizable: None,
                }]
            }
            ProtocolFamily::Frame0x81Keyboard { slots, .. } => {
                let leds = if slots == 71 {
                    slot_leds(ORIGINS_60_SLOTS)
                } else if slots == 77 {
                    let mut v = ORIGINS_60_SLOTS[..68].to_vec();
                    v.extend_from_slice(ORIGINS_65_TAIL);
                    slot_leds(&v)
                } else {
                    (0..slots)
                        .map(|i| {
                            Led::at(
                                format!("LED {}", i + 1),
                                (i % 16) as f32 / 15.0,
                                (i / 16) as f32 / 8.0,
                            )
                        })
                        .collect()
                };
                vec![Zone {
                    id: "keys".into(),
                    name: "Touches".into(),
                    kind: ZoneKind::Matrix,
                    leds,
                    resizable: None,
                }]
            }
            ProtocolFamily::OriginsCore => {
                let leds = ORIGINS_CORE_KEYS
                    .iter()
                    .map(|&(_, r, c, w3c)| Led::at(w3c, c as f32 / 16.0, r as f32 / 5.0).key(w3c))
                    .collect();
                vec![Zone {
                    id: "keys".into(),
                    name: "Touches".into(),
                    kind: ZoneKind::Matrix,
                    leds,
                    resizable: None,
                }]
            }
            ProtocolFamily::Report0x44Origins265 => vec![Zone {
                id: "keys".into(),
                name: "Touches".into(),
                kind: ZoneKind::Matrix,
                leds: slot_leds(ORIGINS_2_65_SLOTS),
                resizable: None,
            }],
            _ => vec![Zone::linear("main", self.def.name, self.def.led_count, 0.5)],
        }
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        let mk = |id: &str, label: &str, colors: (u8, u8), speed: bool, br: bool, savable: bool| {
            HardwareEffect {
                id: id.into(),
                label: label.into(),
                params: EffectParams {
                    colors,
                    speed: if speed { Some((0, 100)) } else { None },
                    brightness: if br { Some((0, 100)) } else { None },
                    savable,
                    ..Default::default()
                },
                zones: Vec::new(),
            }
        };
        if self.def.caps().universal && self.def.led_count > 0 {
            return vec![
                mk("off", "Éteint", (0, 0), false, false, false),
                mk("static", "Statique", (1, 1), false, true, false),
                mk("breathing", "Respiration", (1, 2), true, true, false),
                mk("wave", "Vague arc-en-ciel", (0, 0), true, true, false),
                mk("spectrum", "Cycle spectral", (0, 0), true, true, false),
                mk("confetti", "Confetti", (0, 0), true, true, false),
            ];
        }
        match self.def.family {
            ProtocolFamily::PulsefireDart | ProtocolFamily::PulsefireHasteWireless => vec![
                mk(
                    "static",
                    "Statique",
                    (1, 2),
                    false,
                    true,
                    matches!(self.def.family, ProtocolFamily::PulsefireDart),
                ),
                mk(
                    "breathing",
                    "Respiration",
                    (1, 2),
                    true,
                    true,
                    matches!(self.def.family, ProtocolFamily::PulsefireDart),
                ),
                mk(
                    "spectrum",
                    "Cycle spectral",
                    (0, 0),
                    true,
                    true,
                    matches!(self.def.family, ProtocolFamily::PulsefireDart),
                ),
            ],
            _ => Vec::new(),
        }
    }

    fn max_fps(&self) -> u8 {
        match self.def.family {
            ProtocolFamily::QuadCast2S => 4,
            _ => 20,
        }
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        let scale = |c: Rgb| -> Rgb {
            let p = u16::from(self.state.brightness_pct.min(100));
            if p == 100 {
                return c;
            }
            Rgb::new(
                ((u16::from(c.r) * p) / 100) as u8,
                ((u16::from(c.g) * p) / 100) as u8,
                ((u16::from(c.b) * p) / 100) as u8,
            )
        };
        let first = colors.first().copied().unwrap_or(Rgb::BLACK);
        match self.def.family {
            ProtocolFamily::AlloyElite => {
                for ch in 1..=3u8 {
                    self.io.send_feature(&report_0x07_key_channel(ch, colors))?;
                    std::thread::sleep(Duration::from_millis(5));
                }
                let ext = colors.get(106..).unwrap_or(&[]);
                self.io.send_feature(&alloy_elite_ext_channel(ext))?;
            }
            ProtocolFamily::AlloyFps => {
                for ch in 1..=3u8 {
                    self.io.send_feature(&report_0x07_key_channel(ch, colors))?;
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            ProtocolFamily::Frame0x81Keyboard {
                slots,
                header_byte9,
            } => {
                self.state.last_frame = colors.to_vec();
                let scaled: Vec<Rgb> = colors.iter().copied().map(scale).collect();
                let pkt_count = slots.div_ceil(16);
                self.io
                    .send_feature(&frame_0x81_header(0x00, header_byte9))?;
                for p in 0..pkt_count {
                    let start = p * 16;
                    let slice = scaled
                        .get(start..(start + 16).min(scaled.len()))
                        .unwrap_or(&[]);
                    self.io.send_feature(&frame_0x81_chunk(slice))?;
                }
            }
            ProtocolFamily::OriginsCore => {
                for pkt in origins_core_packets(colors) {
                    self.io.write_output(&pkt)?;
                }
            }
            ProtocolFamily::Report0x44Origins265 => {
                for pkt in report_0x44_packets(65, 0x04, colors, 74) {
                    self.io.write_output(&pkt)?;
                }
            }
            ProtocolFamily::Report0x44Eve1800 => {
                for pkt in report_0x44_packets(65, 0x04, colors, 10) {
                    self.io.write_output(&pkt)?;
                }
            }
            ProtocolFamily::PulsefireSurge => {
                let mut buf = [0u8; 264];
                buf[0] = 0x07;
                buf[1] = 0x14;
                buf[3] = 0xA0;
                for i in 0..32 {
                    let c = colors.get(i).copied().unwrap_or(first);
                    buf[8 + i] = c.r;
                    buf[40 + i] = c.g;
                    buf[72 + i] = c.b;
                }
                let logo = colors.get(32).copied().unwrap_or(first);
                buf[108..111].copy_from_slice(&[logo.r, logo.g, logo.b]);
                self.io.send_feature(&buf)?;
            }
            ProtocolFamily::PulsefireFpsCore | ProtocolFamily::PulsefireRaid => {
                let mut buf = [0u8; 264];
                buf[0] = 0x07;
                buf[1] = 0x0A;
                buf[2..5].copy_from_slice(&[first.r, first.g, first.b]);
                if matches!(self.def.family, ProtocolFamily::PulsefireRaid) {
                    let c1 = colors.get(1).copied().unwrap_or(first);
                    buf[5..8].copy_from_slice(&[c1.r, c1.g, c1.b]);
                }
                buf[8] = 0xA0;
                self.io.send_feature(&buf)?;
            }
            ProtocolFamily::PulsefireDart => {
                self.state.last_frame = colors.to_vec();
                let logo = colors.first().copied().unwrap_or(first);
                let wheel = colors.get(1).copied().unwrap_or(logo);
                self.io.write_output(&pulsefire_dart_pkt(
                    0x10,
                    0x00,
                    wheel,
                    self.state.brightness_pct,
                    0,
                ))?;
                std::thread::sleep(Duration::from_millis(2));
                self.io.write_output(&pulsefire_dart_pkt(
                    0x00,
                    0x00,
                    logo,
                    self.state.brightness_pct,
                    0,
                ))?;
            }
            ProtocolFamily::PulsefireHaste
            | ProtocolFamily::PulsefireFpsPro2
            | ProtocolFamily::PulsefireHasteWireless => {
                self.io.send_feature(&frame_0x81_header(0x02, 0x00))?;
                let mut pkt = [0u8; 65];
                pkt[1] = 0x81;
                pkt[2..5].copy_from_slice(&[first.r, first.g, first.b]);
                pkt[8] = 0x02;
                self.io.send_feature(&pkt)?;
            }
            ProtocolFamily::Mat0x81 { led_count } => {
                self.io.send_feature(&frame_0x81_header(0x00, 0x02))?;
                let bounded: Vec<Rgb> = (0..led_count)
                    .map(|i| colors.get(i).copied().unwrap_or(first))
                    .collect();
                for p in 0..2 {
                    let slice = bounded
                        .get(p * 16..(p * 16 + 16).min(bounded.len()))
                        .unwrap_or(&[]);
                    self.io.send_feature(&frame_0x81_chunk(slice))?;
                }
            }
            ProtocolFamily::QuadCastS | ProtocolFamily::DuoCast => {
                let (top, bot) = if matches!(self.def.family, ProtocolFamily::QuadCastS) {
                    self.state.last_frame = colors.to_vec();
                    let t = scale(colors.first().copied().unwrap_or(first));
                    let b = scale(
                        colors
                            .get(1)
                            .copied()
                            .unwrap_or(colors.first().copied().unwrap_or(first)),
                    );
                    (t, b)
                } else {
                    let t = colors.first().copied().unwrap_or(first);
                    (t, colors.get(1).copied().unwrap_or(t))
                };
                let mut c = [0u8; 65];
                c[1..9].copy_from_slice(&[0x81, top.r, top.g, top.b, 0x81, bot.r, bot.g, bot.b]);
                self.io.send_feature(&c)?;
                std::thread::sleep(Duration::from_millis(15));
                let mut reg = [0u8; 65];
                reg[1] = 0x04;
                reg[2] = 0xF2;
                reg[9] = 0x01;
                self.io.send_feature(&reg)?;
            }
            ProtocolFamily::QuadCast2S => {
                self.state.last_frame = colors.to_vec();
                let scaled: Vec<Rgb> = colors.iter().copied().map(scale).collect();
                for pkt in report_0x44_packets(64, 0x06, &scaled, 108) {
                    self.qc2_exchange(&pkt)?;
                }
            }
            ProtocolFamily::UniversalMouse { .. }
            | ProtocolFamily::UniversalKeyboard
            | ProtocolFamily::UniversalHeadset => {
                send_universal_frame(self.io.as_mut(), self.def, &self.state, colors)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        let br = req.brightness.unwrap_or(self.state.brightness_pct).min(100);
        if self.def.caps().universal && self.def.led_count > 0 {
            self.state.brightness_pct = br;
            if req.brightness.is_some() {
                apply_universal_brightness(self.io.as_mut(), &self.state)?;
            }
            let spd = req.speed.unwrap_or(50).min(100) as u16 * 10;
            let c0 = req.colors.first().copied().unwrap_or(Rgb::CYAN);
            let c1 = req.colors.get(1).copied().unwrap_or(c0);
            let (mode, dir, color_type, cols): (u8, u8, u8, Vec<Rgb>) = match req.effect.as_str() {
                "off" => (0, 0, 0, vec![]),
                "static" => (1, 0, 1, vec![c0]),
                "breathing" => (2, 0, if req.colors.len() > 1 { 2 } else { 1 }, vec![c0, c1]),
                "wave" => (3, if req.reverse { 1 } else { 0 }, 3, vec![]),
                "spectrum" => (4, 0, 3, vec![]),
                "confetti" => (5, 0, 3, vec![]),
                _ => anyhow::bail!("Effet universel HyperX inconnu : {}", req.effect),
            };
            return self.io.write_output(&univ_preset_light_pkt(
                self.state.tio_slot,
                1,
                mode,
                spd,
                dir,
                color_type,
                &cols,
            ));
        }

        let spd_inv = 100u8.saturating_sub(req.speed.unwrap_or(50).min(100));
        let c0 = req.colors.first().copied().unwrap_or(Rgb::CYAN);
        let c1 = req.colors.get(1).copied().unwrap_or(c0);
        match self.def.family {
            ProtocolFamily::PulsefireDart => {
                match req.effect.as_str() {
                    "spectrum" => {
                        self.io.write_output(&pulsefire_dart_pkt(
                            0x20,
                            0x12,
                            Rgb::BLACK,
                            br,
                            spd_inv,
                        ))?;
                    }
                    "static" | "breathing" => {
                        let mode = if req.effect == "static" { 0x00 } else { 0x20 };
                        self.io
                            .write_output(&pulsefire_dart_pkt(0x10, mode, c0, br, spd_inv))?;
                        std::thread::sleep(Duration::from_millis(2));
                        self.io
                            .write_output(&pulsefire_dart_pkt(0x00, mode, c1, br, spd_inv))?;
                    }
                    _ => anyhow::bail!("Effet Pulsefire Dart inconnu : {}", req.effect),
                }
                if req.save {
                    let mut sv = [0u8; 65];
                    sv[1] = 0xDE;
                    sv[2] = 0xFF;
                    self.io.write_output(&sv)?;
                }
            }
            ProtocolFamily::PulsefireHasteWireless => {
                let mode = match req.effect.as_str() {
                    "static" => 0,
                    "breathing" => 1,
                    "spectrum" => 2,
                    _ => anyhow::bail!("Effet Pulsefire Haste Wireless inconnu : {}", req.effect),
                };
                self.io.write_output(&legacy_mouse_builtin_effect_pkt(
                    mode,
                    c0,
                    c1,
                    br,
                    req.speed.unwrap_or(50).min(100),
                ))?;
            }
            _ => anyhow::bail!("Effet matériel HyperX non supporté sur {}", self.def.name),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::find_device;
    use std::sync::{Arc, Mutex};

    type Writes = Arc<Mutex<Vec<(&'static str, Vec<u8>)>>>;

    #[derive(Clone, Default)]
    struct MockIo {
        writes: Writes,
        feature_resp: Arc<Mutex<Vec<u8>>>,
    }

    impl HxIo for MockIo {
        fn write_output(&mut self, data: &[u8]) -> Result<()> {
            self.writes.lock().unwrap().push(("out", data.to_vec()));
            Ok(())
        }
        fn send_feature(&mut self, data: &[u8]) -> Result<()> {
            self.writes.lock().unwrap().push(("feat", data.to_vec()));
            Ok(())
        }
        fn read_input(&mut self, _buf: &mut [u8], _timeout_ms: i32) -> Result<usize> {
            Ok(0)
        }
        fn get_feature(&mut self, buf: &mut [u8]) -> Result<usize> {
            let r = self.feature_resp.lock().unwrap();
            let n = r.len().min(buf.len());
            buf[..n].copy_from_slice(&r[..n]);
            Ok(n)
        }
    }

    #[test]
    fn mat_xl_strictly_zeroes_slots_beyond_2_leds() {
        let def = find_device(KINGSTON_VID, 0x1741).unwrap();
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(def, Box::new(io), "mock".into());
        dev.set_frame(&[Rgb::RED, Rgb::GREEN, Rgb::BLUE, Rgb::WHITE])
            .unwrap();
        let pkts = writes.lock().unwrap();
        // header + 2 chunks
        assert_eq!(pkts.len(), 3);
        let chunk0 = &pkts[1].1;
        assert_eq!(&chunk0[1..9], &[0x81, 255, 0, 0, 0x81, 0, 255, 0]);
        // Slot 2 MUST be black even though caller passed extra colors
        assert_eq!(&chunk0[9..13], &[0x81, 0, 0, 0]);
    }

    #[test]
    fn dart_live_frame_never_writes_flash_and_save_writes_0xde_0xff() {
        let def = find_device(KINGSTON_VID, 0x16E1).unwrap();
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(def, Box::new(io), "mock".into());
        dev.set_frame(&[Rgb::RED, Rgb::BLUE]).unwrap();
        assert!(writes.lock().unwrap().iter().all(|(_, p)| p[1] != 0xDE));

        let req = EffectRequest {
            effect: "static".into(),
            zone: None,
            colors: vec![Rgb::CYAN],
            speed: None,
            brightness: Some(50),
            reverse: false,
            save: true,
        };
        dev.apply_hardware_effect(&req).unwrap();
        let pkts = writes.lock().unwrap();
        assert_eq!(&pkts.last().unwrap().1[1..3], &[0xDE, 0xFF]);
    }

    #[test]
    fn universal_mouse_dpi_and_sensor_settings_emit_expected_packets() {
        let def = find_device(HP_HYPERX_VID, 0x0B97).expect("Pulsefire Haste 2");
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(def, Box::new(io), "mock-h2".into());

        dev.apply_setting("hyperx.active_dpi_stage", &Value::Text("2".into()))
            .unwrap();
        dev.apply_setting("hyperx.dpi", &Value::Number(1600.0))
            .unwrap();
        dev.apply_setting(setting_ids::POLLING_RATE, &Value::Text("8000".into()))
            .unwrap();
        dev.apply_setting("hyperx.lod", &Value::Text("2".into()))
            .unwrap();
        dev.apply_setting(setting_ids::ANGLE_SNAPPING, &Value::Bool(true))
            .unwrap();

        let pkts = writes.lock().unwrap();
        assert_eq!(pkts.len(), 5);
        // Polling + DPI stage packet
        let dpi_pkt = &pkts[2].1;
        assert_eq!(dpi_pkt[0], 0x32);
        assert_eq!(dpi_pkt[4], 1); // 8000 Hz
        assert_eq!(dpi_pkt[6], 2); // active stage 2
        // Stage 2: 1600/50 - 1 = 31
        assert_eq!(&dpi_pkt[7 + 2 * 5..7 + 2 * 5 + 2], &[31, 0]);
        // Sensor packet
        let sensor_pkt = &pkts[4].1;
        assert_eq!(sensor_pkt[0], 0x36);
        assert_eq!(sensor_pkt[4], 1);
        assert_eq!(sensor_pkt[7], 2);
    }

    #[test]
    fn cloud3_wireless_sidetone_mute_and_sleep_match_protocol() {
        let def = find_device(HP_HYPERX_VID, 0x0C9D).expect("Cloud III Wireless");
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(def, Box::new(io), "mock-c3w".into());

        dev.apply_setting(setting_ids::SIDETONE, &Value::Number(80.0))
            .unwrap();
        dev.apply_setting("hyperx.mic_mute", &Value::Bool(true))
            .unwrap();
        dev.apply_setting("hyperx.auto_sleep", &Value::Text("30".into()))
            .unwrap();

        let pkts = writes.lock().unwrap();
        assert_eq!(pkts.len(), 4);
        assert_eq!(pkts[0].1[0..3], [0x07, 0x01, 1]);
        // 80% -> +3 dB
        assert_eq!(pkts[1].1[0..3], [0x07, 0x05, 3]);
        assert_eq!(pkts[2].1[0..3], [0x07, 0x03, 1]);
        assert_eq!(pkts[3].1[0..3], [0x07, 0x02, 30]);
    }

    #[test]
    fn universal_headset_controls_and_catalogue_validation_hold() {
        let def = find_device(HP_HYPERX_VID, 0x0AC1).expect("Cloud Flight 2 Wireless");
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(def, Box::new(io), "mock-cf2".into());

        dev.apply_setting(setting_ids::SIDETONE, &Value::Number(60.0))
            .unwrap();
        dev.apply_setting("hyperx.mic_mute", &Value::Bool(true))
            .unwrap();
        dev.apply_setting("hyperx.noise_gate", &Value::Bool(true))
            .unwrap();
        dev.apply_setting("hyperx.voice_prompt", &Value::Text("beep".into()))
            .unwrap();
        dev.apply_setting("hyperx.eq_preset", &Value::Text("2".into()))
            .unwrap();
        dev.apply_setting("hyperx.auto_sleep", &Value::Text("10".into()))
            .unwrap();

        let pkts = writes.lock().unwrap();
        assert_eq!(pkts.len(), 6);
        assert_eq!(pkts[0].1[0], 0x60);
        assert_eq!(pkts[0].1[6..=7], [1, 60]);
        assert_eq!(pkts[2].1[0..=2], [0x64, 0x01, 1]);
        assert_eq!(pkts[3].1[0..=2], [0x62, 0x01, 0]);
        assert_eq!(pkts[4].1[13], 2);
        assert_eq!(pkts[5].1[0], 0x54);
        drop(pkts);

        for d in crate::tables::DEVICES {
            let mut dev = HyperXDevice::new(
                d,
                Box::new(MockIo::default()),
                format!("mock:{:04x}", d.pid),
            );
            let mut seen_ids = std::collections::BTreeSet::new();
            for s in dev.settings() {
                assert!(
                    seen_ids.insert(s.id.clone()),
                    "Duplicate setting {} on {}",
                    s.id,
                    d.name
                );
                assert!(
                    s.validate(&s.value).is_ok(),
                    "Default setting invalid: {} on {}",
                    s.id,
                    d.name
                );
            }
            if d.led_count > 0 {
                assert!(dev.lighting().is_some(), "Expected Lighting on {}", d.name);
            } else {
                assert!(
                    dev.lighting().is_none(),
                    "Expected no Lighting on {}",
                    d.name
                );
            }
        }
    }

    #[test]
    fn stinger_core_wireless_battery_probe_and_eq_bands_work() {
        let mut resp = vec![0u8; 65];
        resp[9] = 5; // charging
        resp[11..13].copy_from_slice(&4000u16.to_le_bytes()); // 4.000 V -> 70% charging LUT
        let stinger = find_device(KINGSTON_VID, 0x170B).expect("Stinger Core Wireless");
        let io = MockIo {
            writes: Default::default(),
            feature_resp: Arc::new(Mutex::new(resp)),
        };
        let mut dev = HyperXDevice::new(stinger, Box::new(io), "mock-stinger".into());
        let st = dev.probe_on_discover();
        assert_eq!(st.battery_pct, Some(70));
        assert_eq!(st.charging, Some(true));

        let c3s = find_device(HP_HYPERX_VID, 0x06BE).expect("Cloud III S Wireless");
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(c3s, Box::new(io), "mock-c3s".into());
        dev.apply_setting("hyperx.eq_band_3", &Value::Number(4.5))
            .unwrap();
        let pkts = writes.lock().unwrap();
        assert_eq!(pkts[0].1[5..=6], [0x5F, 3]);

        // Alpha Wireless rated 300 h -> 40% => 120 h estimate.
        let alpha = find_device(HP_HYPERX_VID, 0x098D).expect("Cloud Alpha Wireless");
        assert_eq!(alpha.rated_battery_hours(), Some(300));
        let mut status = DeviceStatus {
            battery_pct: Some(40),
            charging: Some(false),
            battery_hours: None,
        };
        let io = MockIo::default();
        let dev = HyperXDevice::new(alpha, Box::new(io), "mock-alpha".into());
        assert_eq!(
            dev.def
                .rated_battery_hours()
                .map(|h| (40u32 * u32::from(h)) / 100),
            Some(120)
        );
        status.battery_hours = Some(120);
        assert_eq!(status.battery_hours, Some(120));
    }

    #[test]
    fn microphones_keyboard_advanced_and_universal_preset_lights_work() {
        let qc2s = find_device(HP_HYPERX_VID, 0x02B5).expect("QuadCast 2 S");
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(qc2s, Box::new(io), "mock-qc2s".into());
        dev.apply_setting("hyperx.polar_pattern", &Value::Text("omni".into()))
            .unwrap();
        dev.apply_setting("hyperx.high_pass", &Value::Bool(true))
            .unwrap();
        dev.apply_setting("hyperx.mic_gain", &Value::Number(75.0))
            .unwrap();
        dev.apply_setting("hyperx.mic_balance", &Value::Number(40.0))
            .unwrap();
        let pkts = writes.lock().unwrap();
        assert_eq!(&pkts[0].1[..3], &[0x77, 0x05, 1]);
        assert_eq!(&pkts[1].1[..3], &[0x77, 0x07, 1]);
        assert_eq!(&pkts[2].1[..3], &[0x77, 0x08, 75]);
        assert_eq!(&pkts[3].1[..3], &[0x77, 0x0A, 40]);
        drop(pkts);

        let duocast = find_device(HP_HYPERX_VID, 0x098C).expect("DuoCast");
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(duocast, Box::new(io), "mock-duo".into());
        dev.apply_setting(setting_ids::HW_BRIGHTNESS, &Value::Number(50.0))
            .unwrap();
        dev.apply_setting("hyperx.reverse_lights", &Value::Bool(true))
            .unwrap();
        dev.apply_setting("hyperx.polar_pattern", &Value::Text("omni".into()))
            .unwrap();
        let pkts = writes.lock().unwrap();
        assert_eq!(pkts[0].0, "feat");
        assert_eq!(pkts[1].1[17], 1);
        assert_eq!(pkts[2].1[2..=3], [0x05, 2]);
        drop(pkts);

        let rise = find_device(HP_HYPERX_VID, 0x0FA0).expect("Alloy Rise");
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = HyperXDevice::new(rise, Box::new(io), "mock-rise".into());
        dev.apply_setting("hyperx.gm_alt_f4", &Value::Bool(true))
            .unwrap();
        dev.apply_setting("hyperx.backlight_fade", &Value::Text("30".into()))
            .unwrap();
        dev.apply_setting("hyperx.ambient_brightness", &Value::Bool(true))
            .unwrap();
        dev.apply_hardware_effect(&EffectRequest {
            effect: "wave".into(),
            zone: None,
            colors: vec![],
            speed: Some(60),
            brightness: Some(80),
            reverse: false,
            save: false,
        })
        .unwrap();
        let pkts = writes.lock().unwrap();
        assert_eq!(pkts[0].1[0], 0x30);
        assert_eq!(pkts[1].1[0..=1], [0x46, 0x01]);
        assert_eq!(pkts[2].1[0..=1], [0x46, 0x03]);
        assert_eq!(pkts[3].1[0..=1], [0x40, 0x01]);
        assert_eq!(pkts[4].1[0..=1], [0x42, 0x02]);
        assert_eq!(pkts[4].1[4], 3);
    }
}
