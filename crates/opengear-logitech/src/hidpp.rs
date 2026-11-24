use crate::features::*;
use hidapi::{HidApi, HidDevice};
use std::ffi::CString;
use thiserror::Error;
use tracing::{debug, trace};

pub const REPORT_ID_SHORT: u8 = 0x10;
pub const REPORT_ID_LONG: u8 = 0x11;
pub const SHORT_LEN: usize = 7;
pub const LONG_LEN: usize = 20;
const SW_ID: u8 = 0x0A; // 4-bit software identifier in low nibble of function byte

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
    dev: HidDevice,
    pub vid: u16,
    pub pid: u16,
    pub product_string: String,
    pub path: String,
    pub device_index: u8,
    pub use_long_only: bool,
    pub proto_major: u8,
    pub proto_minor: u8,
    pub features: Vec<FeatureEntry>,
}

impl HidppTransport {
    /// Open a Logitech HID++ collection matching `usage_page` (e.g. `0xFF00` for mice/receivers,
    /// `0xFF43` for headsets) and probe for an active HID++ 2.0 target device index.
    pub fn open_matching(
        pid_filter: Option<u16>,
        usage_page: u16,
        preferred_usage: Option<u16>,
    ) -> Result<Self, HidppError> {
        let api = HidApi::new()?;
        let mut candidates: Vec<_> = api
            .device_list()
            .filter(|d| {
                d.vendor_id() == LOGITECH_VID
                    && d.usage_page() == usage_page
                    && pid_filter.map_or(true, |p| d.product_id() == p)
            })
            .collect();

        if candidates.is_empty() {
            return Err(HidppError::DeviceNotFound);
        }

        // Prefer long-report collection (usage 0x0002 on 0xFF00, or preferred_usage)
        candidates.sort_by_key(|d| {
            if let Some(u) = preferred_usage {
                if d.usage() == u { 0 } else { 1 }
            } else if d.usage() == 0x0002 {
                0
            } else {
                1
            }
        });

        let chosen = candidates[0];
        let path_cstr: CString = chosen.path().to_owned();
        let path = path_cstr.to_string_lossy().into_owned();
        let vid = chosen.vendor_id();
        let pid = chosen.product_id();
        let product_string = chosen
            .product_string()
            .unwrap_or("Logitech HID++ Device")
            .to_string();
        let use_long_only = chosen.usage() == 0x0002 || usage_page == 0xFF43;

        let dev = api.open_path(&path_cstr)?;

        let mut transport = Self {
            dev,
            vid,
            pid,
            product_string,
            path,
            device_index: 0xFF,
            use_long_only,
            proto_major: 0,
            proto_minor: 0,
            features: Vec::new(),
        };
        transport.drain();

        // Probe device indices: 0x01..=0x06 (wireless LIGHTSPEED/Unifying slots first), then 0xFF (wired/direct)
        let probe_order: &[u8] = if usage_page == 0xFF43 {
            &[0xFF, 0x01, 0x00]
        } else {
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0xFF]
        };

        let mut found_idx = None;
        for &idx in probe_order {
            transport.device_index = idx;
            // Wireless LIGHTSPEED mice in idle sleep can take up to ~1.2s to wake on the first ping.
            let timeout = if idx == 0x01 { 1400 } else { 250 };
            if let Ok((maj, min)) = transport.ping_protocol_version_timeout(timeout) {
                debug!(
                    "HID++ répondu sur {:04X}:{:04X} slot 0x{:02X} -> v{}.{}",
                    vid, pid, idx, maj, min
                );
                transport.proto_major = maj;
                transport.proto_minor = min;
                found_idx = Some(idx);
                break;
            }
        }

        if found_idx.is_none() {
            // Keep default 0xFF for legacy/headset direct commands
            transport.device_index = 0xFF;
            return Ok(transport);
        }

        // Enumerate full HID++ 2.0 feature table
        let _ = transport.enumerate_features();
        Ok(transport)
    }

    /// Enumerate ALL active Logitech HID++ 2.0 devices on a given `usage_page` (`0xFF00` or `0xFF43`),
    /// probing every physical HID interface AND every wireless receiver slot (`0x01..=0x06` + `0xFF`).
    pub fn open_all_matching(usage_page: u16, preferred_usage: Option<u16>) -> Vec<Self> {
        let Ok(api) = HidApi::new() else {
            return Vec::new();
        };

        // Group candidates by (vid, pid, interface_number, serial) and pick the best usage per physical interface
        let mut by_iface: Vec<(u16, u16, i32, String, u16, CString, String)> = Vec::new();
        for d in api.device_list() {
            if d.vendor_id() != LOGITECH_VID || d.usage_page() != usage_page {
                continue;
            }
            let vid = d.vendor_id();
            let pid = d.product_id();
            let iface = d.interface_number();
            let serial = d.serial_number().unwrap_or("").to_string();
            let usage = d.usage();
            let path_cstr = d.path().to_owned();
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
                .find(|e| e.0 == vid && e.1 == pid && e.2 == iface && e.3 == serial)
            {
                if is_better(usage) < is_better(existing.4) {
                    existing.4 = usage;
                    existing.5 = path_cstr;
                    existing.6 = product;
                }
            } else {
                by_iface.push((vid, pid, iface, serial, usage, path_cstr, product));
            }
        }

        let probe_order: &[u8] = if usage_page == 0xFF43 {
            &[0xFF, 0x01]
        } else {
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0xFF]
        };

        let mut results = Vec::new();
        for (vid, pid, _iface, _serial, usage, path_cstr, product) in by_iface {
            let path_str = path_cstr.to_string_lossy().into_owned();
            let use_long_only = usage == 0x0002 || usage_page == 0xFF43;

            let mut matched_slots = Vec::new();
            if let Ok(probe_dev) = api.open_path(&path_cstr) {
                let mut probe = Self {
                    dev: probe_dev,
                    vid,
                    pid,
                    product_string: product.clone(),
                    path: path_str.clone(),
                    device_index: 0xFF,
                    use_long_only,
                    proto_major: 0,
                    proto_minor: 0,
                    features: Vec::new(),
                };
                probe.drain();

                for &idx in probe_order {
                    probe.device_index = idx;
                    let timeout = if idx == 0x01 { 1200 } else { 160 };
                    if let Ok((maj, min)) = probe.ping_protocol_version_timeout(timeout) {
                        matched_slots.push((idx, maj, min));
                        // Direct/wired device answered on 0xFF -> stop probing receiver slots
                        if idx == 0xFF {
                            break;
                        }
                    }
                }
            }

            if matched_slots.is_empty() && usage_page == 0xFF43 {
                // Keep direct legacy fallback for wired headsets like PRO X if ping timed out
                if let Ok(dev) = api.open_path(&path_cstr) {
                    let t = Self {
                        dev,
                        vid,
                        pid,
                        product_string: product.clone(),
                        path: path_str.clone(),
                        device_index: 0xFF,
                        use_long_only,
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
                        dev,
                        vid,
                        pid,
                        product_string: product.clone(),
                        path: path_str.clone(),
                        device_index: idx,
                        use_long_only,
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

    /// Read HID++ 2.0 Device Type (`0x0005` fn `0x20`): 0=Keyboard, 3=Mouse, 8=Headset, etc.
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

    /// Low-level raw long HID++ write (used by HeadsetControl-style direct commands if needed).
    pub fn write_raw_long(&self, dev_idx: u8, payload: &[u8]) -> Result<(), HidppError> {
        let mut buf = [0u8; LONG_LEN];
        buf[0] = REPORT_ID_LONG;
        buf[1] = dev_idx;
        let n = payload.len().min(LONG_LEN - 2);
        buf[2..2 + n].copy_from_slice(&payload[..n]);
        self.dev.write(&buf)?;
        Ok(())
    }

    /// Send a HID++ 2.0 feature request and wait for the matching response.
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
        let use_long = self.use_long_only || params.len() > 3;
        let len = if use_long { LONG_LEN } else { SHORT_LEN };
        let mut tx = vec![0u8; len];
        tx[0] = if use_long { REPORT_ID_LONG } else { REPORT_ID_SHORT };
        tx[1] = self.device_index;
        tx[2] = feature_idx;
        let addr = (fn_id & 0xF0) | SW_ID;
        tx[3] = addr;
        // Never send a silently truncated command (e.g. a 16-band EQ would lose its last band).
        if params.len() > len - 4 {
            return Err(HidppError::Protocol(
                0x02,
                "Paramètres HID++ trop longs pour un rapport (16 octets max)",
            ));
        }
        let copy_n = params.len();
        tx[4..4 + copy_n].copy_from_slice(&params[..copy_n]);

        trace!("HID++ TX: {:02X?}", tx);
        self.dev.write(&tx)?;

        let mut rx = [0u8; 64];
        for _ in 0..12 {
            let n = self.dev.read_timeout(&mut rx, timeout_ms)?;
            if n == 0 {
                return Err(HidppError::Timeout);
            }
            trace!("HID++ RX ({n}B): {:02X?}", &rx[..n]);
            if n < 6 {
                continue;
            }
            let rep_id = rx[0];
            if rep_id != REPORT_ID_SHORT && rep_id != REPORT_ID_LONG {
                continue;
            }
            // Match device index (or 0xFF)
            if rx[1] != self.device_index && self.device_index != 0xFF {
                continue;
            }
            // HID++ 2.0 error response: sub_id == 0xFF, byte[3] == feature_idx, byte[4] == addr, byte[5] == err
            if rx[2] == 0xFF && rx[3] == feature_idx && rx[4] == addr {
                return Err(HidppError::from_hidpp_code(rx[5]));
            }
            // HID++ 1.0 error response: sub_id == 0x8F
            if rx[2] == 0x8F && rx[3] == feature_idx {
                return Err(HidppError::from_hidpp_code(rx[5]));
            }
            // Matching reply
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
        // Ask Root (0x00) for FeatureSet (0x0001)
        let fset = self.request(0x00, 0x00, &FEAT_FEATURE_SET.to_be_bytes())?;
        let fset_idx = fset.first().copied().unwrap_or(0);
        if fset_idx == 0 {
            return Ok(());
        }

        let count_resp = self.request(fset_idx, 0x00, &[])?;
        let count = count_resp.first().copied().unwrap_or(0) as usize;
        let total = count + 1; // includes root at 0

        let mut list = Vec::with_capacity(total);
        for i in 0..total.min(80) {
            if let Ok(r) = self.request(fset_idx, 0x10, &[i as u8]) {
                if r.len() >= 3 {
                    let id = u16::from_be_bytes([r[0], r[1]]);
                    let flags = r[2];
                    list.push(FeatureEntry {
                        index: i as u8,
                        id,
                        flags,
                        name: feature_name(id).to_string(),
                    });
                }
            }
        }
        self.features = list;
        Ok(())
    }

    /// Read device marketing name via HID++ 2.0 Feature `0x0005` (DEVICE_NAME).
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
            // A NUL terminator or a chunk that adds nothing ends the name: re-requesting the
            // same offset would loop forever.
            if hit_terminator || bytes.len() == before {
                break;
            }
        }
        let s = String::from_utf8_lossy(&bytes).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    /// Read firmware strings via HID++ 2.0 Feature `0x0003` (DEVICE_FW_VERSION).
    pub fn read_firmware_versions(&self) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(cnt_resp) = self.request_feature(FEAT_DEVICE_FW_VERSION, 0x00, &[]) else {
            return out;
        };
        let count = cnt_resp.first().copied().unwrap_or(0).min(8);
        for entity in 0..count {
            if let Ok(r) = self.request_feature(FEAT_DEVICE_FW_VERSION, 0x10, &[entity]) {
                if r.len() >= 8 {
                    let prefix = String::from_utf8_lossy(&r[1..4]).trim_end_matches('\0').to_string();
                    let rev = r[4];
                    let minor = r[5];
                    let build = u16::from_be_bytes([r[6], r[7]]);
                    if !prefix.is_empty() {
                        out.push(format!("{prefix} {rev:02X}.{minor:02X}.B{build:04X}"));
                    }
                }
            }
        }
        out
    }
}
