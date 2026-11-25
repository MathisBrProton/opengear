//! Shared `Driver`, `Device`, and `Lighting` implementation for SteelSeries devices.
//!
//! Safety guards enforced per `docs/protocols/steelseries.md`:
//! - Never sends `0x2D` DPI-reset init on Aerox mice.
//! - Excludes `0x06` disco mode on Rival 3/Aerox (photosensitive safety).
//! - Caps Rival 5 to 2 FPS (auto-persistence flash wear protection).
//! - Always sends `0x3B` / `0x41` on Apex G1-G3 `release()` and `Drop`.

use crate::proto::{
    HidSsIo, SsIo, aerox_reactive_pkt, aerox_wired_color_pkt, aerox_wireless_zone_pkt,
    apex3_colors_pkt, apex3_tkl_colors_pkt, apex_direct_frame_pkt, apex_onboard_pkt,
    qck_prism_frame_pkt, rival3_color_pkt, rival3_effect_pkt, rival3_wireless_color_pkt,
    rival5_colors_pkt,
};
use crate::tables::{APEX_KEYS, ProtocolFamily, STEELSERIES_VID, SsDeviceDef, find_device};
use anyhow::Result;
use opengear_core::Rgb;
use opengear_core::driver::{
    Connection, Device, DeviceInfo, Driver, Section, Setting, Value, setting_ids,
};
use opengear_core::lighting::{
    EffectParams, EffectRequest, HardwareEffect, Led, Lighting, Zone, ZoneKind,
};
use std::collections::HashSet;

pub const DRIVER_NAME: &str = "steelseries";

pub struct SteelSeriesDriver;

impl Driver for SteelSeriesDriver {
    fn name(&self) -> &'static str {
        DRIVER_NAME
    }

    fn display_name(&self) -> &'static str {
        "SteelSeries (Apex, Aerox, Rival, Arctis, QcK)"
    }

    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[STEELSERIES_VID]
    }

    fn experimental(&self) -> bool {
        true
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        let Ok(api) = hidapi::HidApi::new() else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        let mut out: Vec<Box<dyn Device>> = Vec::new();
        for dev in api.device_list() {
            if dev.vendor_id() != STEELSERIES_VID {
                continue;
            }
            let Some(def) = find_device(dev.product_id()) else {
                continue;
            };
            if dev.interface_number() != def.interface {
                continue;
            }
            if let Some(up) = def.usage_page
                && dev.usage_page() != up
            {
                continue;
            }
            if let Some(u) = def.usage
                && dev.usage() != u
            {
                continue;
            }
            let path = dev.path().to_string_lossy().into_owned();
            if !seen.insert((def.pid, path.clone())) {
                continue;
            }
            if let Ok(hid) = dev.open_device(&api) {
                out.push(Box::new(SteelSeriesDevice::new(
                    def,
                    Box::new(HidSsIo(hid)),
                    path,
                )));
            }
        }
        out
    }
}

pub struct SteelSeriesDevice {
    def: &'static SsDeviceDef,
    io: Box<dyn SsIo>,
    path_tag: String,
    brightness_pct: u8,
    g3_initialized: bool,
    apex_live_active: bool,
    last_brightness_sent: Option<u8>,
    rival5_reactive_cleared: bool,
}

impl SteelSeriesDevice {
    pub fn new(def: &'static SsDeviceDef, io: Box<dyn SsIo>, path_tag: String) -> Self {
        Self {
            def,
            io,
            path_tag,
            brightness_pct: 100,
            g3_initialized: false,
            apex_live_active: false,
            last_brightness_sent: None,
            rival5_reactive_cleared: false,
        }
    }

    fn is_apex_perkey(&self) -> bool {
        matches!(
            self.def.family,
            ProtocolFamily::ApexG1 | ProtocolFamily::ApexG2 { .. } | ProtocolFamily::ApexG3 { .. }
        )
    }

    fn is_g3(&self) -> bool {
        matches!(self.def.family, ProtocolFamily::ApexG3 { .. })
    }

    fn release_apex_if_needed(&mut self) {
        if self.apex_live_active && self.is_apex_perkey() {
            let pkt = apex_onboard_pkt(self.is_g3());
            let _ = self.io.write_output(&pkt);
            self.apex_live_active = false;
        }
    }
}

impl Drop for SteelSeriesDevice {
    fn drop(&mut self) {
        self.release_apex_if_needed();
    }
}

impl Device for SteelSeriesDevice {
    fn info(&self) -> DeviceInfo {
        let slug: u32 = self
            .path_tag
            .bytes()
            .fold(0x811C_9DC5u32, |h, b| h.wrapping_mul(0x0100_0193) ^ u32::from(b));
        DeviceInfo {
            id: format!("{DRIVER_NAME}:1038:{:04x}:{slug:08x}", self.def.pid),
            vendor: "SteelSeries".into(),
            model: self.def.name.into(),
            kind: self.def.kind,
            connection: if self.def.wireless {
                Connection::Wireless
            } else {
                Connection::Usb
            },
            firmware: None,
        }
    }

    fn settings(&self) -> Vec<Setting> {
        match self.def.family {
            ProtocolFamily::ApexG1
            | ProtocolFamily::ApexG2 { .. }
            | ProtocolFamily::ApexG3 { .. }
            | ProtocolFamily::Apex3
            | ProtocolFamily::Apex3Tkl
            | ProtocolFamily::Aerox3Wired
            | ProtocolFamily::Aerox5Wired
            | ProtocolFamily::AeroxWireless { .. } => {
                vec![Setting::slider(
                    setting_ids::HW_BRIGHTNESS,
                    "Luminosité matérielle",
                    Section::Lighting,
                    (0.0, 100.0, 10.0),
                    "%",
                    f64::from(self.brightness_pct),
                )]
            }
            _ => Vec::new(),
        }
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        if id != setting_ids::HW_BRIGHTNESS {
            anyhow::bail!("Réglage SteelSeries inconnu : {id}");
        }
        let pct = value.as_f64().unwrap_or(100.0).round().clamp(0.0, 100.0) as u8;
        self.brightness_pct = pct;
        match self.def.family {
            ProtocolFamily::ApexG1
            | ProtocolFamily::ApexG2 { .. }
            | ProtocolFamily::ApexG3 { .. } => {
                let mut pkt = [0u8; 65];
                pkt[1] = 0x23;
                pkt[2] = ((pct as u16 * 10 + 50) / 100).min(10) as u8;
                self.io.write_output(&pkt)?;
            }
            ProtocolFamily::Apex3 => {
                let mut pkt = [0u8; 33];
                pkt[1] = 0x0A;
                pkt[3] = pct;
                self.io.write_output(&pkt)?;
            }
            ProtocolFamily::Apex3Tkl => {
                let mut pkt = [0u8; 65];
                pkt[1] = 0x23;
                pkt[2] = ((pct as u16 * 16 + 50) / 100).min(16) as u8;
                self.io.write_output(&pkt)?;
            }
            ProtocolFamily::Aerox3Wired | ProtocolFamily::Aerox5Wired => {
                let mut pkt = [0u8; 65];
                pkt[1] = 0x23;
                pkt[2] = pct;
                self.io.write_output(&pkt)?;
            }
            ProtocolFamily::AeroxWireless { is_24g } => {
                let mut pkt = [0u8; 64];
                pkt[1] = 0x23 | if is_24g { 0x40 } else { 0 };
                pkt[2] = pct / 10;
                self.io.write_output(&pkt)?;
                if is_24g {
                    let mut ack = [0u8; 64];
                    let _ = self.io.read_input(&mut ack, 250);
                }
            }
            _ => {}
        }
        self.last_brightness_sent = Some(pct);
        Ok(())
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        Some(self)
    }
}

impl Lighting for SteelSeriesDevice {
    fn zones(&self) -> Vec<Zone> {
        match self.def.family {
            ProtocolFamily::ApexG1
            | ProtocolFamily::ApexG2 { .. }
            | ProtocolFamily::ApexG3 { .. } => {
                let leds = APEX_KEYS
                    .iter()
                    .map(|&(_code, label, w3c, nx, ny)| Led::at(label, nx, ny).key(w3c))
                    .collect();
                vec![Zone {
                    id: "keys".into(),
                    name: "Clavier par touche".into(),
                    kind: ZoneKind::Matrix,
                    leds,
                    resizable: None,
                }]
            }
            ProtocolFamily::QckPrism => vec![
                Zone::single("bottom", "Bas du tapis", 0.5, 0.8),
                Zone::single("top", "Haut du tapis", 0.5, 0.2),
            ],
            _ => vec![Zone::linear("main", self.def.name, self.def.led_count, 0.5)],
        }
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        match self.def.family {
            ProtocolFamily::Rival3 => vec![
                HardwareEffect {
                    id: "static".into(),
                    label: "Statique".into(),
                    params: EffectParams {
                        colors: (1, 1),
                        brightness: Some((0, 100)),
                        ..Default::default()
                    },
                    zones: Vec::new(),
                },
                HardwareEffect {
                    id: "breathing".into(),
                    label: "Respiration".into(),
                    params: EffectParams {
                        colors: (1, 1),
                        speed: Some((0, 2)),
                        brightness: Some((0, 100)),
                        ..Default::default()
                    },
                    zones: Vec::new(),
                },
                HardwareEffect {
                    id: "spectrum".into(),
                    label: "Cycle spectral".into(),
                    params: EffectParams::default(),
                    zones: Vec::new(),
                },
                HardwareEffect {
                    id: "rainbow_breathing".into(),
                    label: "Respiration arc-en-ciel".into(),
                    params: EffectParams::default(),
                    zones: Vec::new(),
                },
            ],
            ProtocolFamily::AeroxWireless { .. } => vec![HardwareEffect {
                id: "rainbow".into(),
                label: "Arc-en-ciel".into(),
                params: EffectParams::default(),
                zones: Vec::new(),
            }],
            ProtocolFamily::Rival5 => vec![HardwareEffect {
                id: "rainbow".into(),
                label: "Arc-en-ciel".into(),
                params: EffectParams::default(),
                zones: Vec::new(),
            }],
            ProtocolFamily::Apex3Tkl => vec![HardwareEffect {
                id: "rainbow_wave".into(),
                label: "Vague arc-en-ciel".into(),
                params: EffectParams::default(),
                zones: Vec::new(),
            }],
            _ => Vec::new(),
        }
    }

    fn max_fps(&self) -> u8 {
        match self.def.family {
            // §3.4/§7: Rival 5 auto-persists state -> keep cadence very low to protect flash!
            ProtocolFamily::Rival5 => 2,
            ProtocolFamily::AeroxWireless { is_24g: true } => 10,
            _ => 25,
        }
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        let first = colors.first().copied().unwrap_or(Rgb::BLACK);
        match self.def.family {
            ProtocolFamily::Rival3 => {
                for z in 1..=4 {
                    let c = colors.get(z - 1).copied().unwrap_or(first);
                    self.io
                        .write_output(&rival3_color_pkt(z as u8, c, self.brightness_pct))?;
                }
                self.io.write_output(&rival3_effect_pkt(0x04))?;
            }
            ProtocolFamily::Aerox3Wired => {
                self.io
                    .write_output(&aerox_wired_color_pkt(colors, false))?;
            }
            ProtocolFamily::Aerox5Wired => {
                self.io.write_output(&aerox_wired_color_pkt(colors, true))?;
                std::thread::sleep(std::time::Duration::from_millis(2));
                if let Some(&c3) = colors.get(3) {
                    self.io.write_output(&aerox_reactive_pkt(c3, false))?;
                }
            }
            ProtocolFamily::AeroxWireless { is_24g } => {
                for z in 0..self.def.led_count.min(3) {
                    let c = colors.get(z).copied().unwrap_or(first);
                    self.io
                        .write_output(&aerox_wireless_zone_pkt(z as u8, c, is_24g))?;
                    if is_24g {
                        let mut ack = [0u8; 64];
                        let _ = self.io.read_input(&mut ack, 250);
                    }
                }
                if self.def.led_count > 3 {
                    let c3 = colors.get(3).copied().unwrap_or(first);
                    self.io.write_output(&aerox_reactive_pkt(c3, is_24g))?;
                    if is_24g {
                        let mut ack = [0u8; 64];
                        let _ = self.io.read_input(&mut ack, 250);
                    }
                }
            }
            ProtocolFamily::Rival3Wireless => {
                self.io.write_output(&rival3_wireless_color_pkt(first))?;
            }
            ProtocolFamily::Rival5 => {
                if !self.rival5_reactive_cleared {
                    let mut off = [0u8; 64];
                    off[1] = 0x26;
                    self.io.send_feature(&off)?;
                    self.rival5_reactive_cleared = true;
                }
                self.io.send_feature(&rival5_colors_pkt(colors))?;
            }
            ProtocolFamily::Rival100 => {
                self.io
                    .write_output(&[0x00, 0x05, 0x00, first.r, first.g, first.b, 0, 0, 0, 0])?;
                self.io
                    .write_output(&[0x00, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])?;
            }
            ProtocolFamily::Rival300 | ProtocolFamily::Sensei310 => {
                for z in 0..2 {
                    let c = colors.get(z).copied().unwrap_or(first);
                    if matches!(self.def.family, ProtocolFamily::Rival300) {
                        self.io
                            .write_output(&[0x00, 0x08, (z + 1) as u8, c.r, c.g, c.b, 0, 0, 0, 0])?;
                        self.io
                            .write_output(&[0x00, 0x07, (z + 1) as u8, 0x01, 0, 0, 0, 0, 0, 0])?;
                    } else {
                        let mut pkt = [0u8; 66];
                        pkt[1] = 0x5B;
                        pkt[3] = z as u8;
                        pkt[0x14] = 0x01;
                        pkt[0x1C] = 0x01;
                        pkt[0x1D..0x20].copy_from_slice(&[c.r, c.g, c.b]);
                        pkt[0x20..0x23].copy_from_slice(&[c.r, c.g, c.b]);
                        self.io.write_output(&pkt)?;
                    }
                }
            }
            ProtocolFamily::Siberia350 => {
                for cmd in [
                    [0x01, 0x00, 0x95, 2, 0x80, 0xBF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    [0x01, 0x00, 0x80, 2, 0x52, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    [0x01, 0x00, 0x83, 3, first.r, first.g, first.b, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    [0x01, 0x00, 0x93, 2, 0x03, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                ] {
                    self.io.write_output(&cmd)?;
                }
            }
            ProtocolFamily::Arctis5 => {
                for z in 0..2u8 {
                    let c = colors.get(z as usize).copied().unwrap_or(first);
                    let p1 = {
                        let mut b = [0u8; 37];
                        b[0] = 0x06;
                        b[1..5].copy_from_slice(&[0x81, 0x43, 0x01, 0x22]);
                        b
                    };
                    let p2 = {
                        let mut b = [0u8; 37];
                        b[0] = 0x06;
                        b[1..5].copy_from_slice(&[0x81, 0x43, 0x01, 0x23]);
                        b
                    };
                    let p3 = {
                        let mut b = [0u8; 37];
                        b[0] = 0x06;
                        b[1..14].copy_from_slice(&[
                            0x8A, 0x42, 0x00, 0x20, 0x41, 0x00, c.r, c.g, c.b, 0xFF, 0x32, 0xC8,
                            0xC8,
                        ]);
                        b
                    };
                    let p4 = {
                        let mut b = [0u8; 37];
                        b[0] = 0x06;
                        b[1..9].copy_from_slice(&[0x8A, 0x42, 0x00, 0x20, 0x41, 0x08, z, 0x01]);
                        b
                    };
                    let p5 = {
                        let mut b = [0u8; 37];
                        b[0] = 0x06;
                        b[1..7].copy_from_slice(&[0x8A, 0x42, 0x00, 0x20, 0x60, z]);
                        b
                    };
                    let p6 = {
                        let mut b = [0u8; 37];
                        b[0] = 0x06;
                        b[1..6].copy_from_slice(&[0x8A, 0x42, 0x00, 0x20, 0x05]);
                        b
                    };
                    for p in [p1, p2, p3, p4, p5, p6] {
                        self.io.write_output(&p)?;
                    }
                }
            }
            ProtocolFamily::ArctisNova3 => {
                for z in 0..2u8 {
                    let c = colors.get(z as usize).copied().unwrap_or(first);
                    let mut fx = vec![0u8; 521];
                    fx[0] = 0x06;
                    fx[1] = 0xAA;
                    fx[2] = if z == 0 { 0x01 } else { 0x00 };
                    fx[0x0D] = 0x98;
                    fx[0x0E] = 0x3A;
                    fx[0x15] = 0x55;
                    fx[0x17] = 0x96;
                    fx[0x1D] = 0x01;
                    fx[0x25] = 0x02;
                    fx[0x2D..0x30].copy_from_slice(&[c.r, c.g, c.b]);
                    fx[0x30] = 0xFF;
                    self.io.write_output(&fx)?;

                    let mut sel = [0u8; 64];
                    sel[0] = 0x06;
                    sel[1] = 0xA5;
                    sel[2] = if z == 0 { 0x02 } else { 0x01 };
                    sel[3] = 0x0A;
                    self.io.write_output(&sel)?;

                    let mut app = [0u8; 64];
                    app[0] = 0x06;
                    app[1] = if z == 0 { 0x09 } else { 0xA3 };
                    self.io.write_output(&app)?;
                }
            }
            ProtocolFamily::QckPrism => {
                let bottom = colors.first().copied().unwrap_or(Rgb::BLACK);
                let top = colors.get(1).copied().unwrap_or(bottom);
                self.io.send_feature(&qck_prism_frame_pkt(bottom, top))?;
                let mut app = [0u8; 65];
                app[1] = 0x0D;
                self.io.write_output(&app)?;
            }
            ProtocolFamily::ApexG1
            | ProtocolFamily::ApexG2 { .. }
            | ProtocolFamily::ApexG3 { .. } => {
                if self.is_g3() && !self.g3_initialized {
                    let mut init = [0u8; 65];
                    init[1] = 0x4B;
                    self.io.send_feature(&init)?;
                    self.g3_initialized = true;
                }
                let (report_id, short_frame) = match self.def.family {
                    ProtocolFamily::ApexG1 => (0x3A, false),
                    ProtocolFamily::ApexG2 { short_frame } => (0x40, short_frame),
                    ProtocolFamily::ApexG3 { wireless, short_frame } => {
                        (if wireless { 0x61 } else { 0x40 }, short_frame)
                    }
                    _ => unreachable!(),
                };
                let entries: Vec<(u8, Rgb)> = APEX_KEYS
                    .iter()
                    .enumerate()
                    .map(|(i, &(code, _, _, _, _))| (code, colors.get(i).copied().unwrap_or(first)))
                    .collect();
                let pkt = apex_direct_frame_pkt(report_id, short_frame, &entries);
                self.io.send_feature(&pkt)?;
                self.apex_live_active = true;
            }
            ProtocolFamily::ApexM750 => {
                if !self.g3_initialized {
                    for mut p in [[0u8; 513], [0u8; 513], [0u8; 513]] {
                        let _ = p[0];
                        let _ = &mut p;
                    }
                    let mut p1 = [0u8; 513];
                    p1[4] = 0x01;
                    p1[6] = 0x85;
                    let mut p2 = [0u8; 513];
                    p2[4] = 0x03;
                    p2[5] = 0x01;
                    p2[7] = 0xFF;
                    let mut p3 = [0u8; 513];
                    p3[4] = 0x01;
                    p3[6] = 0x85;
                    p3[7] = 0xFF;
                    for p in [p1, p2, p3] {
                        self.io.send_feature(&p)?;
                    }
                    self.g3_initialized = true;
                }
                let mut frame = vec![0u8; 513];
                frame[3..9].copy_from_slice(&[0x01, 0x8E, 0x01, 0x03, 0x06, 0x16]);
                for i in 0..132 {
                    let c = colors.get(i).copied().unwrap_or(Rgb::new(0xFF, 0x32, 0x00));
                    let off = 9 + i * 3;
                    frame[off..off + 3].copy_from_slice(&[c.r, c.g, c.b]);
                }
                self.io.send_feature(&frame)?;
            }
            ProtocolFamily::ApexOg => {
                let mut pkt = [0u8; 33];
                pkt[1] = 0x07;
                for z in 0..5 {
                    let c = colors.get(z).copied().unwrap_or(first);
                    let off = 3 + z * 4;
                    pkt[off..off + 4].copy_from_slice(&[c.r, c.g, c.b, 0x08]);
                }
                self.io.write_output(&pkt)?;
            }
            ProtocolFamily::Apex3 => {
                let mut br = [0u8; 33];
                br[1] = 0x0A;
                br[3] = self.brightness_pct;
                self.io.write_output(&br)?;
                self.io.write_output(&apex3_colors_pkt(colors))?;
            }
            ProtocolFamily::Apex3Tkl => {
                self.io.write_output(&apex3_tkl_colors_pkt(colors))?;
                if self.last_brightness_sent != Some(self.brightness_pct) {
                    let mut br = [0u8; 65];
                    br[1] = 0x23;
                    br[2] = ((self.brightness_pct as u16 * 16 + 50) / 100).min(16) as u8;
                    self.io.write_output(&br)?;
                    self.last_brightness_sent = Some(self.brightness_pct);
                }
            }
        }
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        let color = req.colors.first().copied().unwrap_or(Rgb::CYAN);
        match self.def.family {
            ProtocolFamily::Rival3 => {
                let br = req.brightness.unwrap_or(self.brightness_pct).min(100);
                let fx = match req.effect.as_str() {
                    "static" => 0x04,
                    "breathing" => 0x03u8.saturating_sub(req.speed.unwrap_or(1).min(2)),
                    "spectrum" => 0x00,
                    "rainbow_breathing" => 0x05,
                    _ => anyhow::bail!("Effet Rival 3 non supporté : {}", req.effect),
                };
                for z in 1..=4 {
                    self.io.write_output(&rival3_color_pkt(z, color, br))?;
                }
                self.io.write_output(&rival3_effect_pkt(fx))?;
                if req.save {
                    let mut sv = [0u8; 10];
                    sv[1] = 0x09;
                    self.io.write_output(&sv)?;
                }
                Ok(())
            }
            ProtocolFamily::AeroxWireless { is_24g } if req.effect == "rainbow" => {
                let mut pkt = [0u8; 64];
                pkt[1] = 0x22 | if is_24g { 0x40 } else { 0 };
                pkt[2] = 0xFF;
                self.io.write_output(&pkt)?;
                if is_24g {
                    let mut ack = [0u8; 64];
                    let _ = self.io.read_input(&mut ack, 250);
                }
                Ok(())
            }
            ProtocolFamily::Rival5 if req.effect == "rainbow" => {
                let mut pkt = [0u8; 64];
                pkt[1] = 0x22;
                pkt[2] = 0xFF;
                pkt[3] = 0x03;
                self.io.send_feature(&pkt)
            }
            ProtocolFamily::Apex3Tkl if req.effect == "rainbow_wave" => {
                let mut pkt = [0u8; 65];
                pkt[1] = 0x22;
                pkt[2] = 0xFF;
                self.io.write_output(&pkt)
            }
            _ => anyhow::bail!("Effet matériel SteelSeries non supporté : {}", req.effect),
        }
    }

    fn release(&mut self) -> Result<()> {
        self.release_apex_if_needed();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MockIo {
        writes: Arc<Mutex<Vec<(&'static str, Vec<u8>)>>>,
    }

    impl SsIo for MockIo {
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
    }

    #[test]
    fn aerox_never_sends_dpi_reset_0x2d_on_init_or_frame() {
        let def = find_device(0x1836).unwrap();
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = SteelSeriesDevice::new(def, Box::new(io), "mock".into());
        dev.set_frame(&[Rgb::RED, Rgb::GREEN, Rgb::BLUE]).unwrap();
        let pkts = writes.lock().unwrap();
        assert!(!pkts.is_empty());
        assert!(pkts.iter().all(|(_, p)| p.get(1) != Some(&0x2D)));
    }

    #[test]
    fn apex_release_and_drop_restore_onboard_mode() {
        let def = find_device(0x1642).unwrap(); // G3 wired
        let io = MockIo::default();
        let writes = io.writes.clone();
        let mut dev = SteelSeriesDevice::new(def, Box::new(io), "mock".into());
        dev.set_frame(&[Rgb::CYAN; 112]).unwrap();
        dev.release().unwrap();
        let pkts = writes.lock().unwrap();
        assert_eq!(pkts[0].1[1], 0x4B); // G3 init feature
        assert_eq!(pkts[1].1.len(), 643); // exact direct frame len
        assert_eq!(pkts.last().unwrap().1[1], 0x41); // G3 onboard restore
    }

    #[test]
    fn rival5_caps_fps_and_rival3_excludes_disco() {
        let r5 = find_device(0x183C).unwrap();
        let dev5 = SteelSeriesDevice::new(r5, Box::new(MockIo::default()), "mock".into());
        assert_eq!(dev5.max_fps(), 2);

        let r3 = find_device(0x184C).unwrap();
        let dev3 = SteelSeriesDevice::new(r3, Box::new(MockIo::default()), "mock".into());
        assert!(dev3.hardware_effects().iter().all(|e| e.id != "disco"));
    }

    #[test]
    fn all_pid_entries_are_unique_or_explicitly_documented() {
        let mut seen = HashSet::new();
        for d in crate::tables::DEVICES {
            assert!(seen.insert((d.pid, d.interface)), "Duplicate PID:IF {:04X}:{}", d.pid, d.interface);
        }
    }
}
