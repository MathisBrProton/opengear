//! Every device behind the shared model (`opengear_core::driver`), whatever its vendor.
//!
//! All hardware backends are registered in `crate::drivers::device_drivers` and stored in
//! `driver_devices`, driving devices uniformly without vendor branches in the lighting engine.

use super::backend::BackendEngine;
use opengear_core::PollingRate;
use opengear_core::driver::{
    Connection, Device, DeviceKind, Driver, Value, setting_ids, validate_setting,
};
use serde_json::{Value as Json, json};

/// Re-discover the devices of every registered driver (start-up, rescan).
pub(crate) fn discover_all(drivers: &[Box<dyn Driver>]) -> Vec<Box<dyn Device>> {
    drivers.iter().flat_map(|d| d.discover()).collect()
}

impl BackendEngine {
    pub(crate) fn device_mut(&mut self, id: &str) -> Option<&mut (dyn Device + 'static)> {
        self.driver_devices
            .iter_mut()
            .find(|d| d.info().id == id)
            .map(|d| d.as_mut())
    }

    pub(crate) fn typed_devices<T: 'static>(&self) -> Vec<&T> {
        self.driver_devices
            .iter()
            .filter_map(|d| d.as_any().and_then(|a| a.downcast_ref::<T>()))
            .collect()
    }

    pub(crate) fn typed_device_mut<T: 'static>(&mut self, idx: usize) -> Option<&mut T> {
        self.driver_devices
            .iter_mut()
            .filter_map(|d| d.as_any_mut().and_then(|a| a.downcast_mut::<T>()))
            .nth(idx)
    }

    /// Re-discover USB driver devices whose vendor matches a hotplug change.
    pub(crate) fn rediscover_drivers_for(&mut self, vendors: &[u16]) {
        let touched = self
            .drivers
            .iter()
            .any(|d| d.usb_vendor_ids().iter().any(|v| vendors.contains(v)));
        if !touched {
            return;
        }
        let mut rebuilt: Vec<Box<dyn Device>> = self
            .driver_devices
            .drain(..)
            .filter(|d| d.info().connection == Connection::Internal)
            .collect();
        for drv in &self.drivers {
            if !drv.usb_vendor_ids().is_empty() {
                rebuilt.extend(drv.discover());
            }
        }
        self.driver_devices = rebuilt;
    }

    pub(crate) fn device_list_json(&mut self) -> Json {
        let placements = self.config.device_placements.clone();
        let mut out = Vec::with_capacity(self.driver_devices.len());
        for d in &mut self.driver_devices {
            let info = d.info();
            let placement = placements.get(&info.id).cloned().unwrap_or_default();
            let status = d.status();
            let settings = d.settings();
            let lighting = d.lighting().map(|l| {
                json!({
                    "zones": l.zones(),
                    "hardware_effects": l.hardware_effects(),
                    "max_fps": l.max_fps(),
                    "placement": placement,
                })
            });
            out.push(json!({
                "info": info,
                "status": status,
                "settings": settings,
                "lighting": lighting,
            }));
        }
        Json::Array(out)
    }

    /// Validate and apply one setting, then let the app react to shared setting ids.
    /// Returns an optional message for the user.
    pub(crate) fn set_device_setting(
        &mut self,
        device_id: &str,
        setting_id: &str,
        value: &Value,
    ) -> Result<Option<String>, String> {
        let device = self
            .device_mut(device_id)
            .ok_or_else(|| format!("Appareil introuvable : {device_id}"))?;
        let value = validate_setting(device, setting_id, value).map_err(|e| e.to_string())?;
        device
            .apply_setting(setting_id, &value)
            .map_err(|e| e.to_string())?;
        let kind = device.info().kind;
        Ok(self.after_setting(kind, setting_id, &value))
    }

    /// App-wide consequences of shared settings.
    fn after_setting(
        &mut self,
        kind: DeviceKind,
        setting_id: &str,
        value: &Value,
    ) -> Option<String> {
        match setting_id {
            // The keyboard polling rate and brightness are part of the profile (re-applied when
            // switching profiles).
            setting_ids::POLLING_RATE if kind == DeviceKind::Keyboard => {
                let rate = value
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .and_then(PollingRate::from_hz)?;
                self.config.active_profile_mut().polling_rate = rate;
                self.persist_config();
                None
            }
            setting_ids::HW_BRIGHTNESS if kind == DeviceKind::Keyboard => {
                self.config.active_profile_mut().hw_brightness_pct =
                    value.as_f64().unwrap_or(100.0) as u8;
                self.persist_config();
                None
            }
            // Hardware SOCD and the software one must not both process the same keys.
            setting_ids::HW_SOCD if value.as_bool() == Some(true) => {
                if !self.config.active_profile_mut().socd.enabled {
                    return None;
                }
                self.config.active_profile_mut().socd.enabled = false;
                self.sync_input_rules();
                self.persist_config();
                Some("SOCD logiciel désactivé pour éviter un double traitement".into())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opengear_core::driver::{Connection, DeviceInfo};

    struct FakeDriver;
    struct FakeDevice;

    impl Device for FakeDevice {
        fn info(&self) -> DeviceInfo {
            DeviceInfo {
                id: "fake:1".into(),
                vendor: "Fake".into(),
                model: "Fake".into(),
                kind: DeviceKind::Mouse,
                connection: Connection::Usb,
                firmware: None,
            }
        }
    }

    impl Driver for FakeDriver {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn usb_vendor_ids(&self) -> &'static [u16] {
            &[0x1234]
        }
        fn discover(&self) -> Vec<Box<dyn Device>> {
            vec![Box::new(FakeDevice), Box::new(FakeDevice)]
        }
    }

    #[test]
    fn registered_drivers_are_all_discovered() {
        let drivers: Vec<Box<dyn Driver>> = vec![Box::new(FakeDriver), Box::new(FakeDriver)];
        assert_eq!(discover_all(&drivers).len(), 4);
    }
}
