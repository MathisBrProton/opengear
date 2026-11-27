use crate::proto::{
    KRAKEN_CMD_FINISH_INIT, KRAKEN_CMD_GET_FW, KRAKEN_CMD_GET_STATUS, KRAKEN_CMD_SET_INTERVAL,
    KRAKEN_REPORT_FW, KRAKEN_REPORT_STATUS, KrakenSeries, NZXT_FAN_MIN_PCT, NZXT_PUMP_MIN_PCT,
    NZXT_REPORT_LEN, NZXT_VID, NzxtModel, SMART2_CMD_DETECT_FANS, SMART2_CMD_SET_INTERVAL,
    SMART2_REPORT_FAN_CONFIG, SMART2_REPORT_STATUS, SMART2_STATUS_SPEED, default_kraken_curve,
    encode_kraken_curve, encode_smart2_set_fans, fixed_kraken_curve, identify_nzxt_pid,
    pad_nzxt_report,
};
use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};
use opengear_core::driver::{Connection, Device, DeviceInfo, DeviceKind, DeviceSpec, Driver};
use opengear_core::fans::{Cooling, FanInfo, FanTarget, clamp_target};
use std::ffi::CString;
use tracing::info;

pub trait NzxtHidIo: Send {
    fn write(&self, data: &[u8]) -> std::result::Result<usize, hidapi::HidError>;
    fn read_timeout(
        &self,
        buf: &mut [u8],
        timeout_ms: i32,
    ) -> std::result::Result<usize, hidapi::HidError>;
}

struct RealNzxtHid(HidDevice);

impl NzxtHidIo for RealNzxtHid {
    fn write(&self, data: &[u8]) -> std::result::Result<usize, hidapi::HidError> {
        self.0.write(data)
    }

    fn read_timeout(
        &self,
        buf: &mut [u8],
        timeout_ms: i32,
    ) -> std::result::Result<usize, hidapi::HidError> {
        self.0.read_timeout(buf, timeout_ms)
    }
}

/// Clean-room driver for NZXT USB HID AIO coolers and fan controllers.
/// Marked experimental until validated on physical NZXT hardware.
pub struct NzxtDriver;

impl Driver for NzxtDriver {
    fn name(&self) -> &'static str {
        "nzxt"
    }

    fn display_name(&self) -> &'static str {
        "NZXT Kraken / Smart Device (USB HID)"
    }

    fn experimental(&self) -> bool {
        true
    }

    fn usb_vendor_ids(&self) -> &'static [u16] {
        &[NZXT_VID]
    }

    fn discover(&self) -> Vec<Box<dyn Device>> {
        let Ok(api) = HidApi::new() else {
            return Vec::new();
        };
        let mut out: Vec<Box<dyn Device>> = Vec::new();
        let mut seen = Vec::new();
        for info in api.device_list() {
            if info.vendor_id() != NZXT_VID {
                continue;
            }
            let pid = info.product_id();
            let Some((default_name, model_kind)) = identify_nzxt_pid(pid) else {
                continue;
            };
            let path = info.path().to_string_lossy().to_string();
            if seen.contains(&path) {
                continue;
            }
            let Ok(cpath) = CString::new(info.path().to_bytes()) else {
                continue;
            };
            let Ok(dev) = api.open_path(cpath.as_c_str()) else {
                continue;
            };
            seen.push(path);
            let name = info
                .product_string()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(default_name)
                .to_string();
            let serial = info.serial_number().unwrap_or("0").trim().to_string();
            let mut unit = NzxtCoolingDevice::from_io(
                format!("nzxt_{pid:04X}_{serial}"),
                name,
                serial,
                model_kind,
                Box::new(RealNzxtHid(dev)),
            );
            unit.init_and_refresh();
            info!("Périphérique NZXT prêt : {}", unit.model);
            out.push(Box::new(unit));
        }
        out
    }
}

#[derive(Debug, Clone)]
struct NzxtChannel {
    index: usize,
    id: String,
    label: String,
    min_pct: u8,
    rpm: Option<u32>,
    duty_pct: u8,
    manual_pct: Option<u8>,
}

pub struct NzxtCoolingDevice {
    id: String,
    model: String,
    serial: String,
    kind: NzxtModel,
    firmware: Option<String>,
    coolant_temp_c: Option<f32>,
    channels: Vec<NzxtChannel>,
    io: Box<dyn NzxtHidIo>,
}

impl NzxtCoolingDevice {
    pub(crate) fn from_io(
        id: String,
        model: String,
        serial: String,
        kind: NzxtModel,
        io: Box<dyn NzxtHidIo>,
    ) -> Self {
        let channels = match kind {
            NzxtModel::Kraken(KrakenSeries::X53) => vec![NzxtChannel {
                index: 0,
                id: "pump".into(),
                label: "Pompe AIO".into(),
                min_pct: NZXT_PUMP_MIN_PCT,
                rpm: None,
                duty_pct: NZXT_PUMP_MIN_PCT,
                manual_pct: None,
            }],
            NzxtModel::Kraken(KrakenSeries::Z53 | KrakenSeries::Kraken2023) => vec![
                NzxtChannel {
                    index: 0,
                    id: "pump".into(),
                    label: "Pompe AIO".into(),
                    min_pct: NZXT_PUMP_MIN_PCT,
                    rpm: None,
                    duty_pct: NZXT_PUMP_MIN_PCT,
                    manual_pct: None,
                },
                NzxtChannel {
                    index: 1,
                    id: "fan".into(),
                    label: "Ventilateurs radiateur".into(),
                    min_pct: NZXT_FAN_MIN_PCT,
                    rpm: None,
                    duty_pct: 50,
                    manual_pct: None,
                },
            ],
            NzxtModel::SmartDeviceV2 => (0..3)
                .map(|i| NzxtChannel {
                    index: i,
                    id: format!("fan_{i}"),
                    label: format!("Ventilateur #{}", i + 1),
                    min_pct: NZXT_FAN_MIN_PCT,
                    rpm: None,
                    duty_pct: 50,
                    manual_pct: None,
                })
                .collect(),
        };
        Self {
            id,
            model,
            serial,
            kind,
            firmware: None,
            coolant_temp_c: None,
            channels,
            io,
        }
    }

    pub(crate) fn init_and_refresh(&mut self) {
        match self.kind {
            NzxtModel::SmartDeviceV2 => {
                let _ = self.io.write(&pad_nzxt_report(&SMART2_CMD_SET_INTERVAL));
                let _ = self.io.write(&pad_nzxt_report(&SMART2_CMD_DETECT_FANS));
            }
            NzxtModel::Kraken(_) => {
                let _ = self.io.write(&pad_nzxt_report(&KRAKEN_CMD_GET_FW));
                let _ = self.io.write(&pad_nzxt_report(&KRAKEN_CMD_SET_INTERVAL));
                let _ = self.io.write(&pad_nzxt_report(&KRAKEN_CMD_FINISH_INIT));
            }
        }
        self.poll_reports(4);
    }

    fn poll_reports(&mut self, max_reads: usize) {
        if let NzxtModel::Kraken(KrakenSeries::Z53 | KrakenSeries::Kraken2023) = self.kind {
            let _ = self.io.write(&pad_nzxt_report(&KRAKEN_CMD_GET_STATUS));
        }
        let mut buf = [0u8; NZXT_REPORT_LEN];
        for _ in 0..max_reads {
            let Ok(n) = self.io.read_timeout(&mut buf, 120) else {
                break;
            };
            if n == 0 {
                break;
            }
            self.ingest_report(&buf[..n]);
        }
    }

    pub(crate) fn ingest_report(&mut self, raw: &[u8]) {
        if raw.is_empty() {
            return;
        }
        let report_id = raw[0];
        match (self.kind, report_id) {
            (NzxtModel::Kraken(series), KRAKEN_REPORT_FW) if raw.len() >= 20 => {
                self.firmware = Some(format!("{}.{}.{}", raw[17], raw[18], raw[19]));
                let _ = series;
            }
            (NzxtModel::Kraken(series), KRAKEN_REPORT_STATUS) if raw.len() >= 20 => {
                let c = raw[15] as f32 + (raw[16].min(9) as f32) / 10.0;
                if (5.0..=105.0).contains(&c) {
                    self.coolant_temp_c = Some((c * 10.0).round() / 10.0);
                }
                let pump_rpm = u16::from_le_bytes([raw[17], raw[18]]) as u32;
                let pump_duty = raw[19].min(100);
                if let Some(ch0) = self.channels.get_mut(0) {
                    ch0.rpm = Some(pump_rpm);
                    ch0.duty_pct = pump_duty;
                }
                if series != KrakenSeries::X53
                    && raw.len() >= 26
                    && let Some(ch1) = self.channels.get_mut(1)
                {
                    ch1.rpm = Some(u16::from_le_bytes([raw[23], raw[24]]) as u32);
                    ch1.duty_pct = raw[25].min(100);
                }
            }
            (NzxtModel::SmartDeviceV2, SMART2_REPORT_FAN_CONFIG) if raw.len() >= 19 => {
                // Keep only connected channels if any are detected non-zero.
                let types = &raw[16..19];
                if types.iter().any(|&t| t != 0) {
                    self.channels
                        .retain(|c| types.get(c.index).copied().unwrap_or(0) != 0);
                }
            }
            (NzxtModel::SmartDeviceV2, SMART2_REPORT_STATUS)
                if raw.len() >= 43 && raw[1] == SMART2_STATUS_SPEED =>
            {
                for ch in &mut self.channels {
                    let rpm_off = 24 + ch.index * 2;
                    let duty_off = 40 + ch.index;
                    if duty_off < raw.len() {
                        ch.rpm = Some(u16::from_le_bytes([raw[rpm_off], raw[rpm_off + 1]]) as u32);
                        ch.duty_pct = raw[duty_off].min(100);
                    }
                }
            }
            _ => {}
        }
    }

    fn channel_to_fan(ch: &NzxtChannel) -> FanInfo {
        FanInfo {
            id: ch.id.clone(),
            name: ch.label.clone(),
            rpm: ch.rpm,
            max_rpm: None,
            min_pct: ch.min_pct,
            manual_pct: ch.manual_pct,
        }
    }
}

impl Device for NzxtCoolingDevice {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.id.clone(),
            vendor: "NZXT".into(),
            model: self.model.clone(),
            kind: DeviceKind::Cooler,
            connection: Connection::Usb,
            firmware: self.firmware.clone(),
        }
    }

    fn specs(&self) -> Vec<DeviceSpec> {
        let mut out = vec![
            DeviceSpec::simple("Canaux contrôlables", self.channels.len().to_string()),
            DeviceSpec::expert("Numéro de série", self.serial.clone(), true),
        ];
        if let Some(c) = self.coolant_temp_c {
            out.push(DeviceSpec::simple(
                "Température liquide AIO",
                format!("{c:.1} °C"),
            ));
        }
        out
    }

    fn cooling(&mut self) -> Option<&mut dyn Cooling> {
        (!self.channels.is_empty()).then_some(self as &mut dyn Cooling)
    }
}

impl Cooling for NzxtCoolingDevice {
    fn fans(&self) -> Vec<FanInfo> {
        self.channels.iter().map(Self::channel_to_fan).collect()
    }

    fn refresh(&mut self) {
        self.poll_reports(3);
    }

    fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()> {
        let indices: Vec<usize> = match fan {
            Some(fid) => {
                let pos = self
                    .channels
                    .iter()
                    .position(|c| c.id == fid)
                    .with_context(|| format!("Canal NZXT introuvable : {fid}"))?;
                vec![pos]
            }
            None => (0..self.channels.len()).collect(),
        };

        match self.kind {
            NzxtModel::Kraken(series) => {
                for pos in indices {
                    let ch_idx = self.channels[pos].index as u8;
                    let info = Self::channel_to_fan(&self.channels[pos]);
                    let (curve, next_manual) = match clamp_target(&info, target) {
                        FanTarget::Auto => (default_kraken_curve(ch_idx), None),
                        FanTarget::Fixed(pct) => (fixed_kraken_curve(ch_idx, pct), Some(pct)),
                    };
                    let pkt = encode_kraken_curve(series, ch_idx, &curve);
                    self.io
                        .write(&pkt)
                        .context("Écriture courbe HID NZXT Kraken")?;
                    self.channels[pos].manual_pct = next_manual;
                }
            }
            NzxtModel::SmartDeviceV2 => {
                let mut mask = 0u8;
                let mut duties = [50u8; 3];
                for ch in &self.channels {
                    if ch.index < 3 {
                        duties[ch.index] = ch
                            .manual_pct
                            .unwrap_or(ch.duty_pct)
                            .clamp(NZXT_FAN_MIN_PCT, 100);
                    }
                }
                for &pos in &indices {
                    let info = Self::channel_to_fan(&self.channels[pos]);
                    let hw_idx = self.channels[pos].index;
                    mask |= 1u8 << hw_idx;
                    match clamp_target(&info, target) {
                        FanTarget::Auto => {
                            duties[hw_idx] = 50;
                            self.channels[pos].manual_pct = None;
                        }
                        FanTarget::Fixed(pct) => {
                            duties[hw_idx] = pct;
                            self.channels[pos].manual_pct = Some(pct);
                        }
                    }
                }
                let pkt = encode_smart2_set_fans(mask, duties);
                self.io
                    .write(&pkt)
                    .context("Écriture ventilateurs NZXT Smart Device V2")?;
            }
        }
        Ok(())
    }

    fn overridden(&self) -> bool {
        self.channels.iter().any(|c| c.manual_pct.is_some())
    }

    fn restore_firmware_control(&mut self) -> Result<()> {
        self.set_fan(None, FanTarget::Auto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct MockNzxtHid {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        reads: Mutex<VecDeque<Vec<u8>>>,
    }

    impl NzxtHidIo for MockNzxtHid {
        fn write(&self, data: &[u8]) -> std::result::Result<usize, hidapi::HidError> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(data.len())
        }

        fn read_timeout(
            &self,
            buf: &mut [u8],
            _timeout_ms: i32,
        ) -> std::result::Result<usize, hidapi::HidError> {
            let Some(pkt) = self.reads.lock().unwrap().pop_front() else {
                return Ok(0);
            };
            let n = pkt.len().min(buf.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            Ok(n)
        }
    }

    #[test]
    fn kraken_2023_parses_telemetry_enforces_pump_floor_and_restores_curve() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut status = vec![0u8; 64];
        status[0] = KRAKEN_REPORT_STATUS;
        status[15] = 32;
        status[16] = 4; // 32.4 C coolant
        status[17..=18].copy_from_slice(&2450u16.to_le_bytes());
        status[19] = 75;
        status[23..=24].copy_from_slice(&1180u16.to_le_bytes());
        status[25] = 48;

        let hid = MockNzxtHid {
            writes: Arc::clone(&writes),
            reads: Mutex::new(VecDeque::from([status])),
        };
        let mut dev = NzxtCoolingDevice::from_io(
            "nzxt_test".into(),
            "NZXT Kraken 2023 Elite".into(),
            "N1".into(),
            NzxtModel::Kraken(KrakenSeries::Kraken2023),
            Box::new(hid),
        );
        dev.init_and_refresh();
        assert_eq!(dev.coolant_temp_c, Some(32.4));
        assert_eq!(dev.fans()[0].rpm, Some(2450));
        assert_eq!(dev.fans()[1].rpm, Some(1180));

        // Pump safety floor: 10% -> 45%, Fan floor: 10% -> 30%
        dev.set_fan(Some("pump"), FanTarget::Fixed(10)).unwrap();
        dev.set_fan(Some("fan"), FanTarget::Fixed(10)).unwrap();
        assert!(dev.overridden());
        assert_eq!(dev.fans()[0].manual_pct, Some(45));
        assert_eq!(dev.fans()[1].manual_pct, Some(30));

        dev.restore_firmware_control().unwrap();
        assert!(!dev.overridden());

        let log = writes.lock().unwrap();
        let pump_pkt = log.iter().find(|p| p[0] == 0x72 && p[1] == 1).unwrap();
        assert_eq!(pump_pkt[2], 1);
        assert_eq!(pump_pkt[4], 45);
        assert_eq!(pump_pkt[4 + 39], 100); // critical temp safeguard
        let fan_pkt = log.iter().find(|p| p[0] == 0x72 && p[1] == 2).unwrap();
        assert_eq!(fan_pkt[2], 1);
        assert_eq!(fan_pkt[3], 1);
        assert_eq!(fan_pkt[4], 30);
    }

    #[test]
    fn smart_device_v2_parses_speed_report_and_masks_channels() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut speed = vec![0u8; 64];
        speed[0] = SMART2_REPORT_STATUS;
        speed[1] = SMART2_STATUS_SPEED;
        speed[24..26].copy_from_slice(&920u16.to_le_bytes());
        speed[40] = 42;

        let hid = MockNzxtHid {
            writes: Arc::clone(&writes),
            reads: Mutex::new(VecDeque::from([speed])),
        };
        let mut dev = NzxtCoolingDevice::from_io(
            "nzxt_s2".into(),
            "NZXT Smart Device V2".into(),
            "S2".into(),
            NzxtModel::SmartDeviceV2,
            Box::new(hid),
        );
        dev.init_and_refresh();
        assert_eq!(dev.fans()[0].rpm, Some(920));
        dev.set_fan(Some("fan_1"), FanTarget::Fixed(75)).unwrap();
        let last = writes.lock().unwrap().last().unwrap().clone();
        assert_eq!(last[0], 0x62);
        assert_eq!(last[1], 0x01);
        assert_eq!(last[2], 0x02); // bit 1 = fan_1
        assert_eq!(last[4], 75);
    }
}
