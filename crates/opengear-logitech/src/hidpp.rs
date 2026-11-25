use crate::features::*;
use hidapi::{HidApi, HidDevice};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::trace;

pub const REPORT_ID_SHORT: u8 = 0x10;
pub const REPORT_ID_LONG: u8 = 0x11;
pub const SHORT_LEN: usize = 7;
pub const LONG_LEN: usize = 20;
const SW_ID: u8 = 0x0A;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatteryState {
    Discharging,
    Charging,
    AlmostFull,
    Full,
    SlowCharging,
    Error,
    Unknown(u8),
}

impl BatteryState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discharging => "Sur batterie",
            Self::Charging => "En charge",
            Self::AlmostFull => "Presque pleine",
            Self::Full => "Chargée (100%)",
            Self::SlowCharging => "Charge lente",
            Self::Error => "Erreur charge",
            Self::Unknown(_) => "Inconnu",
        }
    }

    pub fn is_charging(self) -> bool {
        matches!(self, Self::Charging | Self::AlmostFull | Self::SlowCharging)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub percent: Option<u8>,
    pub voltage_mv: Option<u16>,
    pub state: BatteryState,
}

/// Parse HID++ 2.0 `0x1004` Unified Battery charging status byte (`0..=7`).
pub(crate) fn parse_unified_battery_status(code: u8) -> BatteryState {
    match code {
        0 => BatteryState::Discharging,
        1 => BatteryState::Charging,
        2 => BatteryState::AlmostFull,
        3 => BatteryState::Full,
        4 => BatteryState::SlowCharging,
        5..=7 => BatteryState::Error,
        other => BatteryState::Unknown(other),
    }
}

/// Abstraction over raw HID I/O for mockable unit testing.
pub trait HidIo: Send {
    fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError>;
    fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, hidapi::HidError>;
}

struct HidApiIo(HidDevice);

impl HidIo for HidApiIo {
    fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError> {
        self.0.write(data)
    }

    fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, hidapi::HidError> {
        self.0.read_timeout(buf, timeout_ms)
    }
}

#[derive(Debug, Error)]
pub enum HidppError {
    #[error("Aucun périphérique Logitech HID++ détecté")]
    DeviceNotFound,
    #[error("Erreur HID : {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("Timeout HID++ (périphérique en veille ou hors ligne)")]
    Timeout,
    #[error("Erreur HID++ 2.0 (code 0x{0:02X} : {1})")]
    Protocol(u8, &'static str),
    #[error("Erreur HID++ 1.0 (code 0x{0:02X})")]
    Protocol10(u8),
    #[error("Fonctionnalité HID++ 0x{0:04X} non supportée par cet appareil")]
    UnsupportedFeature(u16),
    #[error("Flash embarqué : {0}")]
    Flash(String),
}

impl HidppError {
    pub fn from_hidpp_code(code: u8) -> Self {
        let msg = match code {
            0x01 => "Unknown",
            0x02 => "Invalid Argument",
            0x03 => "Out of Range",
            0x04 => "Hardware Error",
            0x05 => "Logitech Internal Error",
            0x06 => "Invalid Feature Index",
            0x07 => "Invalid Function ID",
            0x08 => "Busy",
            0x09 => "Unsupported",
            _ => "Unknown Error Code",
        };
        Self::Protocol(code, msg)
    }
}

pub struct HidppTransport {
    dev: Box<dyn HidIo>,
    pub vid: u16,
    pub pid: u16,
    pub product_string: String,
    pub path: String,
    pub device_index: u8,
    pub use_long_only: bool,
    pub short_only: bool,
    pub proto_major: u8,
    pub proto_minor: u8,
    pub features: Vec<FeatureEntry>,
}

impl HidppTransport {
    #[cfg(test)]
    pub(crate) fn from_io(dev: Box<dyn HidIo>, device_index: u8, use_long_only: bool) -> Self {
        Self {
            dev,
            vid: LOGITECH_VID,
            pid: 0x4093,
            product_string: "Mock Logitech".into(),
            path: "mock://logi".into(),
            device_index,
            use_long_only,
            short_only: false,
            proto_major: 4,
            proto_minor: 5,
            features: Vec::new(),
        }
    }

    pub fn open_matching(
        pid_filter: Option<u16>,
        usage_page: u16,
        preferred_usage: Option<u16>,
    ) -> Result<Self, HidppError> {
        let all = Self::open_all_matching_filtered(pid_filter, usage_page, preferred_usage);
        all.into_iter().next().ok_or(HidppError::DeviceNotFound)
    }

    pub fn open_all_matching(usage_page: u16, preferred_usage: Option<u16>) -> Vec<Self> {
        Self::open_all_matching_filtered(None, usage_page, preferred_usage)
    }

    fn open_all_matching_filtered(
        pid_filter: Option<u16>,
        usage_page: u16,
        preferred_usage: Option<u16>,
    ) -> Vec<Self> {
        let Ok(api) = HidApi::new() else {
            return Vec::new();
        };

        // Group candidates by (vid, pid, interface_number, serial_or_path) so devices with empty
        // serials never collide and only the best usage per physical interface is opened.
        let mut by_iface: Vec<(u16, u16, i32, String, u16, CString, String)> = Vec::new();
        for d in api.device_list() {
            if d.vendor_id() != LOGITECH_VID || d.usage_page() != usage_page {
                continue;
            }
            if pid_filter.is_some_and(|p| d.product_id() != p) {
                continue;
            }
            let vid = d.vendor_id();
            let pid = d.product_id();
            let iface = d.interface_number();
            let path_cstr = d.path().to_owned();
            let serial_raw = d.serial_number().unwrap_or("").trim();
            let group_key = if serial_raw.is_empty() {
                path_cstr.to_string_lossy().into_owned()
            } else {
                serial_raw.to_string()
            };
            let usage = d.usage();
            let product = d
                .product_string()
                .unwrap_or("Logitech HID++ Device")
                .to_string();

            let is_better = |u: u16| -> u8 {
                if let Some(pref) = preferred_usage {
                    if u == pref { 0 } else { 1 }
                } else if u == 0x0002 {
                    0
                } else {
                    1
                }
            };

            if let Some(existing) = by_iface
                .iter_mut()
                .find(|e| e.0 == vid && e.1 == pid && e.2 == iface && e.3 == group_key)
            {
                if is_better(usage) < is_better(existing.4) {
                    existing.4 = usage;
                    existing.5 = path_cstr;
                    existing.6 = product;
                }
            } else {
                by_iface.push((vid, pid, iface, group_key, usage, path_cstr, product));
            }
        }

        let mut results = Vec::new();
        for (vid, pid, _iface, _key, usage, path_cstr, product) in by_iface {
            let path_str = path_cstr.to_string_lossy().into_owned();
            let use_long_only = usage == 0x0002 || usage_page == 0xFF43;
            let short_only = usage == 0x0001 && usage_page == 0xFF00;

            let mut matched_slots = Vec::new();
            if let Ok(probe_dev) = api.open_path(&path_cstr) {
                let mut probe = Self {
                    dev: Box::new(HidApiIo(probe_dev)),
                    vid,
                    pid,
                    product_string: product.clone(),
                    path: path_str.clone(),
                    device_index: 0xFF,
                    use_long_only,
                    short_only,
                    proto_major: 0,
                    proto_minor: 0,
                    features: Vec::new(),
                };
                probe.drain();

                // Probe wired/direct 0xFF FIRST (fast 150 ms) so wired mice don't stall ~2s on receiver slots.
                probe.device_index = 0xFF;
                if let Ok((maj, min)) = probe.ping_protocol_version_timeout(150) {
                    matched_slots.push((0xFF, maj, min));
                } else if usage_page != 0xFF43 {
                    for idx in 0x01..=0x06u8 {
                        probe.device_index = idx;
                        let timeout = if idx == 0x01 { 1200 } else { 160 };
                        if let Ok((maj, min)) = probe.ping_protocol_version_timeout(timeout) {
                            matched_slots.push((idx, maj, min));
                        }
                    }
                }
            }

            if matched_slots.is_empty() && usage_page == 0xFF43 {
                if let Ok(dev) = api.open_path(&path_cstr) {
                    let t = Self {
                        dev: Box::new(HidApiIo(dev)),
                        vid,
                        pid,
                        product_string: product.clone(),
                        path: path_str.clone(),
                        device_index: 0xFF,
                        use_long_only,
                        short_only,
                        proto_major: 0,
                        proto_minor: 0,
                        features: Vec::new(),
                    };
                    t.drain();
                    results.push(t);
                }
                continue;
            }

            for (idx, maj, min) in matched_slots {
                if let Ok(dev) = api.open_path(&path_cstr) {
                    let mut t = Self {
                        dev: Box::new(HidApiIo(dev)),
                        vid,
                        pid,
                        product_string: product.clone(),
                        path: path_str.clone(),
                        device_index: idx,
                        use_long_only,
                        short_only,
                        proto_major: maj,
                        proto_minor: min,
                        features: Vec::new(),
                    };
                    t.drain();
                    let _ = t.enumerate_features();
                    results.push(t);
                }
            }
        }
        results
    }

    pub fn read_device_type(&self) -> Option<u8> {
        let r = self.request_feature(FEAT_DEVICE_NAME, 0x20, &[]).ok()?;
        r.first().copied()
    }

    pub fn drain(&self) {
        let mut buf = [0u8; 64];
        for _ in 0..8 {
            match self.dev.read_timeout(&mut buf, 2) {
                Ok(n) if n > 0 => continue,
                _ => break,
            }
        }
    }

    pub fn write_raw_long(&self, dev_idx: u8, payload: &[u8]) -> Result<(), HidppError> {
        let mut buf = [0u8; LONG_LEN];
        buf[0] = REPORT_ID_LONG;
        buf[1] = dev_idx;
        let n = payload.len().min(LONG_LEN - 2);
        buf[2..2 + n].copy_from_slice(&payload[..n]);
        self.dev.write(&buf)?;
        Ok(())
    }

    pub fn request(
        &self,
        feature_idx: u8,
        fn_id: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, HidppError> {
        self.request_with_timeout(feature_idx, fn_id, params, 350)
    }

    pub fn request_with_timeout(
        &self,
        feature_idx: u8,
        fn_id: u8,
        params: &[u8],
        timeout_ms: i32,
    ) -> Result<Vec<u8>, HidppError> {
        if self.short_only && params.len() > 3 {
            return Err(HidppError::Protocol(
                0x02,
                "Endpoint HID++ court (usage 0x0001) limité à 3 octets de paramètres",
            ));
        }
        let use_long = self.use_long_only || params.len() > 3;
        let len = if use_long { LONG_LEN } else { SHORT_LEN };
        let mut tx = vec![0u8; len];
        tx[0] = if use_long {
            REPORT_ID_LONG
        } else {
            REPORT_ID_SHORT
        };
        tx[1] = self.device_index;
        tx[2] = feature_idx;
        let addr = (fn_id & 0xF0) | SW_ID;
        tx[3] = addr;
        if params.len() > len - 4 {
            return Err(HidppError::Protocol(
                0x02,
                "Paramètres HID++ trop longs pour un rapport (16 octets max)",
            ));
        }
        tx[4..4 + params.len()].copy_from_slice(params);

        for attempt in 0..4u8 {
            match self.request_once(&tx, feature_idx, addr, timeout_ms) {
                Err(HidppError::Protocol(0x08, _)) if attempt < 3 => {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                other => return other,
            }
        }
        Err(HidppError::Timeout)
    }

    fn request_once(
        &self,
        tx: &[u8],
        feature_idx: u8,
        addr: u8,
        timeout_ms: i32,
    ) -> Result<Vec<u8>, HidppError> {
        trace!("HID++ TX: {:02X?}", tx);
        self.dev.write(tx)?;

        let mut rx = [0u8; 64];
        let deadline = Instant::now() + Duration::from_millis((timeout_ms.max(50) as u64) * 2);
        for _ in 0..16 {
            let rem = deadline.saturating_duration_since(Instant::now());
            if rem.is_zero() {
                break;
            }
            let wait = (rem.as_millis() as i32).clamp(1, timeout_ms.max(1));
            let n = self.dev.read_timeout(&mut rx, wait)?;
            if n == 0 {
                return Err(HidppError::Timeout);
            }
            let n = n.min(rx.len());
            trace!("HID++ RX ({n}B): {:02X?}", &rx[..n]);
            if n < 6 {
                continue;
            }
            let rep_id = rx[0];
            if rep_id != REPORT_ID_SHORT && rep_id != REPORT_ID_LONG {
                continue;
            }
            if rx[1] != self.device_index && self.device_index != 0xFF {
                continue;
            }
            // HID++ 2.0 error response: sub_id == 0xFF
            if rx[2] == 0xFF && rx[3] == feature_idx && rx[4] == addr {
                return Err(HidppError::from_hidpp_code(rx[5]));
            }
            // HID++ 1.0 error response: sub_id == 0x8F
            if rx[2] == 0x8F && rx[3] == feature_idx && rx[4] == addr {
                return Err(HidppError::Protocol10(rx[5]));
            }
            if rx[2] == feature_idx && rx[3] == addr {
                return Ok(rx[4..n].to_vec());
            }
        }
        Err(HidppError::Timeout)
    }

    pub fn ping_protocol_version_timeout(&self, timeout_ms: i32) -> Result<(u8, u8), HidppError> {
        let resp = self.request_with_timeout(0x00, 0x10, &[0x00, 0x00, 0xAA], timeout_ms)?;
        if resp.len() >= 2 {
            Ok((resp[0], resp[1]))
        } else {
            Err(HidppError::Timeout)
        }
    }

    pub fn get_feature_index(&self, feature_id: u16) -> Option<u8> {
        if feature_id == FEAT_ROOT {
            return Some(0);
        }
        self.features
            .iter()
            .find(|f| f.id == feature_id)
            .map(|f| f.index)
    }

    pub fn request_feature(
        &self,
        feature_id: u16,
        fn_id: u8,
        params: &[u8],
    ) -> Result<Vec<u8>, HidppError> {
        let idx = self
            .get_feature_index(feature_id)
            .ok_or(HidppError::UnsupportedFeature(feature_id))?;
        self.request(idx, fn_id, params)
    }

    fn enumerate_features(&mut self) -> Result<(), HidppError> {
        let fset = self.request(0x00, 0x00, &FEAT_FEATURE_SET.to_be_bytes())?;
        let fset_idx = fset.first().copied().unwrap_or(0);
        if fset_idx == 0 {
            return Ok(());
        }

        let count_resp = self.request(fset_idx, 0x00, &[])?;
        let count = count_resp.first().copied().unwrap_or(0) as usize;
        let total = count + 1;

        let mut list = Vec::with_capacity(total);
        for i in 0..total.min(80) {
            if let Ok(r) = self.request(fset_idx, 0x10, &[i as u8])
                && r.len() >= 3
            {
                let id = u16::from_be_bytes([r[0], r[1]]);
                let flags = r[2];
                let version = r.get(3).copied().unwrap_or(0);
                list.push(FeatureEntry {
                    index: i as u8,
                    id,
                    flags,
                    version,
                    name: feature_name(id).to_string(),
                });
            }
        }
        self.features = list;
        Ok(())
    }

    pub fn read_device_name(&self) -> Option<String> {
        let len_resp = self.request_feature(FEAT_DEVICE_NAME, 0x00, &[]).ok()?;
        let name_len = len_resp.first().copied().unwrap_or(0) as usize;
        if name_len == 0 || name_len > 64 {
            return None;
        }
        let mut bytes = Vec::with_capacity(name_len);
        while bytes.len() < name_len {
            let chunk = self
                .request_feature(FEAT_DEVICE_NAME, 0x10, &[bytes.len() as u8])
                .ok()?;
            if chunk.is_empty() {
                break;
            }
            let before = bytes.len();
            let mut hit_terminator = false;
            for &b in &chunk {
                if bytes.len() >= name_len {
                    break;
                }
                if b == 0 {
                    hit_terminator = true;
                    break;
                }
                bytes.push(b);
            }
            if hit_terminator || bytes.len() == before {
                break;
            }
        }
        let s = String::from_utf8_lossy(&bytes).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    pub fn read_firmware_versions(&self) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(cnt_resp) = self.request_feature(FEAT_DEVICE_FW_VERSION, 0x00, &[]) else {
            return out;
        };
        let count = cnt_resp.first().copied().unwrap_or(0).min(8);
        for entity in 0..count {
            if let Ok(r) = self.request_feature(FEAT_DEVICE_FW_VERSION, 0x10, &[entity])
                && r.len() >= 8
            {
                let prefix = String::from_utf8_lossy(&r[1..4])
                    .trim_end_matches('\0')
                    .to_string();
                let rev = r[4];
                let minor = r[5];
                let build = u16::from_be_bytes([r[6], r[7]]);
                if !prefix.is_empty() {
                    out.push(format!("{prefix} {rev:02X}.{minor:02X}.B{build:04X}"));
                }
            }
        }
        out
    }
}

/// Shared HID++ 2.0 battery reader (`0x1004` Unified Battery -> `0x1000` -> `0x1001` Voltage).
pub(crate) fn read_battery_info(t: &HidppTransport) -> Option<BatteryInfo> {
    if let Ok(r) = t.request_feature(FEAT_UNIFIED_BATTERY, 0x10, &[])
        && r.len() >= 3
    {
        let pct = r[0];
        return Some(BatteryInfo {
            percent: if pct <= 100 { Some(pct) } else { None },
            voltage_mv: None,
            state: parse_unified_battery_status(r[2]),
        });
    }
    if let Ok(r) = t.request_feature(FEAT_BATTERY_STATUS, 0x00, &[])
        && r.len() >= 3
    {
        let pct = r[0];
        return Some(BatteryInfo {
            percent: Some(pct.min(100)),
            voltage_mv: None,
            state: parse_unified_battery_status(r[2]),
        });
    }
    if let Ok(r) = t.request_feature(FEAT_BATTERY_VOLTAGE, 0x00, &[])
        && r.len() >= 3
    {
        let mv = u16::from_be_bytes([r[0], r[1]]);
        let flags = r[2];
        let state = if (flags & 0x80) != 0 {
            BatteryState::Charging
        } else {
            BatteryState::Discharging
        };
        let pct = ((mv.saturating_sub(3500) as f32 / 680.0) * 100.0).clamp(0.0, 100.0) as u8;
        return Some(BatteryInfo {
            percent: Some(pct),
            voltage_mv: Some(mv),
            state,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct MockLogiIo {
        reads: Mutex<VecDeque<Vec<u8>>>,
    }

    impl MockLogiIo {
        fn new(reads: Vec<Vec<u8>>) -> Self {
            Self {
                reads: Mutex::new(reads.into()),
            }
        }
    }

    impl HidIo for MockLogiIo {
        fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError> {
            Ok(data.len())
        }
        fn read_timeout(
            &self,
            buf: &mut [u8],
            _timeout_ms: i32,
        ) -> Result<usize, hidapi::HidError> {
            let Some(pkt) = self.reads.lock().unwrap().pop_front() else {
                return Ok(0);
            };
            let n = pkt.len().min(buf.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            Ok(n)
        }
    }

    #[test]
    fn unified_battery_status_maps_almost_full_and_slow_charging_correctly() {
        assert_eq!(parse_unified_battery_status(0), BatteryState::Discharging);
        assert_eq!(parse_unified_battery_status(1), BatteryState::Charging);
        assert_eq!(parse_unified_battery_status(2), BatteryState::AlmostFull);
        assert_eq!(parse_unified_battery_status(3), BatteryState::Full);
        assert_eq!(parse_unified_battery_status(4), BatteryState::SlowCharging);
        assert_eq!(parse_unified_battery_status(5), BatteryState::Error);
        assert!(BatteryState::Charging.is_charging());
        assert!(!BatteryState::Discharging.is_charging());
    }

    #[test]
    fn busy_response_retries_and_succeeds() {
        let io = MockLogiIo::new(vec![
            vec![REPORT_ID_LONG, 0x01, 0xFF, 0x02, 0x10 | SW_ID, 0x08, 0],
            vec![REPORT_ID_LONG, 0x01, 0x02, 0x10 | SW_ID, 0x42, 0x99, 0],
        ]);
        let t = HidppTransport::from_io(Box::new(io), 0x01, true);
        let res = t.request(0x02, 0x10, &[]).unwrap();
        assert_eq!(&res[..2], &[0x42, 0x99]);
    }

    #[test]
    fn hidpp10_error_returns_protocol10() {
        let io = MockLogiIo::new(vec![vec![
            REPORT_ID_SHORT,
            0xFF,
            0x8F,
            0x02,
            0x10 | SW_ID,
            0x04,
            0,
        ]]);
        let t = HidppTransport::from_io(Box::new(io), 0xFF, false);
        let err = t.request(0x02, 0x10, &[]).unwrap_err();
        assert!(matches!(err, HidppError::Protocol10(0x04)));
    }
}
