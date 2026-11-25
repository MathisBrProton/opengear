use crate::proto::*;
use hidapi::{HidApi, HidDevice};
use std::ffi::CString;
use std::time::Duration;
use tracing::{debug, trace, warn};

const READ_TIMEOUT_MS: i32 = 150;
const SHORT_TIMEOUT_MS: i32 = 15;
const EXCHANGE_DEADLINE_MS: u64 = 300;
const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;

/// Abstraction over raw HID device I/O so BragiTransport can be unit-tested with mock transports.
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

/// Locate the Bragi reply to `expected_cmd` in a raw input report: `[0x00|magic, cmd, err, ...]`,
/// or the same prefixed by a `0x00` report ID (`[0x00, magic, cmd, err, ...]`).
/// Validates the magic byte when unwrapping a Report ID prefix so an error response from an
/// unrelated command (e.g. `err == expected_cmd`) is never misinterpreted as a match.
pub(crate) fn match_reply(rx: &[u8], expected_magic: u8, expected_cmd: u8) -> Option<&[u8]> {
    if rx.len() >= 3 && (rx[0] == 0x00 || rx[0] == expected_magic) && rx[1] == expected_cmd {
        return Some(rx);
    }
    if rx.len() >= 4
        && rx[0] == 0x00
        && (rx[1] == expected_magic || (rx[1] & 0xF8) == BRAGI_MAGIC)
        && rx[2] == expected_cmd
    {
        return Some(&rx[1..]);
    }
    None
}

#[derive(Debug, Clone)]
pub struct HidCapsInfo {
    pub out_report_len: usize,
    pub in_report_len: usize,
}

#[derive(Debug, Clone)]
pub struct BragiHidCandidate {
    pub vid: u16,
    pub pid: u16,
    pub product_name: String,
    pub serial: String,
    pub path: String,
    pub path_cstr: CString,
}

#[cfg(windows)]
fn query_windows_hid_caps(path: &str) -> Option<HidCapsInfo> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Devices::HumanInterfaceDevice::{
        HIDP_CAPS, HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetCaps,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return None;
        }
        let mut preparsed = 0isize;
        let ok = HidD_GetPreparsedData(handle, &mut preparsed);
        if ok == 0 || preparsed == 0 {
            CloseHandle(handle);
            return None;
        }
        let mut caps: HIDP_CAPS = std::mem::zeroed();
        let status = HidP_GetCaps(preparsed, &mut caps);
        HidD_FreePreparsedData(preparsed);
        CloseHandle(handle);

        if status == HIDP_STATUS_SUCCESS {
            Some(HidCapsInfo {
                out_report_len: caps.OutputReportByteLength as usize,
                in_report_len: caps.InputReportByteLength as usize,
            })
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
fn query_windows_hid_caps(_path: &str) -> Option<HidCapsInfo> {
    None
}

pub struct BragiTransport {
    dev: Box<dyn HidIo>,
    pub vid: u16,
    pub pid: u16,
    /// `0` = host / wired device (`0x08`); `1..=7` = wireless Slipstream subdevice (`0x08 | id`).
    pub subdevice_id: u8,
    pub product_name: String,
    pub serial: String,
    pub path: String,
    /// Full HID write report length including Report ID byte 0 (usually 65 or 1025).
    out_report_len: usize,
    /// Full HID read report length including Report ID byte 0 (usually 65 or 1025).
    in_report_len: usize,
    tx_buf: Vec<u8>,
    rx_buf: Vec<u8>,
}

impl BragiTransport {
    #[cfg(test)]
    pub(crate) fn from_io(
        dev: Box<dyn HidIo>,
        subdevice_id: u8,
        out_report_len: usize,
        in_report_len: usize,
    ) -> Self {
        let out_len = out_report_len.max(16);
        let in_len = in_report_len.max(16);
        Self {
            dev,
            vid: CORSAIR_VID,
            pid: PID_K70_RGB_TKL,
            subdevice_id: subdevice_id & 0x07,
            product_name: "Mock Corsair".into(),
            serial: "MOCK".into(),
            path: "mock://0".into(),
            out_report_len: out_len,
            in_report_len: in_len,
            tx_buf: vec![0u8; out_len],
            rx_buf: vec![0u8; in_len],
        }
    }

    pub fn out_report_len(&self) -> usize {
        self.out_report_len
    }

    pub fn in_report_len(&self) -> usize {
        self.in_report_len
    }

    /// Enumerate ALL Corsair Bragi command HID interfaces (`VID 1B1C`, `UsagePage 0xFF42`, `Usage 0x0001`).
    pub fn enumerate_candidates() -> Result<Vec<BragiHidCandidate>, BragiError> {
        let api = HidApi::new()?;
        let mut out = Vec::new();
        for d in api.device_list() {
            if d.vendor_id() == CORSAIR_VID
                && d.usage_page() == BRAGI_USAGE_PAGE
                && d.usage() == BRAGI_USAGE_CMD
            {
                let path_cstr = d.path().to_owned();
                let path = path_cstr.to_string_lossy().into_owned();
                if out.iter().any(|c: &BragiHidCandidate| c.path == path) {
                    continue;
                }
                out.push(BragiHidCandidate {
                    vid: d.vendor_id(),
                    pid: d.product_id(),
                    product_name: d
                        .product_string()
                        .unwrap_or("Corsair Bragi Device")
                        .to_string(),
                    serial: d.serial_number().unwrap_or("").to_string(),
                    path,
                    path_cstr,
                });
            }
        }
        Ok(out)
    }

    /// Open a specific Corsair Bragi candidate for a given subdevice slot (`0` = direct/wired, `1..=7` = Slipstream subdevice).
    pub fn open_candidate(
        cand: &BragiHidCandidate,
        subdevice_id: u8,
    ) -> Result<Self, BragiError> {
        let api = HidApi::new()?;
        let caps = query_windows_hid_caps(&cand.path);
        let (out_report_len, in_report_len) = match caps {
            Some(c) if c.out_report_len >= 65 => {
                debug!(
                    "Detected HID report lengths for {:04X}:{:04X}: out={}, in={}",
                    cand.vid, cand.pid, c.out_report_len, c.in_report_len
                );
                (c.out_report_len, c.in_report_len.max(65))
            }
            _ => {
                warn!("Could not query HIDP_CAPS, defaulting to 65-byte reports");
                (65, 65)
            }
        };

        let dev = api.open_path(&cand.path_cstr)?;

        let mut transport = Self {
            dev: Box::new(HidApiIo(dev)),
            vid: cand.vid,
            pid: cand.pid,
            subdevice_id: subdevice_id & 0x07,
            product_name: cand.product_name.clone(),
            serial: cand.serial.clone(),
            path: cand.path.clone(),
            out_report_len,
            in_report_len,
            tx_buf: vec![0u8; out_report_len],
            rx_buf: vec![0u8; in_report_len],
        };
        transport.drain_input();
        Ok(transport)
    }

    pub fn open_first(target_pid: Option<u16>) -> Result<Self, BragiError> {
        let cands = Self::enumerate_candidates()?;
        let chosen = cands
            .into_iter()
            .find(|c| target_pid.map_or(true, |p| c.pid == p))
            .ok_or(BragiError::DeviceNotFound)?;
        Self::open_candidate(&chosen, 0)
    }

    #[inline]
    pub fn magic(&self) -> u8 {
        BRAGI_MAGIC | (self.subdevice_id & 0x07)
    }

    pub fn payload_len(&self) -> usize {
        self.out_report_len.saturating_sub(1)
    }

    pub fn drain_input(&mut self) {
        for _ in 0..8 {
            match self.dev.read_timeout(&mut self.rx_buf, 2) {
                Ok(n) if n > 0 => continue,
                _ => break,
            }
        }
    }

    pub fn drain_pending(&mut self) {
        for _ in 0..64 {
            match self.dev.read_timeout(&mut self.rx_buf, 0) {
                Ok(n) if n > 0 => continue,
                _ => break,
            }
        }
    }

    /// Send a Bragi packet and wait for its matching response.
    pub fn exchange(&mut self, payload: &[u8]) -> Result<Vec<u8>, BragiError> {
        let copy_n = payload.len().min(self.payload_len());
        self.tx_buf.fill(0);
        self.tx_buf[0] = 0x00;
        self.tx_buf[1..1 + copy_n].copy_from_slice(&payload[..copy_n]);

        self.drain_pending();
        trace!(
            "TX ({} bytes): {:02X?}",
            self.tx_buf.len(),
            &self.tx_buf[..self.tx_buf.len().min(16)]
        );
        self.dev.write(&self.tx_buf)?;

        let expected_magic = self.magic();
        let expected_cmd = payload.get(1).copied().unwrap_or(0);
        let deadline = std::time::Instant::now() + Duration::from_millis(EXCHANGE_DEADLINE_MS);

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(BragiError::Timeout);
            }
            let wait = (remaining.as_millis() as i32).clamp(1, READ_TIMEOUT_MS);
            let n = self.dev.read_timeout(&mut self.rx_buf, wait)?;
            if n == 0 {
                continue;
            }
            let n = n.min(self.rx_buf.len());
            trace!("RX ({} bytes): {:02X?}", n, &self.rx_buf[..n.min(16)]);
            if let Some(reply) = match_reply(&self.rx_buf[..n], expected_magic, expected_cmd) {
                let mut out = reply.to_vec();
                out.resize(self.in_report_len.max(out.len()), 0);
                return Ok(out);
            }
            trace!("RX ignoré (commande 0x{:02X} attendue)", expected_cmd);
        }
    }

    /// Fire-and-forget write for high-speed live RGB streaming.
    pub fn write_chunk(&mut self, payload: &[u8], wait_reply: bool) -> Result<(), BragiError> {
        let copy_n = payload.len().min(self.payload_len());
        self.tx_buf.fill(0);
        self.tx_buf[0] = 0x00;
        self.tx_buf[1..1 + copy_n].copy_from_slice(&payload[..copy_n]);

        self.dev.write(&self.tx_buf)?;

        if wait_reply {
            let expected_magic = self.magic();
            let expected_cmd = payload.get(1).copied().unwrap_or(0);
            let n = self.dev.read_timeout(&mut self.rx_buf, SHORT_TIMEOUT_MS)?;
            if n == 0 {
                std::thread::sleep(Duration::from_millis(2));
            } else {
                let n = n.min(self.rx_buf.len());
                if let Some(reply) = match_reply(&self.rx_buf[..n], expected_magic, expected_cmd) {
                    let err = reply[2];
                    if err != ERR_OK {
                        return Err(BragiError::from_code(err));
                    }
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    /// Fast fire-and-forget handle write (used for real-time RGB frames).
    pub fn write_handle_data(&mut self, handle: u8, data: &[u8]) -> Result<(), BragiError> {
        self.drain_pending();
        let max_payload = self.payload_len();
        if max_payload < 8 {
            return Err(BragiError::Protocol(0x01, "Report HID trop court"));
        }
        let data_len = data.len() as u32;
        let len_bytes = data_len.to_le_bytes();

        let first_hdr = 7usize;
        let first_cap = max_payload - first_hdr;
        let first_chunk = data.len().min(first_cap);

        let mut pkt = vec![0u8; max_payload];
        pkt[0] = self.magic();
        pkt[1] = CMD_WRITE_DATA;
        pkt[2] = handle;
        pkt[3..7].copy_from_slice(&len_bytes);
        pkt[first_hdr..first_hdr + first_chunk].copy_from_slice(&data[..first_chunk]);
        self.write_chunk(&pkt, true)?;

        let mut offset = first_chunk;
        let cont_hdr = 3usize;
        let cont_cap = max_payload - cont_hdr;
        while offset < data.len() {
            let take = (data.len() - offset).min(cont_cap);
            pkt.fill(0);
            pkt[0] = self.magic();
            pkt[1] = CMD_CONTINUE_WRITE;
            pkt[2] = handle;
            pkt[cont_hdr..cont_hdr + take].copy_from_slice(&data[offset..offset + take]);
            self.write_chunk(&pkt, false)?;
            offset += take;
        }
        Ok(())
    }

    /// Reliable ACK-verified handle write (MUST be used for onboard flash / HWP writes).
    pub fn write_handle_data_reliable(&mut self, handle: u8, data: &[u8]) -> Result<(), BragiError> {
        let max_payload = self.payload_len();
        if max_payload < 8 {
            return Err(BragiError::Protocol(0x01, "Report HID trop court"));
        }
        let data_len = data.len() as u32;
        let len_bytes = data_len.to_le_bytes();

        let first_hdr = 7usize;
        let first_cap = max_payload - first_hdr;
        let first_chunk = data.len().min(first_cap);

        let mut pkt = vec![0u8; max_payload];
        pkt[0] = self.magic();
        pkt[1] = CMD_WRITE_DATA;
        pkt[2] = handle;
        pkt[3..7].copy_from_slice(&len_bytes);
        pkt[first_hdr..first_hdr + first_chunk].copy_from_slice(&data[..first_chunk]);

        let rx = self.exchange(&pkt)?;
        if rx[2] != ERR_OK {
            return Err(BragiError::from_code(rx[2]));
        }

        let mut offset = first_chunk;
        let cont_hdr = 3usize;
        let cont_cap = max_payload - cont_hdr;
        while offset < data.len() {
            let take = (data.len() - offset).min(cont_cap);
            pkt.fill(0);
            pkt[0] = self.magic();
            pkt[1] = CMD_CONTINUE_WRITE;
            pkt[2] = handle;
            pkt[cont_hdr..cont_hdr + take].copy_from_slice(&data[offset..offset + take]);
            let rx_c = self.exchange(&pkt)?;
            if rx_c[2] != ERR_OK {
                return Err(BragiError::from_code(rx_c[2]));
            }
            offset += take;
        }
        Ok(())
    }

    pub fn get_property(&mut self, prop: u8) -> Result<u32, BragiError> {
        self.get_property_ext(prop as u16)
    }

    pub fn get_property_ext(&mut self, prop: u16) -> Result<u32, BragiError> {
        let p = prop.to_le_bytes();
        let rx = self.exchange(&[self.magic(), CMD_GET, p[0], p[1]])?;
        let err = rx[2];
        if err == ERR_NOT_SUPPORTED {
            return Err(BragiError::UnsupportedProperty(prop));
        }
        if err != ERR_OK {
            return Err(BragiError::from_code(err));
        }
        let val = u32::from_le_bytes([
            rx.get(3).copied().unwrap_or(0),
            rx.get(4).copied().unwrap_or(0),
            rx.get(5).copied().unwrap_or(0),
            rx.get(6).copied().unwrap_or(0),
        ]);
        Ok(val)
    }

    pub fn set_property(&mut self, prop: u8, val: u16) -> Result<(), BragiError> {
        self.set_property_ext(prop as u16, val as u32)
    }

    pub fn set_property_u32(&mut self, prop: u8, val: u32) -> Result<(), BragiError> {
        self.set_property_ext(prop as u16, val)
    }

    pub fn set_property_ext(&mut self, prop: u16, val: u32) -> Result<(), BragiError> {
        let p = prop.to_le_bytes();
        let b = val.to_le_bytes();
        let rx = self.exchange(&[self.magic(), CMD_SET, p[0], p[1], b[0], b[1], b[2], b[3]])?;
        let err = rx[2];
        if err == ERR_NOT_SUPPORTED {
            return Err(BragiError::UnsupportedProperty(prop));
        }
        if err != ERR_OK {
            return Err(BragiError::from_code(err));
        }
        Ok(())
    }

    pub fn open_handle(&mut self, handle: u8, resource: u16) -> Result<u8, BragiError> {
        let lo = (resource & 0xFF) as u8;
        let hi = (resource >> 8) as u8;
        let rx = self.exchange(&[self.magic(), CMD_OPEN_HANDLE, handle, lo, hi])?;
        let err = rx[2];
        if err == ERR_FAILED_OR_OPEN {
            let _ = self.close_handle(handle);
            let rx2 = self.exchange(&[self.magic(), CMD_OPEN_HANDLE, handle, lo, hi])?;
            return Ok(rx2[2]);
        }
        Ok(err)
    }

    pub fn close_handle(&mut self, handle: u8) -> Result<(), BragiError> {
        let _ = self.exchange(&[self.magic(), CMD_CLOSE_HANDLE, 0x01, handle])?;
        Ok(())
    }

    pub fn send_keepalive(&mut self) -> Result<(), BragiError> {
        let _ = self.exchange(&[self.magic(), CMD_KEEPALIVE])?;
        Ok(())
    }

    pub fn describe_handle(&mut self, handle: u8) -> Result<usize, BragiError> {
        let rx = self.exchange(&[self.magic(), CMD_PROBE_HANDLE, handle])?;
        debug!("DescribeBuffer(h={handle}) RX: {:02X?}", &rx[..rx.len().min(16)]);
        let err = rx[2];
        if err != ERR_OK {
            return Err(BragiError::from_code(err));
        }
        let sz = u32::from_le_bytes([
            rx.get(5).copied().unwrap_or(0),
            rx.get(6).copied().unwrap_or(0),
            rx.get(7).copied().unwrap_or(0),
            rx.get(8).copied().unwrap_or(0),
        ]) as usize;
        Ok(sz)
    }

    pub fn read_handle_data(&mut self, handle: u8, expected_len: usize) -> Result<Vec<u8>, BragiError> {
        let mut out = Vec::with_capacity(expected_len);
        let max_iters = (expected_len / 32).max(1) + 8;
        for iter in 0..max_iters {
            if expected_len > 0 && out.len() >= expected_len {
                break;
            }
            let rx = self.exchange(&[self.magic(), CMD_READ_DATA, handle])?;
            if iter == 0 {
                debug!(
                    "ReadBuffer(h={handle}) RX[0..24]: {:02X?}",
                    &rx[..rx.len().min(24)]
                );
            }
            let err = rx[2];
            if err != ERR_OK {
                return Err(BragiError::from_code(err));
            }
            let max_chunk = self.in_report_len.saturating_sub(1).saturating_sub(3);
            let avail = rx.len().saturating_sub(3).min(max_chunk);
            if avail == 0 {
                break;
            }
            let take = if expected_len > 0 {
                (expected_len - out.len()).min(avail)
            } else {
                avail
            };
            out.extend_from_slice(&rx[3..3 + take]);
            if expected_len == 0 {
                break;
            }
        }
        if expected_len > 0 && out.len() < expected_len {
            return Err(BragiError::Protocol(0x04, "Lecture Bragi tronquée"));
        }
        Ok(out)
    }

    pub fn read_file(&mut self, file_id: u16) -> Result<Vec<u8>, BragiError> {
        let handle = 0x00u8;
        let _ = self.close_handle(handle);
        let code = self.open_handle(handle, file_id)?;
        if code != ERR_OK {
            return Err(BragiError::from_code(code));
        }
        let res = (|| -> Result<Vec<u8>, BragiError> {
            let sz = self.describe_handle(handle)?;
            if sz == 0 {
                return Ok(Vec::new());
            }
            if sz > 65536 {
                return Err(BragiError::Protocol(0x01, "Taille fichier Bragi anormale"));
            }
            self.read_handle_data(handle, sz)
        })();
        let _ = self.close_handle(handle);
        res
    }

    pub fn create_file(&mut self, file_id: u16) -> Result<(), BragiError> {
        let b = file_id.to_le_bytes();
        let rx = self.exchange(&[self.magic(), CMD_CREATE_FILE, b[0], b[1]])?;
        let err = rx[2];
        if err != ERR_OK {
            return Err(BragiError::from_code(err));
        }
        Ok(())
    }

    pub fn remove_file(&mut self, file_id: u16) -> Result<(), BragiError> {
        let b = file_id.to_le_bytes();
        let rx = self.exchange(&[self.magic(), CMD_REMOVE_FILE, b[0], b[1]])?;
        let err = rx[2];
        if err != ERR_OK {
            return Err(BragiError::from_code(err));
        }
        Ok(())
    }

    /// Write raw bytes to an existing Bragi flash file by `FileId` with full ACK verification.
    pub fn write_file(&mut self, file_id: u16, data: &[u8]) -> Result<(), BragiError> {
        let handle = 0x00u8;
        let _ = self.close_handle(handle);
        let code = self.open_handle(handle, file_id)?;
        if code != ERR_OK {
            return Err(BragiError::from_code(code));
        }
        let res = self.write_handle_data_reliable(handle, data);
        let _ = self.close_handle(handle);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct MockCorsairIo {
        writes: Mutex<Vec<Vec<u8>>>,
        reads: Mutex<VecDeque<Vec<u8>>>,
    }

    impl MockCorsairIo {
        fn new(reads: Vec<Vec<u8>>) -> Self {
            Self {
                writes: Mutex::new(Vec::new()),
                reads: Mutex::new(reads.into()),
            }
        }
    }

    impl HidIo for MockCorsairIo {
        fn write(&self, data: &[u8]) -> Result<usize, hidapi::HidError> {
            self.writes.lock().unwrap().push(data.to_vec());
            Ok(data.len())
        }

        fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, hidapi::HidError> {
            if timeout_ms <= 0 {
                return Ok(0);
            }
            let Some(pkt) = self.reads.lock().unwrap().pop_front() else {
                return Ok(0);
            };
            let n = pkt.len().min(buf.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            Ok(n)
        }
    }

    #[test]
    fn reply_matches_command_byte() {
        let rx = [0x00, 0x09, 0x00, 0x01, 0x02];
        assert_eq!(match_reply(&rx, 0x08, 0x09), Some(&rx[..]));
    }

    #[test]
    fn reply_with_report_id_prefix_is_unwrapped() {
        let rx = [0x00, 0x08, 0x02, 0x00, 0x55];
        assert_eq!(match_reply(&rx, 0x08, 0x02), Some(&rx[1..]));
    }

    #[test]
    fn foreign_reports_and_error_echo_do_not_false_match() {
        assert_eq!(match_reply(&[0x00, 0x02, 0x00, 0x10], 0x08, 0x0D), None);
        assert_eq!(match_reply(&[0x00, 0x0D], 0x08, 0x0D), None);
        // Reply to CMD_GET (0x02) carrying ERR_NOT_SUPPORTED (0x05) must NOT match CMD_CLOSE_HANDLE (0x05)
        assert_eq!(match_reply(&[0x00, 0x02, 0x05, 0x00], 0x08, 0x05), None);
    }

    #[test]
    fn reliable_write_verifies_every_continuation_ack() {
        // 30 bytes payload on 16-byte report (15 payload: first chunk takes 8 bytes, 2 continuations needed)
        let io = MockCorsairIo::new(vec![
            vec![0x00, 0x08, CMD_WRITE_DATA, ERR_OK],
            vec![0x00, 0x08, CMD_CONTINUE_WRITE, ERR_OK],
            vec![0x00, 0x08, CMD_CONTINUE_WRITE, ERR_INVALID_VALUE],
        ]);
        let mut t = BragiTransport::from_io(Box::new(io), 0, 16, 16);
        let err = t.write_handle_data_reliable(0, &[0xAA; 30]).unwrap_err();
        assert!(matches!(err, BragiError::Protocol(ERR_INVALID_VALUE, _)));
    }

    #[test]
    fn read_handle_data_fails_on_mid_stream_truncation() {
        let io = MockCorsairIo::new(vec![
            vec![0x00, 0x08, CMD_READ_DATA, ERR_OK, 1, 2, 3, 4],
            vec![0x00, 0x08, CMD_READ_DATA, ERR_FAILED_OR_OPEN],
        ]);
        let mut t = BragiTransport::from_io(Box::new(io), 0, 16, 16);
        assert!(t.read_handle_data(0, 64).is_err());
    }
}
