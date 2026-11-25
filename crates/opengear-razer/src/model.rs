//! Razer devices (generic 90-byte protocol) in the shared device and lighting models.
//!
//! Everything here comes from `docs/protocols/razer.md`. Supported: the generic protocol with the
//! `extended`, `standard` and `linear` dialects (177 devices). Not supported yet: Kraken headsets,
//! the ARGB controller, the Hanbo AIO, the five `custom` models and devices relayed by a wireless
//! receiver (they need protocol paths the driver does not implement).
//!
//! Experimental: no Razer hardware was available to test it. Nothing is ever written to device
//! memory (storage byte always `0x00`).

use crate::proto::{self, RazerIo, RzReport, WaveDirection};
use crate::tables::{DEVICES, KEY_LAYOUTS};
use anyhow::{Context, Result, bail};
use opengear_core::Rgb;
use opengear_core::driver::{
    Connection, Device, DeviceInfo, DeviceKind as CoreKind, DeviceStatus, Driver, Section, Setting,
    Value, setting_ids,
};
use opengear_core::lighting::{
    EffectParams, EffectRequest, HardwareEffect, Led, Lighting, Zone, ZoneKind,
};
use std::time::Duration;

/// Razer product family, as listed in the protocol sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Laptop,
    Mouse,
    Mousemat,
    Keypad,
    Headset,
    HeadsetStand,
    Accessory,
    Speaker,
    Microphone,
    LedStrip,
    Light,
    Gpu,
}

/// Lighting command dialect (§2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    /// Class `0x0F` (most devices since ~2018).
    Extended,
    /// Class `0x03`, frame rows with `0x0B`.
    Standard,
    /// Class `0x03`, single row with `0x0C`.
    Linear,
}

/// HID collection a device is reached through (`None` = any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidSelector {
    pub interface: Option<i32>,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneDef {
    pub name: &'static str,
    pub kind: ZoneKind,
    pub rows: u8,
    pub cols: u8,
}

/// One Razer model (§9).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceDef {
    pub name: &'static str,
    pub pid: u16,
    pub kind: DeviceKind,
    pub matrix: Matrix,
    pub tx_id: u8,
    pub led_id: u8,
    /// Frame grid sent to the device (may be larger than the sum of the zones).
    pub rows: u8,
    pub cols: u8,
    pub zones: &'static [ZoneDef],
    pub hid: &'static [HidSelector],
}

/// Key positions of a keyboard grid (§10), ANSI unless `laptop_iso`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyLayout {
    pub pids: &'static [u16],
    pub rows: u8,
    pub cols: u8,
    pub laptop_iso: bool,
    /// `(row, col, name, is_w3c_code)`.
    pub keys: &'static [(u8, u8, &'static str, bool)],
}

/// Name of the experimental driver (key of `AppConfig::experimental_drivers`).
pub const DRIVER_NAME: &str = "razer";
/// PIDs that must be switched to software mode before taking control (§3a).
const SOFTWARE_MODE_PIDS: [u16; 3] = [0x0F26, 0x0F07, 0x0068];
/// Models whose brightness scale stops at 100 instead of 255 (§5).
const BRIGHTNESS_100_PREFIX: &str = "Razer DeathAdder Essential V2";
/// Models that only get breathing when their firmware lists it (§4).
const NO_DEFAULT_BREATHING_PREFIXES: [&str; 2] = ["Razer Basilisk V3", "Razer Basilisk V4 Pro"];
/// Pause between two frame reports (§11: do not go below ≈1 ms).
const ROW_PAUSE: Duration = Duration::from_millis(1);

// Effect identifiers of the extended dialect (§2.6).
const FX_WAVE: u8 = 0x04;
const FX_REACTIVE: u8 = 0x05;
const FX_RIPPLE: u8 = 0x06;
const FX_STARLIGHT: u8 = 0x07;
const FX_FIRE: u8 = 0x09;
const FX_BREATHING: u8 = 0x02;

pub fn find_device(pid: u16) -> Option<&'static DeviceDef> {
    DEVICES.iter().find(|d| d.pid == pid)
}

fn core_kind(kind: DeviceKind) -> CoreKind {
    match kind {
        DeviceKind::Keyboard | DeviceKind::Keypad => CoreKind::Keyboard,
        DeviceKind::Laptop => CoreKind::Laptop,
        DeviceKind::Mouse => CoreKind::Mouse,
        DeviceKind::Mousemat => CoreKind::Mousepad,
        DeviceKind::Headset => CoreKind::Headset,
        DeviceKind::LedStrip | DeviceKind::Light => CoreKind::LedController,
        DeviceKind::HeadsetStand
        | DeviceKind::Accessory
        | DeviceKind::Speaker
        | DeviceKind::Microphone
        | DeviceKind::Gpu => CoreKind::Other,
    }
}

/// HID report ID of the 90-byte reports (§2.1).
fn report_id(def: &DeviceDef) -> u8 {
    if def.name.starts_with("Razer Leviathan V2") {
        0x07
    } else {
        0x00
    }
}

fn max_brightness(def: &DeviceDef) -> u8 {
    if def.name.starts_with(BRIGHTNESS_100_PREFIX) {
        100
    } else {
        255
    }
}

// ---------------------------------------------------------------------------------------------
// Geometry: zones, LED positions and their cell in the frame grid
// ---------------------------------------------------------------------------------------------

/// Apply the desktop ISO rule of §10 to an ANSI key list.
fn iso_desktop_keys(keys: &[(u8, u8, &'static str, bool)]) -> Vec<(u8, u8, &'static str, bool)> {
    let row_of = |code: &str| keys.iter().find(|k| k.3 && k.2 == code).map(|k| k.0);
    let col_of = |code: &str| keys.iter().find(|k| k.3 && k.2 == code).map(|k| k.1);
    let (Some(tab_row), Some(caps_row), Some(enter_col), Some(shift_row), Some(z_col)) = (
        row_of("Tab"),
        row_of("CapsLock"),
        keys.iter()
            .find(|k| k.3 && k.2 == "Enter" && Some(k.0) == row_of("CapsLock"))
            .map(|k| k.1),
        row_of("ShiftLeft"),
        col_of("KeyZ"),
    ) else {
        return keys.to_vec();
    };
    let mut out: Vec<_> = keys
        .iter()
        .copied()
        .filter(|k| !(k.3 && k.2 == "Backslash" && k.0 == tab_row))
        .collect();
    if enter_col > 0 {
        out.push((caps_row, enter_col - 1, "Backslash", true));
    }
    if z_col > 0 {
        out.push((shift_row, z_col - 1, "IntlBackslash", true));
    }
    out
}

/// Key list for this model, ANSI or ISO.
fn keys_for(def: &DeviceDef, iso: bool) -> Option<Vec<(u8, u8, &'static str, bool)>> {
    let ansi = KEY_LAYOUTS
        .iter()
        .find(|l| !l.laptop_iso && l.pids.contains(&def.pid))?;
    if !iso {
        return Some(ansi.keys.to_vec());
    }
    if def.kind == DeviceKind::Laptop {
        return KEY_LAYOUTS
            .iter()
            .find(|l| l.laptop_iso && l.pids.contains(&def.pid))
            .map(|l| l.keys.to_vec())
            .or_else(|| Some(ansi.keys.to_vec()));
    }
    Some(iso_desktop_keys(ansi.keys))
}

/// Zones in frame order, and for each LED (in the same order) its index in the frame grid.
///
/// Zones fill the grid one after the other in row-major order (§3b, §5).
pub fn build_zones(def: &DeviceDef, iso: bool) -> (Vec<Zone>, Vec<usize>) {
    let rows = f32::from(def.rows.max(1));
    let cols = f32::from(def.cols.max(1));
    let cell_pos = |idx: usize| {
        let (r, c) = (idx / def.cols as usize, idx % def.cols as usize);
        ((c as f32 + 0.5) / cols, (r as f32 + 0.5) / rows)
    };
    let keys = keys_for(def, iso);
    let mut zones = Vec::new();
    let mut cells = Vec::new();
    let mut next = 0usize;
    for (zi, z) in def.zones.iter().enumerate() {
        let count = z.rows as usize * z.cols as usize;
        let mut leds = Vec::new();
        let is_keyboard = zi == 0 && z.kind == ZoneKind::Matrix && keys.is_some();
        if is_keyboard {
            // Only cells that hold a key are LEDs; the others are sent black.
            let mut ks = keys.clone().unwrap_or_default();
            ks.sort_by_key(|k| (k.0, k.1));
            for (r, c, name, is_code) in ks {
                let idx = next + r as usize * def.cols as usize + c as usize;
                if idx >= next + count {
                    continue;
                }
                let (x, y) = cell_pos(idx);
                let led = Led::at(name, x, y);
                leds.push(if is_code { led.key(name) } else { led });
                cells.push(idx);
            }
        } else {
            for i in 0..count {
                let idx = next + i;
                let (x, y) = cell_pos(idx);
                leds.push(Led::at(format!("{} {}", z.name, i + 1), x, y));
                cells.push(idx);
            }
        }
        zones.push(Zone {
            id: format!("z{zi}"),
            name: z.name.to_string(),
            kind: z.kind,
            leds,
            resizable: None,
        });
        next += count;
    }
    (zones, cells)
}

/// Full frame grid (`rows × cols`, row-major) for `colors` given LED by LED.
pub fn grid_frame(def: &DeviceDef, cells: &[usize], colors: &[Rgb]) -> Vec<Rgb> {
    let mut grid = vec![Rgb::BLACK; def.rows as usize * def.cols as usize];
    for (cell, color) in cells.iter().zip(colors) {
        if let Some(slot) = grid.get_mut(*cell) {
            *slot = *color;
        }
    }
    grid
}

/// Reports that show `grid` (§3b): one per row, then "activate the frame".
pub fn frame_reports(def: &DeviceDef, grid: &[Rgb]) -> Vec<RzReport> {
    let cols = def.cols as usize;
    let mut out: Vec<RzReport> = grid
        .chunks(cols)
        .enumerate()
        .map(|(row, colors)| match def.matrix {
            Matrix::Extended => proto::cmd_ext_frame_row(def.tx_id, row as u8, 0, colors),
            Matrix::Standard => proto::cmd_std_frame_row(def.tx_id, row as u8, 0, colors),
            Matrix::Linear => proto::cmd_linear_frame_row(def.tx_id, 0, colors),
        })
        .collect();
    out.push(match def.matrix {
        Matrix::Extended => proto::cmd_ext_enable_custom_frame(def.tx_id),
        Matrix::Standard | Matrix::Linear => proto::cmd_std_matrix_enable_frame(def.tx_id),
    });
    out
}

// ---------------------------------------------------------------------------------------------
// Hardware effects
// ---------------------------------------------------------------------------------------------

fn effect(
    id: &str,
    label: &str,
    colors: (u8, u8),
    speed: Option<(u8, u8)>,
    direction: bool,
) -> HardwareEffect {
    HardwareEffect {
        id: id.into(),
        label: label.into(),
        params: EffectParams {
            colors,
            speed,
            brightness: None,
            direction,
            savable: false,
        },
        zones: Vec::new(),
    }
}

/// Effects offered for this model. `listed` = effect ids reported by the firmware (extended
/// dialect only); it can only add effects (§4).
pub fn effects_for(def: &DeviceDef, listed: &[u8]) -> Vec<HardwareEffect> {
    let has = |fx: u8| listed.contains(&fx);
    let breathing = !NO_DEFAULT_BREATHING_PREFIXES
        .iter()
        .any(|p| def.name.starts_with(p))
        || has(FX_BREATHING);
    let mut out = vec![
        effect("off", "Éteint", (0, 0), None, false),
        effect("static", "Statique", (1, 1), None, false),
    ];
    if breathing {
        out.push(effect(
            "breathing",
            "Respiration (0 couleur = aléatoire)",
            (0, 2),
            None,
            false,
        ));
    }
    out.push(effect("spectrum", "Cycle spectral", (0, 0), None, false));
    if def.matrix == Matrix::Extended {
        let has_input = matches!(
            def.kind,
            DeviceKind::Keyboard | DeviceKind::Laptop | DeviceKind::Mouse | DeviceKind::Keypad
        );
        if has(FX_WAVE) {
            out.push(effect("wave", "Vague", (0, 0), None, true));
        }
        if has(FX_REACTIVE) && has_input {
            out.push(effect("reactive", "Réactif", (1, 1), Some((1, 3)), false));
        }
        if has(FX_STARLIGHT) {
            out.push(effect(
                "starlight",
                "Étoiles (0 couleur = aléatoire)",
                (0, 2),
                Some((1, 3)),
                false,
            ));
        }
        if has(FX_RIPPLE) && has_input {
            out.push(effect("ripple", "Ondulation", (1, 1), None, false));
        }
        if has(FX_FIRE) {
            out.push(effect("fire", "Feu", (0, 0), None, false));
        }
    }
    out
}

/// Reports for a validated effect request, followed by the brightness report (§3c).
pub fn effect_reports(
    def: &DeviceDef,
    req: &EffectRequest,
    brightness: u8,
) -> Result<Vec<RzReport>> {
    let (tx, led) = (def.tx_id, def.led_id);
    let c = |i: usize| req.colors.get(i).copied().unwrap_or(Rgb::BLACK);
    let duration = proto::duration_from_speed(req.speed);
    let ext = def.matrix == Matrix::Extended;
    let fx = match (req.effect.as_str(), req.colors.len(), ext) {
        ("off", _, true) => proto::cmd_ext_off(tx, led),
        ("off", _, false) => proto::cmd_std_matrix_off(tx),
        ("static", _, true) => proto::cmd_ext_static(tx, led, c(0)),
        ("static", _, false) => proto::cmd_std_matrix_static(tx, c(0)),
        ("breathing", 0, true) => proto::cmd_ext_breath_random(tx, led),
        ("breathing", 1, true) => proto::cmd_ext_breath_1(tx, led, c(0)),
        ("breathing", _, true) => proto::cmd_ext_breath_2(tx, led, c(0), c(1)),
        ("breathing", 0, false) => proto::cmd_std_matrix_breath_random(tx),
        ("breathing", 1, false) => proto::cmd_std_matrix_breath_1(tx, c(0)),
        ("breathing", _, false) => proto::cmd_std_matrix_breath_2(tx, c(0), c(1)),
        ("spectrum", _, true) => proto::cmd_ext_spectrum(tx, led),
        ("spectrum", _, false) => proto::cmd_std_matrix_spectrum(tx),
        ("wave", _, true) => proto::cmd_ext_wave(tx, led, WaveDirection::from_reverse(req.reverse)),
        ("reactive", _, true) => proto::cmd_ext_reactive(tx, led, duration, c(0)),
        ("starlight", 0, true) => proto::cmd_ext_starlight_random(tx, led, duration),
        ("starlight", 1, true) => proto::cmd_ext_starlight_1(tx, led, duration, c(0)),
        ("starlight", _, true) => proto::cmd_ext_starlight_2(tx, led, duration, c(0), c(1)),
        ("ripple", _, true) => proto::cmd_ext_ripple(tx, led, c(0)),
        ("fire", _, true) => proto::cmd_ext_fire(tx, led),
        (other, _, _) => bail!("Effet « {other} » indisponible sur {}", def.name),
    };
    Ok(vec![fx, brightness_report(def, brightness)])
}

fn brightness_report(def: &DeviceDef, brightness: u8) -> RzReport {
    match def.matrix {
        Matrix::Extended => proto::cmd_ext_brightness(def.tx_id, def.led_id, brightness),
        Matrix::Standard | Matrix::Linear => {
            proto::cmd_std_brightness(def.tx_id, def.led_id, brightness)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Device
// ---------------------------------------------------------------------------------------------

pub struct RazerDevice {
    def: &'static DeviceDef,
    io: Box<dyn RazerIo>,
    report_id: u8,
    serial: String,
    firmware: Option<String>,
    iso: bool,
    zones: Vec<Zone>,
    cells: Vec<usize>,
    effects: Vec<HardwareEffect>,
    /// Device scale (0..=255, or 0..=100 on a few models).
    brightness: u8,
}

impl RazerDevice {
    /// Open a device on `io`: read its identity, keyboard layout and supported effects. Reads that
    /// fail are tolerated (the device still works with the defaults of its model).
    pub fn open(def: &'static DeviceDef, mut io: Box<dyn RazerIo>) -> Self {
        let report_id = report_id(def);
        let mut read =
            |req: RzReport| proto::transfer_reliable(io.as_mut(), report_id, &req, false).ok();
        let firmware = read(proto::cmd_get_firmware(def.tx_id)).map(|r| proto::parse_firmware(&r));
        let serial = read(proto::cmd_get_serial(def.tx_id))
            .map(|r| proto::parse_serial(&r))
            .unwrap_or_default();
        let iso = matches!(def.kind, DeviceKind::Keyboard | DeviceKind::Laptop)
            && read(proto::cmd_get_keyboard_info(def.tx_id)).is_some_and(|r| {
                proto::parse_keyboard_layout(r.args[0]) != proto::PhysicalLayout::Ansi
            });
        let listed = if def.matrix == Matrix::Extended {
            list_firmware_effects(def, &mut read)
        } else {
            Vec::new()
        };
        if SOFTWARE_MODE_PIDS.contains(&def.pid) {
            let _ = proto::send_command(
                io.as_mut(),
                report_id,
                &proto::cmd_set_device_mode(def.tx_id, 0x03),
                false,
            );
        }
        let (zones, cells) = build_zones(def, iso);
        Self {
            def,
            io,
            report_id,
            serial,
            firmware,
            iso,
            zones,
            cells,
            effects: effects_for(def, &listed),
            brightness: max_brightness(def),
        }
    }

    pub fn definition(&self) -> &'static DeviceDef {
        self.def
    }

    pub fn is_iso(&self) -> bool {
        self.iso
    }

    fn send_all(&mut self, reports: &[RzReport], pause: Duration) -> Result<()> {
        for r in reports {
            self.io.sleep(pause);
            proto::send_command(self.io.as_mut(), self.report_id, r, false)?;
        }
        Ok(())
    }
}

/// Firmware effect list (§4): the model's region if it has one, else the intersection over all
/// regions.
fn list_firmware_effects(
    def: &DeviceDef,
    read: &mut impl FnMut(RzReport) -> Option<RzReport>,
) -> Vec<u8> {
    let regions = if def.led_id != 0x00 {
        vec![def.led_id]
    } else {
        read(proto::cmd_ext_list_regions(def.tx_id))
            .map(|r| proto::parse_ext_regions(&r))
            .unwrap_or_default()
    };
    let mut common: Option<Vec<u8>> = None;
    for region in regions {
        let fx = read(proto::cmd_ext_list_effects(def.tx_id, region))
            .map(|r| proto::parse_ext_effects(&r))
            .unwrap_or_default();
        common = Some(match common {
            None => fx,
            Some(prev) => prev.into_iter().filter(|f| fx.contains(f)).collect(),
        });
    }
    common.unwrap_or_default()
}

impl Device for RazerDevice {
    fn info(&self) -> DeviceInfo {
        let slot = if self.serial.is_empty() {
            "0".to_string()
        } else {
            self.serial.clone()
        };
        DeviceInfo {
            id: format!("razer:1532:{:04X}:{slot}", self.def.pid),
            vendor: "Razer".into(),
            model: format!("{} (expérimental)", self.def.name),
            kind: core_kind(self.def.kind),
            connection: if self.def.tx_id == 0x9F || self.def.name.contains("Wireless") {
                Connection::Wireless
            } else {
                Connection::Usb
            },
            firmware: self.firmware.clone(),
        }
    }

    fn status(&self) -> DeviceStatus {
        DeviceStatus::default()
    }

    fn settings(&self) -> Vec<Setting> {
        let max = f64::from(max_brightness(self.def));
        vec![Setting::slider(
            setting_ids::HW_BRIGHTNESS,
            "Luminosité matérielle",
            Section::Lighting,
            (0.0, 100.0, 1.0),
            "%",
            (f64::from(self.brightness) * 100.0 / max).round(),
        )]
    }

    fn apply_setting(&mut self, id: &str, value: &Value) -> Result<()> {
        match id {
            setting_ids::HW_BRIGHTNESS => {
                let pct = value.as_f64().context("nombre attendu")?;
                let level = (pct / 100.0 * f64::from(max_brightness(self.def))).round() as u8;
                let report = brightness_report(self.def, level);
                self.send_all(&[report], Duration::ZERO)?;
                self.brightness = level;
                Ok(())
            }
            other => bail!("Réglage inconnu : {other}"),
        }
    }

    fn lighting(&mut self) -> Option<&mut dyn Lighting> {
        Some(self)
    }
}

impl Lighting for RazerDevice {
    fn zones(&self) -> Vec<Zone> {
        self.zones.clone()
    }

    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        self.effects.clone()
    }

    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()> {
        let grid = grid_frame(self.def, &self.cells, colors);
        let reports = frame_reports(self.def, &grid);
        self.send_all(&reports, ROW_PAUSE)
    }

    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        let reports = effect_reports(self.def, req, self.brightness)?;
        self.send_all(&reports, ROW_PAUSE)
    }

    fn release(&mut self) -> Result<()> {
        if SOFTWARE_MODE_PIDS.contains(&self.def.pid) {
            let report = proto::cmd_set_device_mode(self.def.tx_id, 0x00);
            self.send_all(&[report], Duration::ZERO)?;
        }
        Ok(())
    }

    fn max_fps(&self) -> u8 {
        // One report per grid row + activation, ≈1 ms apart.
        20
    }
}

// ---------------------------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------------------------

/// Experimental Razer driver (disabled unless the user enables it).
pub struct RazerDriver;

fn selector_matches(sel: &HidSelector, info: &hidapi::DeviceInfo) -> bool {
    sel.interface.is_none_or(|i| i == info.interface_number())
        && sel.usage_page.is_none_or(|p| p == info.usage_page())
        && sel.usage.is_none_or(|u| u == info.usage())
}

impl Driver for RazerDriver {
    fn name(&self) -> &'static str {
        DRIVER_NAME
    }

    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[proto::VID_RAZER]
    }

    fn display_name(&self) -> &'static str {
        "Razer (claviers, souris, tapis, accessoires)"
    }

    fn experimental(&self) -> bool {
        true
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        let Ok(api) = hidapi::HidApi::new() else {
            return Vec::new();
        };
        let candidates: Vec<&hidapi::DeviceInfo> = api
            .device_list()
            .filter(|d| d.vendor_id() == proto::VID_RAZER)
            .collect();
        let mut out: Vec<Box<dyn Device>> = Vec::new();
        let mut opened_paths = Vec::new();
        for def in DEVICES {
            // Selectors are tried in the sheet's order (e.g. firmware < 1.5 then >= 1.5).
            let matching = def.hid.iter().find_map(|sel| {
                let found: Vec<_> = candidates
                    .iter()
                    .filter(|d| d.product_id() == def.pid && selector_matches(sel, d))
                    .collect();
                (!found.is_empty()).then_some(found)
            });
            for info in matching.unwrap_or_default() {
                if opened_paths.contains(&info.path().to_owned()) {
                    continue;
                }
                match info.open_device(&api) {
                    Ok(dev) => {
                        opened_paths.push(info.path().to_owned());
                        let io = Box::new(proto::HidRazerIo::new(dev));
                        out.push(Box::new(RazerDevice::open(def, io)));
                    }
                    Err(e) => tracing::warn!("Razer {} : ouverture impossible ({e})", def.name),
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    /// Records every feature report; answers reads with a success echo of the last request.
    #[derive(Clone, Default)]
    struct FakeIo {
        sent: Arc<Mutex<Vec<Vec<u8>>>>,
        fail_reads: bool,
    }

    impl RazerIo for FakeIo {
        fn send_feature_report(&mut self, data: &[u8]) -> Result<()> {
            self.sent.lock().unwrap().push(data.to_vec());
            Ok(())
        }
        fn get_feature_report(&mut self, buf: &mut [u8]) -> Result<usize> {
            if self.fail_reads {
                bail!("no device");
            }
            let last = self
                .sent
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or_default();
            buf[..last.len()].copy_from_slice(&last);
            buf[1] = proto::status::SUCCESS;
            Ok(last.len())
        }
        fn write(&mut self, data: &[u8]) -> Result<usize> {
            Ok(data.len())
        }
        fn read_timeout(&mut self, _: &mut [u8], _: i32) -> Result<usize> {
            Ok(0)
        }
        fn sleep(&self, _: Duration) {}
    }

    fn def(pid: u16) -> &'static DeviceDef {
        find_device(pid).expect("device in table")
    }

    #[test]
    fn tables_are_consistent() {
        assert_eq!(DEVICES.len(), 177);
        let mut pids = HashSet::new();
        for d in DEVICES {
            assert!(pids.insert(d.pid), "{} en double", d.name);
            assert!(
                d.cols as usize <= 25,
                "{}: {} colonnes ne tiennent pas dans un rapport",
                d.name,
                d.cols
            );
            if d.matrix == Matrix::Linear {
                assert_eq!(
                    d.rows, 1,
                    "{}: le dialecte linéaire n'a qu'une rangée",
                    d.name
                );
            }
            let leds: usize = d
                .zones
                .iter()
                .map(|z| z.rows as usize * z.cols as usize)
                .sum();
            assert!(
                leds <= d.rows as usize * d.cols as usize,
                "{}: zones plus grandes que la grille",
                d.name
            );
            assert!(!d.hid.is_empty(), "{}: aucune interface HID", d.name);
        }
        for l in KEY_LAYOUTS {
            for &(r, c, _, _) in l.keys {
                assert!(r < l.rows && c < l.cols);
            }
        }
    }

    #[test]
    fn keyboard_leds_carry_physical_keys_and_iso_rule_applies() {
        let bw = def(0x0241); // BlackWidow 2019, 6x22
        let (zones, cells) = build_zones(bw, false);
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].leds.len(), cells.len());
        let esc = zones[0]
            .leds
            .iter()
            .position(|l| l.key_code.as_deref() == Some("Escape"))
            .unwrap();
        assert_eq!(cells[esc], 1, "Escape is row 0, column 1");
        let has = |zs: &[Zone], code: &str| {
            zs[0]
                .leds
                .iter()
                .any(|l| l.key_code.as_deref() == Some(code))
        };
        assert!(!has(&zones, "IntlBackslash"));

        let (iso, iso_cells) = build_zones(bw, true);
        assert!(has(&iso, "IntlBackslash"));
        let bs = iso[0]
            .leds
            .iter()
            .position(|l| l.key_code.as_deref() == Some("Backslash"))
            .unwrap();
        assert_eq!(
            iso_cells[bs],
            3 * 22 + 13,
            "ISO # key left of Enter (row 3, column 13)"
        );
        let ib = iso[0]
            .leds
            .iter()
            .position(|l| l.key_code.as_deref() == Some("IntlBackslash"))
            .unwrap();
        assert_eq!(
            iso_cells[ib],
            4 * 22 + 2,
            "ISO key left of Z (row 4, column 2)"
        );
    }

    #[test]
    fn zones_fill_the_grid_in_order_beyond_the_keyboard() {
        let v4 = def(0x0287); // Blackwidow V4: 8x23, keyboard 6x23 + 2 underglows of 9
        let (zones, cells) = build_zones(v4, false);
        assert_eq!(zones.len(), 3);
        let left = &zones[1];
        assert_eq!(left.leds.len(), 9);
        let first_left = cells.len() - 18;
        assert_eq!(
            cells[first_left],
            6 * 23,
            "left underglow starts row 6, column 0"
        );
        assert_eq!(*cells.last().unwrap(), 6 * 23 + 17);
    }

    #[test]
    fn frame_reports_follow_each_dialect() {
        // Extended: rows with 0x0F/0x03 then activation on LED 0.
        let bw = def(0x0241);
        let (_, cells) = build_zones(bw, false);
        let colors = vec![Rgb::new(1, 2, 3); cells.len()];
        let grid = grid_frame(bw, &cells, &colors);
        let reports = frame_reports(bw, &grid);
        assert_eq!(reports.len(), 7);
        let row0 = reports[0].encode(0);
        assert_eq!(
            &row0[1..9],
            &[0x00, 0x1F, 0x00, 0x00, 0x00, 5 + 3 * 22, 0x0F, 0x03]
        );
        assert_eq!(&row0[9..14], &[0, 0, 0, 0, 21]);
        assert_eq!(&row0[14..17], &[0, 0, 0], "column 0 has no key: black");
        assert_eq!(&row0[17..20], &[1, 2, 3], "Escape");
        let checksum = row0[3..=88].iter().fold(0u8, |a, b| a ^ b);
        assert_eq!(row0[89], checksum);
        let act = reports[6].encode(0);
        assert_eq!(&act[6..12], &[0x0C, 0x0F, 0x02, 0x00, 0x00, 0x08]);

        // Standard: 0x03/0x0B rows, activation 0x03/0x0A [0x05, 0x00].
        let std_kb = def(0x0203);
        let reports = frame_reports(std_kb, &vec![Rgb::BLACK; 6 * 22]);
        let r = reports[0].encode(0);
        assert_eq!(&r[6..13], &[4 + 3 * 22, 0x03, 0x0B, 0xFF, 0x00, 0x00, 21]);
        let act = reports.last().unwrap().encode(0);
        assert_eq!(&act[6..11], &[0x02, 0x03, 0x0A, 0x05, 0x00]);

        // Linear: one 0x03/0x0C row with fixed size 0x32.
        let linear = DEVICES.iter().find(|d| d.matrix == Matrix::Linear).unwrap();
        let reports = frame_reports(linear, &vec![Rgb::BLACK; linear.cols as usize]);
        assert_eq!(reports.len(), 2);
        let r = reports[0].encode(0);
        assert_eq!(&r[6..9], &[0x32, 0x03, 0x0C]);
    }

    #[test]
    fn effects_are_gated_and_encoded_per_dialect() {
        let bw = def(0x0241);
        let base: Vec<String> = effects_for(bw, &[]).into_iter().map(|e| e.id).collect();
        assert_eq!(base, ["off", "static", "breathing", "spectrum"]);
        let full: Vec<String> = effects_for(bw, &[0x04, 0x05, 0x06, 0x07, 0x09])
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert!(full.contains(&"wave".to_string()) && full.contains(&"reactive".to_string()));
        let basilisk = DEVICES
            .iter()
            .find(|d| d.name.starts_with("Razer Basilisk V3"))
            .unwrap();
        assert!(
            !effects_for(basilisk, &[])
                .iter()
                .any(|e| e.id == "breathing")
        );
        assert!(
            effects_for(basilisk, &[0x02])
                .iter()
                .any(|e| e.id == "breathing")
        );

        let req = EffectRequest {
            effect: "static".into(),
            zone: None,
            colors: vec![Rgb::new(9, 8, 7)],
            speed: None,
            brightness: None,
            reverse: false,
            save: false,
        };
        let reports = effect_reports(bw, &req, 200).unwrap();
        let fx = reports[0].encode(0);
        assert_eq!(
            &fx[6..18],
            &[
                0x09, 0x0F, 0x02, 0x00, 0x05, 0x01, 0x00, 0x00, 0x01, 9, 8, 7
            ]
        );
        let br = reports[1].encode(0);
        assert_eq!(&br[6..12], &[0x03, 0x0F, 0x04, 0x00, 0x05, 200]);
        assert!(
            effect_reports(
                def(0x0203),
                &EffectRequest {
                    effect: "wave".into(),
                    ..req
                },
                255
            )
            .is_err()
        );
    }

    #[test]
    fn nothing_is_ever_stored_in_device_memory() {
        for d in DEVICES {
            for e in effects_for(d, &[0x02, 0x04, 0x05, 0x06, 0x07, 0x09]) {
                assert!(
                    !e.params.savable,
                    "{}: {} ne doit pas être enregistrable",
                    d.name, e.id
                );
            }
        }
    }

    #[test]
    fn opening_tolerates_silent_devices_and_frames_reach_the_wire() {
        let io = FakeIo {
            fail_reads: true,
            ..Default::default()
        };
        let sent = io.sent.clone();
        let mut dev = RazerDevice::open(def(0x0241), Box::new(io));
        assert!(dev.info().model.contains("expérimental"));
        assert_eq!(dev.firmware, None);
        sent.lock().unwrap().clear();
        let n = opengear_core::lighting::frame_len(&dev.zones());
        dev.set_frame(&vec![Rgb::new(255, 0, 0); n]).unwrap();
        let wire = sent.lock().unwrap();
        assert_eq!(wire.len(), 7);
        assert!(
            wire.iter()
                .all(|w| w.len() == proto::REPORT90_WIRE_LEN && w[0] == 0x00)
        );
    }
}
