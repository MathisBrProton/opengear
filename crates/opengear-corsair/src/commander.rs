//! Corsair Commander Pro (`0x0C10`) & Obsidian 1000D (`0x1D00`) USB HID fan/temp hub.

use crate::proto::CORSAIR_VID;
use crate::transport::HidIo;
use anyhow::{Context, Result, bail};
use hidapi::HidApi;
use opengear_core::driver::{Connection, Device, DeviceInfo, DeviceKind, DeviceSpec};
use opengear_core::fans::{Cooling, FanInfo, FanTarget, clamp_target};
use std::ffi::CString;

pub const PID_COMMANDER_PRO: u16 = 0x0C10;
pub const PID_OBSIDIAN_1000D: u16 = 0x1D00;

pub const CPRO_NUM_FANS: usize = 6;
pub const CPRO_NUM_TEMPS: usize = 4;
pub const CPRO_MIN_FAN_PCT: u8 = 30;

const CPRO_OUT_REPORT_LEN: usize = 64; // 0x00 report id + 63 byte payload
const CPRO_IN_REPORT_LEN: usize = 16;

const CTL_GET_FW_VER: u8 = 0x02;
const CTL_GET_TMP_CNCT: u8 = 0x10;
const CTL_GET_TMP: u8 = 0x11;
const CTL_GET_FAN_CNCT: u8 = 0x20;
const CTL_GET_FAN_RPM: u8 = 0x21;
const CTL_GET_FAN_PWM: u8 = 0x22;
const CTL_SET_FAN_FPWM: u8 = 0x23;
const CTL_SET_FAN_CURVE: u8 = 0x25;

pub(crate) fn cpro_model_name(pid: u16) -> Option<&'static str> {
    match pid {
        PID_COMMANDER_PRO => Some("Corsair Commander Pro"),
        PID_OBSIDIAN_1000D => Some("Corsair Obsidian 1000D Fan Hub"),
        _ => None,
    }
}

/// Parse a centi-Celsius signed 16-bit big-endian thermistor reading.
pub(crate) fn parse_cpro_temp_c(hi: u8, lo: u8) -> f32 {
    let raw = i16::from_be_bytes([hi, lo]);
    (raw as f32 / 100.0 * 10.0).round() / 10.0
}

/// Build a 64-byte HID write packet (`[0x00, cmd, b1, b2, b3, 0...]`) for Windows `hidapi`.
pub(crate) fn encode_cpro_cmd(cmd: u8, b1: u8, b2: u8, b3: u8) -> [u8; CPRO_OUT_REPORT_LEN] {
    let mut pkt = [0u8; CPRO_OUT_REPORT_LEN];
    pkt[1] = cmd;
    pkt[2] = b1;
    pkt[3] = b2;
    pkt[4] = b3;
    pkt
}

/// Build a 64-byte hardware curve command (`CTL_SET_FAN_CURVE = 0x25`) to hand control back to firmware.
pub(crate) fn encode_cpro_auto_curve(channel: u8) -> [u8; CPRO_OUT_REPORT_LEN] {
    // 6 (temperature centi-C BE, RPM BE) pairs from 20 C -> 800 RPM up to 60 C -> 2200 RPM.
    const TEMPS_C: [i16; 6] = [2000, 3000, 3800, 4500, 5200, 6000];
    const RPMS: [i16; 6] = [800, 1000, 1300, 1600, 1900, 2200];
    let mut pkt = [0u8; CPRO_OUT_REPORT_LEN];
    pkt[1] = CTL_SET_FAN_CURVE;
    pkt[2] = channel;
    pkt[3] = 0x00; // Temp sensor #1 reference
    for i in 0..6 {
        let tb = TEMPS_C[i].to_be_bytes();
        let rb = RPMS[i].to_be_bytes();
        pkt[4 + i * 2] = tb[0];
        pkt[4 + i * 2 + 1] = tb[1];
        pkt[16 + i * 2] = rb[0];
        pkt[16 + i * 2 + 1] = rb[1];
    }
    pkt
}

struct HidApiRaw(hidapi::HidDevice);

impl HidIo for HidApiRaw {
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

#[derive(Debug, Clone)]
pub struct CproFanState {
    pub index: usize,
    pub is_pwm: bool,
    pub rpm: u32,
    pub manual_pct: Option<u8>,
}

/// Corsair Commander Pro / 1000D cooling hub exposed via [`Device`] + [`Cooling`].
pub struct CorsairCommanderProDevice {
    id: String,
    model: String,
    serial: String,
    firmware: Option<String>,
    io: Box<dyn HidIo>,
    fans: Vec<CproFanState>,
    temps_c: Vec<(usize, f32)>,
}

impl CorsairCommanderProDevice {
    pub fn discover_all() -> Vec<Self> {
        let Ok(api) = HidApi::new() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut seen_paths = Vec::new();
        for info in api.device_list() {
            if info.vendor_id() != CORSAIR_VID {
                continue;
            }
            let pid = info.product_id();
            let Some(default_name) = cpro_model_name(pid) else {
                continue;
            };
            let path = info.path().to_string_lossy().to_string();
            if seen_paths.contains(&path) {
                continue;
            }
            let Ok(cpath) = CString::new(info.path().to_bytes()) else {
                continue;
            };
            let Ok(dev) = api.open_path(cpath.as_c_str()) else {
                continue;
            };
            seen_paths.push(path);
            let model = info
                .product_string()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(default_name)
                .to_string();
            let serial = info.serial_number().unwrap_or("0").trim().to_string();
            let mut cpro = Self::from_io(
                format!("corsair_cpro_{pid:04X}_{serial}"),
                model,
                serial,
                Box::new(HidApiRaw(dev)),
            );
            cpro.probe_initial();
            if !cpro.fans.is_empty() || !cpro.temps_c.is_empty() {
                out.push(cpro);
            }
        }
        out
    }

    pub(crate) fn from_io(id: String, model: String, serial: String, io: Box<dyn HidIo>) -> Self {
        Self {
            id,
            model,
            serial,
            firmware: None,
            io,
            fans: Vec::new(),
            temps_c: Vec::new(),
        }
    }

    fn exchange_raw(&self, tx: &[u8]) -> Result<[u8; CPRO_IN_REPORT_LEN]> {
        self.io
            .write(tx)
            .context("Écriture HID Corsair Commander Pro")?;
        let mut rx = [0u8; 32];
        let n = self
            .io
            .read_timeout(&mut rx, 250)
            .context("Lecture HID Corsair Commander Pro")?;
        if n == 0 {
            bail!("Délai dépassé sur Corsair Commander Pro");
        }
        // Unwrap optional Windows leading 0x00 report ID if larger than 16 bytes.
        let slice = if n > CPRO_IN_REPORT_LEN && rx[0] == 0x00 {
            &rx[1..]
        } else {
            &rx[..n]
        };
        let mut out = [0u8; CPRO_IN_REPORT_LEN];
        let copy = slice.len().min(CPRO_IN_REPORT_LEN);
        out[..copy].copy_from_slice(&slice[..copy]);
        if out[0] != 0x00 && out[0] != 0x11 && out[0] != 0x12 {
            bail!("Erreur protocole Commander Pro (code 0x{:02X})", out[0]);
        }
        Ok(out)
    }

    fn cmd(&self, op: u8, b1: u8, b2: u8, b3: u8) -> Result<[u8; CPRO_IN_REPORT_LEN]> {
        let tx = encode_cpro_cmd(op, b1, b2, b3);
        self.exchange_raw(&tx)
    }

    pub(crate) fn probe_initial(&mut self) {
        if let Ok(fw) = self.cmd(CTL_GET_FW_VER, 0, 0, 0)
            && fw[0] == 0
        {
            self.firmware = Some(format!("{}.{}.{}", fw[1], fw[2], fw[3]));
        }
        if let Ok(fc) = self.cmd(CTL_GET_FAN_CNCT, 0, 0, 0)
            && fc[0] == 0
        {
            self.fans.clear();
            for ch in 0..CPRO_NUM_FANS {
                let kind = fc[1 + ch];
                if kind == 1 || kind == 2 {
                    self.fans.push(CproFanState {
                        index: ch,
                        is_pwm: kind == 2,
                        rpm: 0,
                        manual_pct: None,
                    });
                }
            }
        }
        self.refresh_readings();
    }

    fn refresh_readings(&mut self) {
        let mut updated = Vec::with_capacity(self.fans.len());
        for fan in &self.fans {
            let mut f = fan.clone();
            if let Ok(r) = self.cmd(CTL_GET_FAN_RPM, f.index as u8, 0, 0)
                && r[0] == 0
            {
                f.rpm = u16::from_be_bytes([r[1], r[2]]) as u32;
            }
            if let Ok(p) = self.cmd(CTL_GET_FAN_PWM, f.index as u8, 0, 0) {
                if p[0] == 0 {
                    f.manual_pct = Some(p[1].min(100));
                } else if p[0] == 0x12 {
                    f.manual_pct = None;
                }
            }
            updated.push(f);
        }
        self.fans = updated;
        if let Ok(tc) = self.cmd(CTL_GET_TMP_CNCT, 0, 0, 0)
            && tc[0] == 0
        {
            self.temps_c.clear();
            for ch in 0..CPRO_NUM_TEMPS {
                if tc[1 + ch] == 1
                    && let Ok(t) = self.cmd(CTL_GET_TMP, ch as u8, 0, 0)
                    && t[0] == 0
                {
                    let c = parse_cpro_temp_c(t[1], t[2]);
                    if (-20.0..=125.0).contains(&c) {
                        self.temps_c.push((ch, c));
                    }
                }
            }
        }
    }

    fn fan_info(f: &CproFanState) -> FanInfo {
        FanInfo {
            id: format!("fan_{}", f.index),
            name: format!(
                "Ventilateur #{} ({})",
                f.index + 1,
                if f.is_pwm { "PWM" } else { "DC" }
            ),
            rpm: Some(f.rpm),
            max_rpm: None,
            min_pct: CPRO_MIN_FAN_PCT,
            manual_pct: f.manual_pct,
        }
    }
}

impl Device for CorsairCommanderProDevice {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.id.clone(),
            vendor: "Corsair".into(),
            model: self.model.clone(),
            kind: DeviceKind::Cooler,
            connection: Connection::Usb,
            firmware: self.firmware.clone(),
        }
    }

    fn specs(&self) -> Vec<DeviceSpec> {
        let mut out = vec![
            DeviceSpec::simple("Canaux actifs", self.fans.len().to_string()),
            DeviceSpec::simple("Sondes thermiques", self.temps_c.len().to_string()),
            DeviceSpec::expert("Numéro de série", self.serial.clone(), true),
        ];
        for &(ch, c) in &self.temps_c {
            out.push(DeviceSpec::simple(
                format!("Sonde #{}", ch + 1),
                format!("{c:.1} °C"),
            ));
        }
        out
    }

    fn cooling(&mut self) -> Option<&mut dyn Cooling> {
        (!self.fans.is_empty()).then_some(self as &mut dyn Cooling)
    }
}

impl Cooling for CorsairCommanderProDevice {
    fn fans(&self) -> Vec<FanInfo> {
        self.fans.iter().map(Self::fan_info).collect()
    }

    fn refresh(&mut self) {
        self.refresh_readings();
    }

    fn set_fan(&mut self, fan: Option<&str>, target: FanTarget) -> Result<()> {
        let target_indices: Vec<usize> = match fan {
            Some(fid) => {
                let idx = fid
                    .strip_prefix("fan_")
                    .and_then(|s| s.parse::<usize>().ok())
                    .context("Identifiant ventilateur Commander Pro invalide")?;
                if !self.fans.iter().any(|f| f.index == idx) {
                    bail!("Canal ventilateur introuvable : {fid}");
                }
                vec![idx]
            }
            None => self.fans.iter().map(|f| f.index).collect(),
        };

        for idx in target_indices {
            let info = self
                .fans
                .iter()
                .find(|f| f.index == idx)
                .map(Self::fan_info)
                .unwrap();
            match clamp_target(&info, target) {
                FanTarget::Auto => {
                    let pkt = encode_cpro_auto_curve(idx as u8);
                    let r = self.exchange_raw(&pkt)?;
                    if r[0] != 0 {
                        bail!(
                            "Restauration courbe firmware Commander Pro #{} refusée (0x{:02X})",
                            idx + 1,
                            r[0]
                        );
                    }
                    if let Some(f) = self.fans.iter_mut().find(|f| f.index == idx) {
                        f.manual_pct = None;
                    }
                }
                FanTarget::Fixed(pct) => {
                    let r = self.cmd(CTL_SET_FAN_FPWM, idx as u8, pct, 0)?;
                    if r[0] != 0 {
                        bail!(
                            "Écriture PWM Commander Pro #{} refusée (0x{:02X})",
                            idx + 1,
                            r[0]
                        );
                    }
                    if let Some(f) = self.fans.iter_mut().find(|f| f.index == idx) {
                        f.manual_pct = Some(pct);
                    }
                }
            }
        }
        Ok(())
    }

    fn overridden(&self) -> bool {
        self.fans.iter().any(|f| f.manual_pct.is_some())
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

    struct ScriptedCproHid {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        replies: Mutex<VecDeque<Vec<u8>>>,
    }

    impl HidIo for ScriptedCproHid {
        fn write(&self, data: &[u8]) -> std::result::Result<usize, hidapi::HidError> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(data.len())
        }

        fn read_timeout(
            &self,
            buf: &mut [u8],
            _timeout_ms: i32,
        ) -> std::result::Result<usize, hidapi::HidError> {
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| vec![0u8; CPRO_IN_REPORT_LEN]);
            let n = reply.len().min(buf.len());
            buf[..n].copy_from_slice(&reply[..n]);
            Ok(n)
        }
    }

    #[test]
    fn commander_pro_probes_temps_enforces_floor_and_restores_curve() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut replies = VecDeque::new();
        let mut fw = vec![0u8; 16];
        fw[1..=3].copy_from_slice(&[0, 9, 212]);
        replies.push_back(fw);
        let mut cnct = vec![0u8; 16];
        cnct[1] = 2; // ch0 = 4-pin PWM
        cnct[2] = 0;
        replies.push_back(cnct);
        // RPM + PWM (curve mode = 0x12) for ch0
        let mut rpm = vec![0u8; 16];
        rpm[1..=2].copy_from_slice(&1350u16.to_be_bytes());
        replies.push_back(rpm);
        let mut pwm = vec![0u8; 16];
        pwm[0] = 0x12;
        replies.push_back(pwm);
        // Temp sensor connect + sensor 0 reading = 34.5 C (3450 centi-C)
        let mut tcnct = vec![0u8; 16];
        tcnct[1] = 1;
        replies.push_back(tcnct);
        let mut t0 = vec![0u8; 16];
        t0[1..=2].copy_from_slice(&3450i16.to_be_bytes());
        replies.push_back(t0);
        // Write fixed PWM + restore curve replies
        replies.push_back(vec![0u8; 16]);
        replies.push_back(vec![0u8; 16]);

        let hid = ScriptedCproHid {
            writes: Arc::clone(&writes),
            replies: Mutex::new(replies),
        };
        let mut dev = CorsairCommanderProDevice::from_io(
            "cpro_test".into(),
            "Corsair Commander Pro".into(),
            "S1".into(),
            Box::new(hid),
        );
        dev.probe_initial();
        assert_eq!(dev.firmware.as_deref(), Some("0.9.212"));
        assert_eq!(dev.fans().len(), 1);
        assert_eq!(dev.fans()[0].rpm, Some(1350));
        assert!((dev.temps_c[0].1 - 34.5).abs() < 0.01);

        // Safety floor clamps 10% -> 30%
        dev.set_fan(Some("fan_0"), FanTarget::Fixed(10)).unwrap();
        assert!(dev.overridden());
        assert_eq!(dev.fans()[0].manual_pct, Some(30));

        dev.restore_firmware_control().unwrap();
        assert!(!dev.overridden());

        let log = writes.lock().unwrap();
        let set_pkt = &log[log.len() - 2];
        assert_eq!(set_pkt[1], CTL_SET_FAN_FPWM);
        assert_eq!(set_pkt[2], 0);
        assert_eq!(set_pkt[3], 30);
        let restore_pkt = &log[log.len() - 1];
        assert_eq!(restore_pkt[1], CTL_SET_FAN_CURVE);
        assert_eq!(restore_pkt[2], 0);
    }
}
