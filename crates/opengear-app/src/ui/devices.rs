//! Every device behind the shared model (`opengear_core::driver`), whatever its vendor.
//!
//! All hardware backends are registered in `crate::drivers::device_drivers` and stored in
//! `driver_devices`, driving devices uniformly without vendor branches in the lighting engine.

use super::backend::BackendEngine;
use opengear_core::PollingRate;
use opengear_core::driver::{
    Connection, Device, DeviceKind, Driver, Value, setting_ids, validate_setting,
};
use opengear_core::fans::{Cooling, FanTarget, clamp_target};
use serde_json::{Value as Json, json};

/// `{ fans, overridden }` of a device's `Cooling`, as sent to the UI.
pub(crate) fn cooling_json(c: &dyn Cooling) -> Json {
    json!({ "fans": c.fans(), "overridden": c.overridden() })
}

/// Clamp `target` to the floor of the addressed fan (`None` = the strictest floor of the device).
pub(crate) fn clamp_for(
    c: &dyn Cooling,
    fan: Option<&str>,
    target: FanTarget,
) -> Result<FanTarget, String> {
    let fans = c.fans();
    match fan {
        Some(id) => fans
            .iter()
            .find(|f| f.id == id)
            .map(|f| clamp_target(f, target))
            .ok_or_else(|| format!("Ventilateur introuvable : {id}")),
        None => Ok(fans
            .iter()
            .map(|f| clamp_target(f, target))
            .max_by_key(|t| match t {
                FanTarget::Auto => 0,
                FanTarget::Fixed(p) => u16::from(*p) + 1,
            })
            .unwrap_or(target)),
    }
}

/// Re-discover the devices of every registered driver (start-up, rescan).
/// Discover the devices of every registered driver; experimental drivers only when the user
/// enabled them (`enabled_experimental` = `AppConfig::experimental_drivers`).
pub(crate) fn discover_all(
    drivers: &[Box<dyn Driver>],
    enabled_experimental: &std::collections::BTreeSet<String>,
) -> Vec<Box<dyn Device>> {
    drivers
        .iter()
        .filter(|d| !d.experimental() || enabled_experimental.contains(d.name()))
        .flat_map(|d| d.discover())
        .collect()
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
        let enabled = &self.config.experimental_drivers;
        for drv in &self.drivers {
            let allowed = !drv.experimental() || enabled.contains(drv.name());
            if allowed && !drv.usb_vendor_ids().is_empty() {
                rebuilt.extend(drv.discover());
            }
        }
        self.driver_devices = rebuilt;
    }

    /// Experimental drivers and whether the user enabled them (Système → Pilotes expérimentaux).
    pub(crate) fn experimental_drivers_json(&self) -> Json {
        Json::Array(
            self.drivers
                .iter()
                .filter(|d| d.experimental())
                .map(|d| {
                    serde_json::json!({
                        "name": d.name(),
                        "label": d.display_name(),
                        "enabled": self.config.experimental_drivers.contains(d.name()),
                    })
                })
                .collect(),
        )
    }

    /// Enable or disable an experimental driver, then rediscover devices.
    pub(crate) fn set_experimental_driver(
        &mut self,
        name: &str,
        enabled: bool,
    ) -> Result<(), String> {
        if !self
            .drivers
            .iter()
            .any(|d| d.experimental() && d.name() == name)
        {
            return Err(format!("Pilote expérimental inconnu : {name}"));
        }
        if enabled {
            self.config.experimental_drivers.insert(name.to_string());
        } else {
            self.config.experimental_drivers.remove(name);
        }
        self.persist_config();
        self.reconnect_all();
        Ok(())
    }

    pub(crate) fn device_list_json(&mut self) -> Json {
        let placements = self.config.device_placements.clone();
        let mut out = Vec::with_capacity(self.driver_devices.len());
        for d in &mut self.driver_devices {
            let info = d.info();
            let placement = placements.get(&info.id).cloned().unwrap_or_default();
            let status = d.status();
            let settings = d.settings();
            let cooling = d.cooling().map(|c| cooling_json(c));
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
                "cooling": cooling,
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
        // Some settings change fan control (Max Fan, thermal profile): keep the crash marker in sync.
        self.sync_cooling_marker();
        Ok(self.after_setting(kind, setting_id, &value))
    }

    /// Set one fan (or every fan, `fan = None`) of a device, clamped to its safety floor.
    pub(crate) fn set_device_fan(
        &mut self,
        device_id: &str,
        fan: Option<&str>,
        target: FanTarget,
    ) -> Result<(), String> {
        let device = self
            .device_mut(device_id)
            .ok_or_else(|| format!("Appareil introuvable : {device_id}"))?;
        let cooling = device
            .cooling()
            .ok_or_else(|| format!("Ventilateurs non pilotables sur {device_id}"))?;
        let target = clamp_for(cooling, fan, target)?;
        let res = cooling.set_fan(fan, target).map_err(|e| e.to_string());
        // The manual level was just sent: the next keep-alive is due one interval from now.
        self.cooling_keepalive
            .insert(device_id.to_string(), std::time::Instant::now());
        self.sync_cooling_marker();
        res
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
        let none = std::collections::BTreeSet::new();
        assert_eq!(discover_all(&drivers, &none).len(), 4);
    }

    struct ExperimentalDriver;

    impl Driver for ExperimentalDriver {
        fn name(&self) -> &'static str {
            "exp"
        }
        fn experimental(&self) -> bool {
            true
        }
        fn discover(&self) -> Vec<Box<dyn Device>> {
            vec![Box::new(FakeDevice)]
        }
    }

    #[test]
    fn experimental_drivers_need_an_explicit_opt_in() {
        let drivers: Vec<Box<dyn Driver>> =
            vec![Box::new(FakeDriver), Box::new(ExperimentalDriver)];
        let mut enabled = std::collections::BTreeSet::new();
        assert_eq!(discover_all(&drivers, &enabled).len(), 2);
        enabled.insert("exp".to_string());
        assert_eq!(discover_all(&drivers, &enabled).len(), 3);
    }
}
