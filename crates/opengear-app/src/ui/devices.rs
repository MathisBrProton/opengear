//! Every device behind the shared model (`opengear_core::driver`), whatever its vendor.
//!
//! All hardware backends are registered in `crate::drivers::device_drivers` and stored in
//! `driver_devices`, driving devices uniformly without vendor branches in the lighting engine.

use super::backend::BackendEngine;
use opengear_core::PollingRate;
use opengear_core::driver::{Device, DeviceKind, Driver, Value, setting_ids, validate_setting};
use opengear_core::fans::{Cooling, CurveCooling, FanTarget, clamp_target};
use opengear_core::onboard::OnboardMemory;
use opengear_core::pointer::PointerDevice;
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

/// Re-discover only the USB drivers whose `usb_vendor_ids()` intersects `vendors`,
/// keeping untouched drivers' devices intact.
pub(crate) fn rediscover_for_vendors(
    driver_devices: &mut Vec<Box<dyn Device>>,
    drivers: &[Box<dyn Driver>],
    enabled_experimental: &std::collections::BTreeSet<String>,
    vendors: &[u16],
) -> Vec<String> {
    let touched_prefixes: Vec<String> = drivers
        .iter()
        .filter(|d| {
            (!d.experimental() || enabled_experimental.contains(d.name()))
                && d.usb_vendor_ids().iter().any(|v| vendors.contains(v))
        })
        .map(|d| format!("{}:", d.name()))
        .collect();
    if touched_prefixes.is_empty() {
        return touched_prefixes;
    }

    let mut rebuilt: Vec<Box<dyn Device>> = driver_devices
        .drain(..)
        .filter(|d| {
            let id = d.info().id;
            !touched_prefixes.iter().any(|p| id.starts_with(p))
        })
        .collect();
    let enabled: Vec<&Box<dyn Driver>> = drivers
        .iter()
        .filter(|drv| {
            (!drv.experimental() || enabled_experimental.contains(drv.name()))
                && drv.usb_vendor_ids().iter().any(|v| vendors.contains(v))
        })
        .collect();
    if enabled.len() <= 1 {
        for drv in enabled {
            rebuilt.extend(drv.discover());
        }
    } else {
        std::thread::scope(|s| {
            let handles: Vec<_> = enabled
                .into_iter()
                .map(|drv| s.spawn(move || drv.discover()))
                .collect();
            for h in handles {
                if let Ok(devs) = h.join() {
                    rebuilt.extend(devs);
                }
            }
        });
    }
    *driver_devices = rebuilt;
    touched_prefixes
}

/// Discover the devices of every registered driver in parallel (preserving driver order);
/// experimental drivers only when enabled (`enabled_experimental` = `AppConfig::experimental_drivers`).
pub(crate) fn discover_all(
    drivers: &[Box<dyn Driver>],
    enabled_experimental: &std::collections::BTreeSet<String>,
) -> Vec<Box<dyn Device>> {
    let active: Vec<&Box<dyn Driver>> = drivers
        .iter()
        .filter(|d| !d.experimental() || enabled_experimental.contains(d.name()))
        .collect();
    if active.len() <= 1 {
        return active.into_iter().flat_map(|d| d.discover()).collect();
    }
    std::thread::scope(|s| {
        let handles: Vec<_> = active
            .into_iter()
            .map(|drv| s.spawn(move || drv.discover()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

impl BackendEngine {
    pub(crate) fn device_mut(&mut self, id: &str) -> Option<&mut (dyn Device + 'static)> {
        self.driver_devices
            .iter_mut()
            .find(|d| d.info().id == id)
            .map(|d| d.as_mut())
    }

    pub(crate) fn curve_cooling_for_mut(&mut self, id: &str) -> Option<&mut dyn CurveCooling> {
        self.device_mut(id).and_then(|d| d.fan_curves())
    }

    pub(crate) fn refresh_device_fans(&mut self, device_id: &str) -> Result<(), String> {
        let device = self
            .device_mut(device_id)
            .ok_or_else(|| format!("Appareil introuvable : {device_id}"))?;
        let cooling = device
            .cooling()
            .ok_or_else(|| format!("Ventilateurs non pilotables sur {device_id}"))?;
        cooling.refresh();
        self.sync_cooling_marker();
        Ok(())
    }

    pub(crate) fn find_pointer_mut(
        &mut self,
        device_id: Option<&str>,
    ) -> Option<&mut dyn PointerDevice> {
        self.device_mut(device_id?).and_then(|d| d.pointer())
    }

    pub(crate) fn find_onboard_mut(
        &mut self,
        device_id: Option<&str>,
    ) -> Option<&mut dyn OnboardMemory> {
        self.device_mut(device_id?).and_then(|d| d.onboard())
    }

    /// Re-discover only the USB driver(s) whose vendor matches a hotplug change.
    pub(crate) fn rediscover_drivers_for(&mut self, vendors: &[u16]) {
        let enabled = &self.config.experimental_drivers;
        rediscover_for_vendors(&mut self.driver_devices, &self.drivers, enabled, vendors);
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
        let curves_snapshot = self.curve_states.clone();
        let mut out = Vec::with_capacity(self.driver_devices.len());
        for d in &mut self.driver_devices {
            let info = d.info();
            let placement = placements.get(&info.id).cloned().unwrap_or_default();
            let status = d.status();
            let settings = d.settings();
            let specs = d.specs();
            let curves = curves_snapshot.get(&info.id).map(|cs| {
                let channels: Vec<Json> = cs
                    .channels
                    .iter()
                    .enumerate()
                    .map(|(idx, c)| {
                        json!({
                            "index": idx,
                            "header": format!("{:?}", c.header),
                            "label": c.header.label(),
                            "short_label": super::state::fan_short_label(c.header),
                            "is_pump": c.header == opengear_core::cooling::FanHeaderKind::AioPump,
                            "policy": format!("{:?}", c.policy),
                            "source": format!("{:?}", c.source),
                            "custom_mix": c.custom_mix,
                            "hysteresis_c": c.hysteresis_c,
                            "smoothing_secs": c.smoothing_secs,
                            "curve": c.curve,
                        })
                    })
                    .collect();
                json!({
                    "is_admin": cs.is_admin,
                    "available": cs.available,
                    "message": cs.message,
                    "channels": channels,
                })
            });
            let cooling = d.cooling().map(|c| cooling_json(c));
            let lighting = d.lighting().map(|l| {
                json!({
                    "zones": l.zones(),
                    "hardware_effects": l.hardware_effects(),
                    "max_fps": l.max_fps(),
                    "placement": placement,
                })
            });
            let pointer = d.pointer().and_then(|p| p.pointer_info());
            let battery_model = (info.kind == DeviceKind::Mouse)
                .then(|| {
                    status.battery_pct.map(|pct| {
                        let hz = pointer
                            .as_ref()
                            .and_then(|p| p.report_rate_hz)
                            .unwrap_or(1000);
                        super::state::mouse_power_model_json(
                            &self.local_assets,
                            &info.id,
                            &info.model,
                            pct,
                            hz,
                        )
                    })
                })
                .flatten();
            let equalizer = d.equalizer().and_then(|e| e.eq_info());
            let onboard = d.onboard().map(|o| {
                json!({
                    "info": o.onboard_info(),
                    "profiles": o.onboard_profiles(),
                    "button_presets": o.button_presets(),
                    "backups": o.onboard_backups(),
                })
            });
            out.push(json!({
                "info": info,
                "status": status,
                "settings": settings,
                "specs": specs,
                "curves": curves,
                "lighting": lighting,
                "cooling": cooling,
                "pointer": pointer,
                "battery_model": battery_model,
                "equalizer": equalizer,
                "onboard": onboard,
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
        Ok(self.after_setting(device_id, kind, setting_id, &value))
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
        device_id: &str,
        kind: DeviceKind,
        setting_id: &str,
        value: &Value,
    ) -> Option<String> {
        match setting_id {
            // Polling rate changes on USB devices (e.g. Corsair AXON keyboards) can trigger USB
            // re-enumeration that clears the device's live LED framebuffer: force an immediate
            // frame resend (+ confirmation frame) whenever software lighting is active.
            setting_ids::POLLING_RATE => {
                if kind == DeviceKind::Keyboard
                    && let Some(rate) = value
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .and_then(PollingRate::from_hz)
                {
                    self.config.active_profile_mut().polling_rate = rate;
                    self.persist_config();
                }
                self.invalidate_rgb_frame_for(device_id);
                self.live_active_devices.remove(device_id);
                if self.lighting_activity_flags().0 {
                    self.push_live_rgb_frame();
                }
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

    struct OtherDevice;
    impl Device for OtherDevice {
        fn info(&self) -> DeviceInfo {
            DeviceInfo {
                id: "other:1".into(),
                vendor: "Other".into(),
                model: "Other".into(),
                kind: DeviceKind::Keyboard,
                connection: Connection::Usb,
                firmware: None,
            }
        }
    }

    struct CountingOtherDriver(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Driver for CountingOtherDriver {
        fn name(&self) -> &'static str {
            "other"
        }
        fn usb_vendor_ids(&self) -> &'static [u16] {
            &[0x9999]
        }
        fn discover(&self) -> Vec<Box<dyn Device>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![Box::new(OtherDevice)]
        }
    }

    #[test]
    fn targeted_hotplug_skips_unrelated_usb_drivers() {
        let other_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let drivers: Vec<Box<dyn Driver>> = vec![
            Box::new(FakeDriver),
            Box::new(CountingOtherDriver(other_calls.clone())),
        ];
        let enabled = std::collections::BTreeSet::new();
        let mut devs = discover_all(&drivers, &enabled);
        assert_eq!(other_calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let touched = rediscover_for_vendors(&mut devs, &drivers, &enabled, &[0x1234]);
        assert_eq!(touched, vec!["fake:".to_string()]);
        assert_eq!(other_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(devs.len(), 3);
    }
}
