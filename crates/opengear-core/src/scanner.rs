use crate::device::{DetectedDevice, DeviceVendor};
use anyhow::Result;
use hidapi::HidApi;

fn classify_device(vid: u16, pid: u16, usage_page: u16) -> (&'static str, &'static str) {
    match (vid, pid) {
        (0x1B1C, 0x1B73) => (
            "Corsair K70 RGB TKL Champion Series",
            if usage_page == 0xFF42 {
                "Supporté (Bragi v2)"
            } else {
                "Interface HID standard"
            },
        ),
        (0x0B05, 0x19AF) => (
            "ASUS AURA LED Controller (B650-A)",
            if usage_page == 0x0059 {
                "Prêt pour driver LampArray (0x59)"
            } else if usage_page == 0xFF72 {
                "Prêt pour driver ASUS Aura"
            } else {
                "Interface ASUS"
            },
        ),
        (0x0B05, 0x1A52) => ("ASUS Realtek USB Audio", "Audio / HID fournisseur"),
        (0x046D, 0xC547) => (
            "Logitech LIGHTSPEED Receiver",
            if usage_page == 0xFF00 {
                "Prêt pour driver HID++ 2.0"
            } else {
                "Interface souris/clavier"
            },
        ),
        (0x046D, 0x0AAA) => (
            "Logitech PRO X Gaming Headset",
            if usage_page == 0xFF43 || usage_page == 0xFF99 {
                "Prêt pour driver Audio/EQ HID"
            } else {
                "Interface audio/headset"
            },
        ),
        (0x187C, 0x1014) => (
            "Alienware AW2725DF Hub / AlienFX",
            "Prêt pour driver AlienFX",
        ),
        (0x054C, 0x0CE6) => (
            "Sony DualSense Wireless Controller",
            "Prêt pour driver DualSense",
        ),
        _ => ("", "Non mappé"),
    }
}

/// Enumerate all connected HID interfaces belonging to known gaming/hardware vendors.
pub fn scan_hid_devices(include_all: bool) -> Result<Vec<DetectedDevice>> {
    let api = HidApi::new()?;
    let mut results = Vec::new();

    for dev in api.device_list() {
        let vendor = DeviceVendor::from_vid(dev.vendor_id());
        if !include_all && matches!(vendor, DeviceVendor::Unknown(_)) {
            continue;
        }

        let (known_name, status) =
            classify_device(dev.vendor_id(), dev.product_id(), dev.usage_page());
        let product_name = dev
            .product_string()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if !known_name.is_empty() {
                    known_name.to_string()
                } else {
                    format!("USB {:04X}:{:04X}", dev.vendor_id(), dev.product_id())
                }
            });

        results.push(DetectedDevice {
            vendor,
            vid: dev.vendor_id(),
            pid: dev.product_id(),
            product_name,
            manufacturer: dev.manufacturer_string().unwrap_or("").to_string(),
            serial: dev.serial_number().unwrap_or("").to_string(),
            interface_number: dev.interface_number(),
            usage_page: dev.usage_page(),
            usage: dev.usage(),
            path: dev.path().to_string_lossy().into_owned(),
            driver_status: status.to_string(),
        });
    }

    results.sort_by(|a, b| {
        (a.vid, a.pid, a.interface_number, a.usage_page, a.usage).cmp(&(
            b.vid,
            b.pid,
            b.interface_number,
            b.usage_page,
            b.usage,
        ))
    });
    Ok(results)
}
