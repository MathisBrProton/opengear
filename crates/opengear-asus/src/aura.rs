use anyhow::Context;
use hidapi::{HidApi, HidDevice};
use opengear_core::Rgb;
use opengear_core::driver::{
    Connection, Device, DeviceInfo, DeviceKind, DeviceStatus, Driver, Setting, Value,
};
use opengear_core::lighting::{
    EffectParams, EffectRequest, HardwareEffect, Lighting, Zone, ZoneKind,
};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use thiserror::Error;
use tracing::{debug, info};

pub const ASUS_VID: u16 = 0x0B05;
pub const AURA_MB_PIDS: &[u16] = &[0x18F3, 0x1939, 0x19AF, 0x1AA6, 0x1BED];
pub const AURA_USAGE_PAGE: u16 = 0xFF72;
pub const AURA_USAGE: u16 = 0x00A1;

const REPORT_ID: u8 = 0xEC;
const PACKET_LEN: usize = 65;
const LEDS_PER_PACKET: usize = 20; // 0x14

const CMD_EFFECT: u8 = 0x35;
const CMD_EFFECT_COLOR: u8 = 0x36;
const CMD_COMMIT: u8 = 0x3F;
const CMD_DIRECT: u8 = 0x40;
const CMD_FIRMWARE: u8 = 0x82;
const CMD_CONFIG_TABLE: u8 = 0xB0;

const DIRECT_APPLY_FLAG: u8 = 0x80;
const DIRECT_OVERFLOW_FLAG: u8 = 0x10;
const MAX_ARGB_HEADERS: u8 = 16;
const MAX_DIRECT_LEDS: usize = 511;

#[derive(Debug, Error)]
pub enum AuraError {
    #[error("Aucun contrôleur ASUS AURA USB détecté (VID 0B05, UsagePage FF72)")]
    DeviceNotFound,
    #[error("Erreur HID : {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("Impossible de lire la table de configuration Aura (réponse invalide)")]
    InvalidConfigTable,
    #[error("Index de canal Aura invalide ({0})")]
    InvalidChannel(usize),
    #[error("Plage de LED Aura hors limites (start={start}, count={count})")]
    LedRangeOutOfBounds { start: usize, count: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AuraHwMode {
    Off = 0,
    Static = 1,
    Breathing = 2,
    Flashing = 3,
    SpectrumCycle = 4,
    Rainbow = 5,
    SpectrumCycleBreathing = 6,
    ChaseFade = 7,
    SpectrumCycleChaseFade = 8,
    Chase = 9,
    SpectrumCycleChase = 10,
    Wave = 11,
    ChaseRainbowPulse = 12,
    RandomFlicker = 13,
    GentleTransition = 16,
    WavePropagation = 17,
    RedPulse = 19,
    Direct = 0xFF,
}

impl AuraHwMode {
    pub const HARDWARE_MODES: &'static [Self] = &[
        Self::Static,
        Self::Breathing,
        Self::Rainbow,
        Self::SpectrumCycle,
        Self::SpectrumCycleBreathing,
        Self::Wave,
        Self::ChaseFade,
        Self::SpectrumCycleChaseFade,
        Self::Chase,
        Self::SpectrumCycleChase,
        Self::Flashing,
        Self::RandomFlicker,
        Self::GentleTransition,
        Self::WavePropagation,
        Self::RedPulse,
        Self::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Éteint",
            Self::Static => "Statique",
            Self::Breathing => "Respiration",
            Self::Flashing => "Flash",
            Self::SpectrumCycle => "Cycle de spectre",
            Self::Rainbow => "Arc-en-ciel",
            Self::SpectrumCycleBreathing => "Respiration Arc-en-ciel",
            Self::ChaseFade => "Comète (Fondu)",
            Self::SpectrumCycleChaseFade => "Comète Arc-en-ciel",
            Self::Chase => "Poursuite",
            Self::SpectrumCycleChase => "Poursuite Arc-en-ciel",
            Self::Wave => "Vague",
            Self::ChaseRainbowPulse => "Pulsation Arc-en-ciel",
            Self::RandomFlicker => "Scintillement",
            Self::GentleTransition => "Transition douce",
            Self::WavePropagation => "Propagation d'onde",
            Self::RedPulse => "Pulsation ROG",
            Self::Direct => "Direct (OpenGear Sync)",
        }
    }

    pub fn uses_color(self) -> bool {
        matches!(
            self,
            Self::Static | Self::Breathing | Self::Flashing | Self::ChaseFade | Self::Chase
        )
    }

    pub fn from_str_id(s: &str) -> Self {
        match s {
            "Off" => Self::Off,
            "Static" => Self::Static,
            "Breathing" => Self::Breathing,
            "Flashing" => Self::Flashing,
            "SpectrumCycle" => Self::SpectrumCycle,
            "Rainbow" => Self::Rainbow,
            "SpectrumCycleBreathing" => Self::SpectrumCycleBreathing,
            "ChaseFade" => Self::ChaseFade,
            "SpectrumCycleChaseFade" => Self::SpectrumCycleChaseFade,
            "Chase" => Self::Chase,
            "SpectrumCycleChase" => Self::SpectrumCycleChase,
            "Wave" => Self::Wave,
            "ChaseRainbowPulse" => Self::ChaseRainbowPulse,
            "RandomFlicker" => Self::RandomFlicker,
            "GentleTransition" => Self::GentleTransition,
            "WavePropagation" => Self::WavePropagation,
            "RedPulse" => Self::RedPulse,
            "Direct" => Self::Direct,
            _ => Self::Static,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuraChannelKind {
    /// Fixed motherboard onboard zones + 12V 4-pin RGB headers
    MainboardFixed,
    /// 5V 3-pin Addressable Gen2 RGB header
    AddressableGen2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraChannel {
    pub index: usize,
    pub name: String,
    pub kind: AuraChannelKind,
    pub effect_channel: u8,
    pub direct_channel: u8,
    /// Number of fixed LEDs reported by firmware (for Addressable headers, user can customize `led_count`).
    pub default_led_count: usize,
    pub led_count: usize,
    pub rgb_headers_12v: u8,
}

#[derive(Debug, Clone)]
pub struct AuraMainboardInfo {
    pub vid: u16,
    pub pid: u16,
    pub product_name: String,
    pub firmware_version: String,
    pub config_table_hex: String,
    pub mainboard_fixed_leds: u8,
    pub rgb_12v_headers: u8,
    pub argb_5v_headers: u8,
    pub channels: Vec<AuraChannel>,
}

pub(crate) trait AuraIo: Send {
    fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError>;
    fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, hidapi::HidError>;
}

impl AuraIo for HidDevice {
    fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError> {
        HidDevice::write(self, data)
    }

    fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, hidapi::HidError> {
        HidDevice::read_timeout(self, buf, timeout_ms)
    }
}

pub struct AuraMainboardController {
    dev: Box<dyn AuraIo>,
    pub info: AuraMainboardInfo,
    channel_direct: Vec<bool>,
}

/// Build the 65-byte Aura Direct packets for `colors`, splitting cleanly at LED 256 so a single
/// packet never straddles the 8-bit `offset` wrap boundary.
pub(crate) fn build_direct_packets(direct_channel: u8, colors: &[Rgb]) -> Vec<[u8; PACKET_LEN]> {
    let total = colors.len().min(MAX_DIRECT_LEDS);
    if total == 0 {
        return Vec::new();
    }

    let mut packets = Vec::new();
    let mut configured = 0usize;
    while configured < total {
        let in_high_bank = configured >= 256;
        let bank_end = if in_high_bank { total } else { total.min(256) };
        let bank_remaining = bank_end - configured;
        let in_pkt = bank_remaining.min(LEDS_PER_PACKET);
        let is_final = configured + in_pkt == total;

        let mut command = direct_channel;
        if in_high_bank {
            command |= DIRECT_OVERFLOW_FLAG;
        }
        if is_final {
            command |= DIRECT_APPLY_FLAG;
        }

        let offset = (configured % 256) as u8;
        let mut buf = [0u8; PACKET_LEN];
        buf[0] = REPORT_ID;
        buf[1] = CMD_DIRECT;
        buf[2] = command;
        buf[3] = offset;
        buf[4] = in_pkt as u8;

        for i in 0..in_pkt {
            let c = colors[configured + i];
            buf[5 + i * 3] = c.r;
            buf[6 + i * 3] = c.g;
            buf[7 + i * 3] = c.b;
        }
        packets.push(buf);
        configured += in_pkt;
    }
    packets
}

impl AuraMainboardController {
    pub fn connect() -> Result<Self, AuraError> {
        let api = HidApi::new()?;

        let found = api
            .device_list()
            .find(|d| {
                d.vendor_id() == ASUS_VID
                    && AURA_MB_PIDS.contains(&d.product_id())
                    && d.usage_page() == AURA_USAGE_PAGE
                    && d.usage() == AURA_USAGE
            })
            .or_else(|| {
                api.device_list().find(|d| {
                    d.vendor_id() == ASUS_VID
                        && AURA_MB_PIDS.contains(&d.product_id())
                        && d.usage_page() == AURA_USAGE_PAGE
                })
            })
            .ok_or(AuraError::DeviceNotFound)?;

        let path: CString = found.path().to_owned();
        let vid = found.vendor_id();
        let pid = found.product_id();
        let product_name = found
            .product_string()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("ASUS AURA LED Controller")
            .to_string();

        let dev: Box<dyn AuraIo> = Box::new(api.open_path(&path)?);

        let firmware_version = Self::read_firmware_version(dev.as_ref())?;
        let config_table = Self::read_config_table(dev.as_ref())?;

        let mainboard_fixed_leds = config_table[0x1B];
        let mut rgb_12v_headers = config_table[0x1D].min(8);
        let argb_5v_headers = config_table[0x02].min(MAX_ARGB_HEADERS);
        if mainboard_fixed_leds < rgb_12v_headers {
            rgb_12v_headers = 0;
        }

        let mut channels = Vec::new();
        let mut effect_channel: u8 = 0;
        let argb_from_zero = firmware_version.starts_with("AULA3-AR32");

        if mainboard_fixed_leds > 0 {
            let mb_eff_ch = if argb_from_zero {
                argb_5v_headers
            } else {
                let c = effect_channel;
                effect_channel = effect_channel.saturating_add(1);
                c
            };
            channels.push(AuraChannel {
                index: channels.len(),
                name: "Carte mère (Zones intégrées & RGB 12V)".to_string(),
                kind: AuraChannelKind::MainboardFixed,
                effect_channel: mb_eff_ch,
                direct_channel: 0x04,
                default_led_count: mainboard_fixed_leds as usize,
                led_count: mainboard_fixed_leds as usize,
                rgb_headers_12v: rgb_12v_headers,
            });
        }

        for i in 0..argb_5v_headers {
            channels.push(AuraChannel {
                index: channels.len(),
                name: format!("Header ARGB Gen2 #{}", i + 1),
                kind: AuraChannelKind::AddressableGen2,
                effect_channel,
                direct_channel: i,
                default_led_count: 20,
                led_count: 20,
                rgb_headers_12v: 0,
            });
            effect_channel = effect_channel.saturating_add(1);
        }

        let config_table_hex = config_table
            .iter()
            .take(36)
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");

        let channel_count = channels.len();
        let info = AuraMainboardInfo {
            vid,
            pid,
            product_name,
            firmware_version,
            config_table_hex,
            mainboard_fixed_leds,
            rgb_12v_headers,
            argb_5v_headers,
            channels,
        };

        info!(
            "ASUS AURA connecté ({:04X}:{:04X}) FW={} — {} LEDs CM, {} headers 12V RGB, {} headers 5V ARGB",
            info.vid,
            info.pid,
            info.firmware_version,
            info.mainboard_fixed_leds,
            info.rgb_12v_headers,
            info.argb_5v_headers
        );

        let mut ctrl = Self {
            dev,
            info,
            channel_direct: vec![false; channel_count],
        };
        if ctrl.info.argb_5v_headers > 0 {
            ctrl.set_gen1()?;
        }
        Ok(ctrl)
    }

    fn read_firmware_version(dev: &dyn AuraIo) -> Result<String, AuraError> {
        let mut buf = [0u8; PACKET_LEN];
        buf[0] = REPORT_ID;
        buf[1] = CMD_FIRMWARE;
        dev.write(&buf)?;

        let mut rx = [0u8; PACKET_LEN];
        let n = dev.read_timeout(&mut rx, 250)?;
        debug!("Aura FW RX ({n}B): {:02X?}", &rx[..n.min(20)]);
        if n >= 18 && rx[1] == 0x02 {
            let raw = &rx[2..18];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            Ok(String::from_utf8_lossy(&raw[..end]).trim().to_string())
        } else {
            Ok("Unknown".to_string())
        }
    }

    fn read_config_table(dev: &dyn AuraIo) -> Result<[u8; 60], AuraError> {
        let mut buf = [0u8; PACKET_LEN];
        buf[0] = REPORT_ID;
        buf[1] = CMD_CONFIG_TABLE;
        dev.write(&buf)?;

        let mut rx = [0u8; PACKET_LEN];
        let n = dev.read_timeout(&mut rx, 250)?;
        debug!("Aura Config RX ({n}B): {:02X?}", &rx[..n.min(24)]);
        if n >= 64 && rx[1] == 0x30 {
            let mut table = [0u8; 60];
            table.copy_from_slice(&rx[4..64]);
            Ok(table)
        } else {
            Err(AuraError::InvalidConfigTable)
        }
    }

    /// Enable Gen1 addressing compatibility on ARGB headers (matches OpenRGB `SetGen1()`).
    pub fn set_gen1(&mut self) -> Result<(), AuraError> {
        let mut buf = [0u8; PACKET_LEN];
        buf[0] = REPORT_ID;
        buf[1] = 0x52;
        buf[2] = 0x53;
        buf[3] = 0x00;
        buf[4] = 0x01;
        self.dev.write(&buf)?;
        Ok(())
    }

    pub fn set_channel_led_count(&mut self, channel_idx: usize, count: usize) {
        if let Some(ch) = self.info.channels.get_mut(channel_idx)
            && ch.kind == AuraChannelKind::AddressableGen2
        {
            ch.led_count = count.clamp(1, 120);
        }
    }

    fn send_effect_cmd(
        &self,
        effect_channel: u8,
        mode: AuraHwMode,
        shutdown_effect: bool,
    ) -> Result<(), AuraError> {
        let mut buf = [0u8; PACKET_LEN];
        buf[0] = REPORT_ID;
        buf[1] = CMD_EFFECT;
        buf[2] = effect_channel;
        buf[3] = 0x00;
        buf[4] = if shutdown_effect { 0x01 } else { 0x00 };
        buf[5] = mode as u8;
        self.dev.write(&buf)?;
        Ok(())
    }

    fn send_effect_color(
        &self,
        start_led: usize,
        led_count: usize,
        color: Rgb,
        shutdown_effect: bool,
    ) -> Result<(), AuraError> {
        if led_count == 0 {
            return Ok(());
        }
        if start_led.saturating_add(led_count) > 16 {
            return Err(AuraError::LedRangeOutOfBounds {
                start: start_led,
                count: led_count,
            });
        }
        let mask: u16 = (((1u32 << led_count) - 1) << start_led) as u16;
        let mut buf = [0u8; PACKET_LEN];
        buf[0] = REPORT_ID;
        buf[1] = CMD_EFFECT_COLOR;
        buf[2] = (mask >> 8) as u8;
        buf[3] = (mask & 0xFF) as u8;
        buf[4] = if shutdown_effect { 0x01 } else { 0x00 };

        for i in 0..led_count {
            let off = 5 + 3 * (start_led + i);
            if off + 2 < PACKET_LEN {
                buf[off] = color.r;
                buf[off + 1] = color.g;
                buf[off + 2] = color.b;
            }
        }
        self.dev.write(&buf)?;
        Ok(())
    }

    /// Apply a built-in Aura hardware effect (e.g. Static, Breathing, Rainbow, Off) to a channel.
    pub fn set_channel_hw_mode(
        &mut self,
        channel_idx: usize,
        mode: AuraHwMode,
        color: Rgb,
        shutdown_effect: bool,
    ) -> Result<(), AuraError> {
        let ch = self
            .info
            .channels
            .get(channel_idx)
            .ok_or(AuraError::InvalidChannel(channel_idx))?
            .clone();

        self.send_effect_cmd(ch.effect_channel, mode, shutdown_effect)?;

        if mode == AuraHwMode::Direct {
            if let Some(flag) = self.channel_direct.get_mut(channel_idx) {
                *flag = true;
            }
            return Ok(());
        }
        if let Some(flag) = self.channel_direct.get_mut(channel_idx) {
            *flag = false;
        }

        let mut start_led = 0usize;
        for c in self.info.channels.iter().take(channel_idx) {
            start_led += match c.kind {
                AuraChannelKind::MainboardFixed => c.default_led_count,
                AuraChannelKind::AddressableGen2 => 1,
            };
        }
        let hw_led_count = match ch.kind {
            AuraChannelKind::MainboardFixed => ch.default_led_count,
            AuraChannelKind::AddressableGen2 => 1,
        };
        self.send_effect_color(start_led, hw_led_count, color, shutdown_effect)?;
        Ok(())
    }

    /// Apply a hardware mode across ALL channels (mainboard + all ARGB headers).
    pub fn set_all_hw_mode(
        &mut self,
        mode: AuraHwMode,
        color: Rgb,
        shutdown_effect: bool,
    ) -> Result<(), AuraError> {
        let count = self.info.channels.len();
        for idx in 0..count {
            self.set_channel_hw_mode(idx, mode, color, shutdown_effect)?;
        }
        Ok(())
    }

    /// Persist current hardware effect & shutdown effect into the motherboard's Aura flash memory.
    pub fn commit(&mut self) -> Result<(), AuraError> {
        let mut buf = [0u8; PACKET_LEN];
        buf[0] = REPORT_ID;
        buf[1] = CMD_COMMIT;
        buf[2] = 0x55;
        self.dev.write(&buf)?;
        info!("Configuration ASUS AURA sauvegardée en mémoire flash (Commit 0x3F 0x55)");
        Ok(())
    }

    /// True while at least one controller channel is in software Direct mode.
    pub fn is_in_direct_mode(&self) -> bool {
        self.channel_direct.iter().any(|&d| d)
    }

    pub fn enter_direct_mode(&mut self) -> Result<(), AuraError> {
        for i in 0..self.info.channels.len() {
            if self.channel_direct.get(i).copied().unwrap_or(false) {
                continue;
            }
            let eff_ch = self.info.channels[i].effect_channel;
            self.send_effect_cmd(eff_ch, AuraHwMode::Direct, false)?;
            if let Some(flag) = self.channel_direct.get_mut(i) {
                *flag = true;
            }
        }
        Ok(())
    }

    /// Stream direct per-LED RGB colors to a specific channel (Mainboard or ARGB header).
    pub fn send_channel_direct(
        &mut self,
        channel_idx: usize,
        colors: &[Rgb],
    ) -> Result<(), AuraError> {
        let ch = self
            .info
            .channels
            .get(channel_idx)
            .ok_or(AuraError::InvalidChannel(channel_idx))?
            .clone();

        if !self
            .channel_direct
            .get(channel_idx)
            .copied()
            .unwrap_or(false)
        {
            self.send_effect_cmd(ch.effect_channel, AuraHwMode::Direct, false)?;
            if let Some(flag) = self.channel_direct.get_mut(channel_idx) {
                *flag = true;
            }
        }

        for pkt in build_direct_packets(ch.direct_channel, colors) {
            self.dev.write(&pkt)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn from_test_io(dev: Box<dyn AuraIo>, info: AuraMainboardInfo) -> Self {
        let count = info.channels.len();
        Self {
            dev,
            info,
            channel_direct: vec![false; count],
        }
    }
}

/// Driver discovering ASUS Aura USB mainboard lighting controllers.
pub struct AsusAuraDriver;

impl Driver for AsusAuraDriver {
    fn name(&self) -> &'static str {
        "asus_aura"
    }

    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[ASUS_VID]
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        AuraMainboardController::connect()
            .ok()
            .map(|d| Box::new(d) as Box<dyn Device>)
            .into_iter()
            .collect()
    }
}

impl Device for AuraMainboardController {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: format!("asus_aura:{:04x}:{:04x}", self.info.vid, self.info.pid),
            vendor: "ASUS".into(),
            model: self.info.product_name.clone(),
            kind: DeviceKind::Motherboard,
            connection: Connection::Usb,
            firmware: Some(self.info.firmware_version.clone()),
        }
    }

    fn status(&self) -> DeviceStatus {
        DeviceStatus::default()
    }

    fn settings(&self) -> Vec<Setting> {
        Vec::new()
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        Some(self)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn apply_setting(&mut self, id: &str, _value: &Value) -> anyhow::Result<()> {
        anyhow::bail!("Réglage ASUS Aura inconnu : {id}")
    }
}

fn aura_channel_idx_from_zone(zone_id: &str) -> Option<usize> {
    zone_id.strip_prefix("ch_")?.parse().ok()
}

impl Lighting for AuraMainboardController {
    fn zones(&self) -> Vec<Zone> {
        self.info
            .channels
            .iter()
            .map(|ch| {
                let mut zone =
                    Zone::linear(&format!("ch_{}", ch.index), &ch.name, ch.led_count, 0.5);
                if ch.kind == AuraChannelKind::MainboardFixed && ch.led_count <= 1 {
                    zone.kind = ZoneKind::Single;
                }
                zone.resizable = (ch.kind == AuraChannelKind::AddressableGen2).then_some((1, 120));
                zone
            })
            .collect()
    }

    fn resize_zone(&mut self, zone_id: &str, led_count: u16) -> anyhow::Result<()> {
        if let Some(idx) = aura_channel_idx_from_zone(zone_id) {
            self.set_channel_led_count(idx, led_count as usize);
        }
        Ok(())
    }

    fn max_fps(&self) -> u8 {
        30
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        let zone_ids: Vec<String> = self
            .info
            .channels
            .iter()
            .map(|ch| format!("ch_{}", ch.index))
            .collect();
        let mut effects: Vec<HardwareEffect> = AuraHwMode::HARDWARE_MODES
            .iter()
            .map(|&mode| HardwareEffect {
                id: format!("{mode:?}"),
                label: mode.label().into(),
                params: EffectParams {
                    colors: if mode.uses_color() { (1, 1) } else { (0, 0) },
                    speed: None,
                    brightness: None,
                    direction: false,
                    savable: true,
                },
                zones: zone_ids.clone(),
            })
            .collect();

        for mode in [
            AuraHwMode::Off,
            AuraHwMode::Static,
            AuraHwMode::Breathing,
            AuraHwMode::Rainbow,
        ] {
            effects.push(HardwareEffect {
                id: format!("shutdown:{mode:?}"),
                label: format!("À l'extinction : {}", mode.label()),
                params: EffectParams {
                    colors: if mode == AuraHwMode::Static {
                        (1, 1)
                    } else {
                        (0, 0)
                    },
                    speed: None,
                    brightness: None,
                    direction: false,
                    savable: true,
                },
                zones: Vec::new(),
            });
        }
        effects
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> anyhow::Result<()> {
        let mut offset = 0usize;
        for idx in 0..self.info.channels.len() {
            let count = self.info.channels[idx].led_count;
            let end = (offset + count).min(colors.len());
            let slice = &colors[offset..end];
            offset = end;
            if !slice.is_empty() {
                self.send_channel_direct(idx, slice)
                    .context("Envoi trame Direct ASUS Aura")?;
            }
        }
        Ok(())
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> anyhow::Result<()> {
        let color = req.colors.first().copied().unwrap_or(Rgb::CYAN);
        if let Some(shutdown_mode) = req.effect.strip_prefix("shutdown:") {
            anyhow::ensure!(
                req.save,
                "Le mode d'extinction ASUS Aura écrit en flash et nécessite une confirmation explicite"
            );
            let mode = AuraHwMode::from_str_id(shutdown_mode);
            self.set_all_hw_mode(mode, color, true)
                .context("Application mode d'extinction ASUS Aura")?;
            self.commit().context("Sauvegarde flash ASUS Aura")?;
            return Ok(());
        }

        let mode = AuraHwMode::from_str_id(&req.effect);
        match req.zone.as_deref().and_then(aura_channel_idx_from_zone) {
            Some(ch_idx) => self
                .set_channel_hw_mode(ch_idx, mode, color, false)
                .context("Application mode matériel ASUS Aura")?,
            None => self
                .set_all_hw_mode(mode, color, false)
                .context("Application mode matériel ASUS Aura")?,
        }
        if req.save {
            self.commit().context("Sauvegarde flash ASUS Aura")?;
        }
        Ok(())
    }

    fn release(&mut self) -> anyhow::Result<()> {
        for idx in 0..self.info.channels.len() {
            if self.channel_direct.get(idx).copied().unwrap_or(false) {
                self.set_channel_hw_mode(idx, AuraHwMode::Rainbow, Rgb::CYAN, false)
                    .context("Restauration mode matériel ASUS Aura")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opengear_core::lighting::{Placement, render_frame};
    use opengear_core::{EffectKind, RgbEffectConfig};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MockAuraIo {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl AuraIo for MockAuraIo {
        fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(data.len())
        }

        fn read_timeout(
            &self,
            _buf: &mut [u8],
            _timeout_ms: i32,
        ) -> Result<usize, hidapi::HidError> {
            Ok(0)
        }
    }

    fn test_info() -> AuraMainboardInfo {
        AuraMainboardInfo {
            vid: ASUS_VID,
            pid: 0x19AF,
            product_name: "ASUS ROG STRIX".into(),
            firmware_version: "AURA-V1".into(),
            config_table_hex: String::new(),
            mainboard_fixed_leds: 4,
            rgb_12v_headers: 1,
            argb_5v_headers: 1,
            channels: vec![
                AuraChannel {
                    index: 0,
                    name: "Carte mère".into(),
                    kind: AuraChannelKind::MainboardFixed,
                    effect_channel: 0,
                    direct_channel: 0x04,
                    default_led_count: 4,
                    led_count: 4,
                    rgb_headers_12v: 1,
                },
                AuraChannel {
                    index: 1,
                    name: "Header ARGB #1".into(),
                    kind: AuraChannelKind::AddressableGen2,
                    effect_channel: 1,
                    direct_channel: 0,
                    default_led_count: 20,
                    led_count: 24,
                    rgb_headers_12v: 0,
                },
            ],
        }
    }

    #[test]
    fn direct_packets_split_cleanly_at_256_boundary() {
        let colors = vec![Rgb::new(1, 2, 3); 265];
        let pkts = build_direct_packets(0x01, &colors);
        // 256 LEDs -> 12*20 + 16 = 13 packets; remaining 9 LEDs -> 1 packet
        assert_eq!(pkts.len(), 14);
        // Packet #12 (index 240..256): 16 LEDs, NO overflow, NO apply
        assert_eq!(pkts[12][2], 0x01);
        assert_eq!(pkts[12][3], 240);
        assert_eq!(pkts[12][4], 16);
        // Packet #13 (index 256..265): 9 LEDs, overflow + apply, offset 0
        assert_eq!(pkts[13][2], 0x01 | DIRECT_OVERFLOW_FLAG | DIRECT_APPLY_FLAG);
        assert_eq!(pkts[13][3], 0);
        assert_eq!(pkts[13][4], 9);
    }

    #[test]
    fn lighting_set_frame_and_hw_effects_match_legacy_hid_bytes() {
        let io_old = MockAuraIo::default();
        let io_new = MockAuraIo::default();
        let mut old_ctrl =
            AuraMainboardController::from_test_io(Box::new(io_old.clone()), test_info());
        let mut new_ctrl =
            AuraMainboardController::from_test_io(Box::new(io_new.clone()), test_info());

        let eff = RgbEffectConfig {
            kind: EffectKind::RainbowWave,
            brightness: 0.85,
            speed: 1.5,
            ..Default::default()
        };
        let t = 0.42f32;
        for (ch_idx, ch) in old_ctrl.info.channels.clone().iter().enumerate() {
            let leds: Vec<Rgb> = (0..ch.led_count)
                .map(|i| {
                    let nx = if ch.led_count <= 1 {
                        0.5
                    } else {
                        i as f32 / (ch.led_count - 1) as f32
                    };
                    eff.sample(i, nx, 0.5, t)
                })
                .collect();
            old_ctrl.send_channel_direct(ch_idx, &leds).unwrap();
        }
        old_ctrl
            .set_all_hw_mode(AuraHwMode::Breathing, Rgb::new(10, 20, 30), false)
            .unwrap();
        old_ctrl.commit().unwrap();
        old_ctrl
            .set_all_hw_mode(AuraHwMode::Static, Rgb::new(1, 2, 3), true)
            .unwrap();
        old_ctrl.commit().unwrap();

        let zones = new_ctrl.zones();
        assert_eq!(zones[0].resizable, None);
        assert_eq!(zones[1].resizable, Some((1, 120)));
        let frame = render_frame(&zones, &Placement::default(), &eff, t);
        new_ctrl.set_frame(&frame).unwrap();
        new_ctrl
            .apply_hardware_effect(&EffectRequest {
                effect: "Breathing".into(),
                zone: None,
                colors: vec![Rgb::new(10, 20, 30)],
                speed: None,
                brightness: None,
                reverse: false,
                save: true,
            })
            .unwrap();
        // Shutdown mode refuses without save:true.
        assert!(
            new_ctrl
                .apply_hardware_effect(&EffectRequest {
                    effect: "shutdown:Static".into(),
                    zone: None,
                    colors: vec![Rgb::new(1, 2, 3)],
                    speed: None,
                    brightness: None,
                    reverse: false,
                    save: false,
                })
                .is_err()
        );
        new_ctrl
            .apply_hardware_effect(&EffectRequest {
                effect: "shutdown:Static".into(),
                zone: None,
                colors: vec![Rgb::new(1, 2, 3)],
                speed: None,
                brightness: None,
                reverse: false,
                save: true,
            })
            .unwrap();

        assert_eq!(
            *io_old.writes.lock().unwrap(),
            *io_new.writes.lock().unwrap()
        );
    }
}
