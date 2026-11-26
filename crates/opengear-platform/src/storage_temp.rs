//! NVMe / storage drive temperature polling via `IOCTL_STORAGE_QUERY_PROPERTY`.
//!
//! Uses `StorageDeviceTemperatureProperty` (`PropertyId = 53`), which works without
//! administrator privileges (`desired_access = 0`) on Windows 10+ and returns all
//! reported drive temperature sensors in Celsius.

/// `IOCTL_STORAGE_QUERY_PROPERTY` (`CTL_CODE(IOCTL_STORAGE_BASE, 0x0500, METHOD_BUFFERED, FILE_ANY_ACCESS)`).
#[cfg(windows)]
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
#[cfg(windows)]
const STORAGE_DEVICE_TEMPERATURE_PROPERTY: u32 = 53;
#[cfg(windows)]
const PROPERTY_STANDARD_QUERY: u32 = 0;

/// Header size of `STORAGE_TEMPERATURE_DATA_DESCRIPTOR` before `TemperatureInfo[]`:
/// `Version (4) + Size (4) + CriticalTemperature (2) + WarningTemperature (2) +
///  InfoCount (2) + Reserved0 (2) + Reserved1 (8) = 24 bytes`.
const DESCRIPTOR_HEADER_LEN: usize = 24;
/// Byte size of one `STORAGE_TEMPERATURE_INFO` entry (`16` bytes).
const TEMP_INFO_STRIDE: usize = 16;

/// Parses a raw `STORAGE_TEMPERATURE_DATA_DESCRIPTOR` buffer and returns the
/// hottest plausible drive sensor reading in °C (clamped to `-20.0..=125.0` °C).
pub fn parse_storage_temperature_descriptor(buf: &[u8]) -> Option<f32> {
    if buf.len() < DESCRIPTOR_HEADER_LEN {
        return None;
    }
    let info_count = u16::from_le_bytes([buf[12], buf[13]]) as usize;
    if info_count == 0 {
        return None;
    }
    let max_entries = ((buf.len() - DESCRIPTOR_HEADER_LEN) / TEMP_INFO_STRIDE).min(info_count);
    let mut best: Option<f32> = None;
    for idx in 0..max_entries {
        let off = DESCRIPTOR_HEADER_LEN + idx * TEMP_INFO_STRIDE;
        let temp_c = i16::from_le_bytes([buf[off + 2], buf[off + 3]]) as f32;
        if (-20.0..=125.0).contains(&temp_c) && temp_c != 0.0 {
            best = Some(best.map_or(temp_c, |cur| cur.max(temp_c)));
        }
    }
    best
}

/// Polls `\\.\PhysicalDrive0..=3` for the highest NVMe / SATA temperature via
/// `IOCTL_STORAGE_QUERY_PROPERTY(StorageDeviceTemperatureProperty)`.
#[cfg(windows)]
pub fn read_nvme_max_temp_c() -> Option<f32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    #[repr(C)]
    struct StoragePropertyQuery {
        property_id: u32,
        query_type: u32,
        additional_parameters: [u8; 4],
    }

    let query = StoragePropertyQuery {
        property_id: STORAGE_DEVICE_TEMPERATURE_PROPERTY,
        query_type: PROPERTY_STANDARD_QUERY,
        additional_parameters: [0; 4],
    };

    let mut max_temp: Option<f32> = None;
    for drive_idx in 0..4u8 {
        let wide: Vec<u16> = format!("\\\\.\\PhysicalDrive{drive_idx}\0")
            .encode_utf16()
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            continue;
        }
        let mut out_buf = [0u8; DESCRIPTOR_HEADER_LEN + TEMP_INFO_STRIDE * 8];
        let mut bytes_returned: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &query as *const _ as *const _,
                std::mem::size_of::<StoragePropertyQuery>() as u32,
                out_buf.as_mut_ptr() as *mut _,
                out_buf.len() as u32,
                &mut bytes_returned,
                std::ptr::null_mut(),
            )
        };
        unsafe {
            CloseHandle(handle);
        }
        if ok != 0 {
            let valid_len = (bytes_returned as usize).min(out_buf.len());
            if let Some(t) = parse_storage_temperature_descriptor(&out_buf[..valid_len]) {
                max_temp = Some(max_temp.map_or(t, |cur| cur.max(t)));
            }
        }
    }
    max_temp
}

#[cfg(not(windows))]
pub fn read_nvme_max_temp_c() -> Option<f32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_storage_temperature_entries() {
        let mut buf = vec![0u8; DESCRIPTOR_HEADER_LEN + TEMP_INFO_STRIDE * 3];
        let total_len = buf.len() as u32;
        // Version = 1, Size = total
        buf[0..4].copy_from_slice(&1u32.to_le_bytes());
        buf[4..8].copy_from_slice(&total_len.to_le_bytes());
        // InfoCount = 3
        buf[12..14].copy_from_slice(&3u16.to_le_bytes());
        // Entry 0: 38 C
        buf[DESCRIPTOR_HEADER_LEN + 2..DESCRIPTOR_HEADER_LEN + 4]
            .copy_from_slice(&38i16.to_le_bytes());
        // Entry 1: 52 C (controller peak)
        buf[DESCRIPTOR_HEADER_LEN + TEMP_INFO_STRIDE + 2
            ..DESCRIPTOR_HEADER_LEN + TEMP_INFO_STRIDE + 4]
            .copy_from_slice(&52i16.to_le_bytes());
        // Entry 2: invalid sentinel (32767 C -> ignored)
        buf[DESCRIPTOR_HEADER_LEN + 2 * TEMP_INFO_STRIDE + 2
            ..DESCRIPTOR_HEADER_LEN + 2 * TEMP_INFO_STRIDE + 4]
            .copy_from_slice(&i16::MAX.to_le_bytes());

        assert_eq!(parse_storage_temperature_descriptor(&buf), Some(52.0));
    }

    #[test]
    fn rejects_empty_or_truncated_descriptor() {
        assert_eq!(parse_storage_temperature_descriptor(&[0u8; 10]), None);
        let buf = vec![0u8; DESCRIPTOR_HEADER_LEN];
        assert_eq!(parse_storage_temperature_descriptor(&buf), None);
    }
}
