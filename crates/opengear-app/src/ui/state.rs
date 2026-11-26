use super::backend::BackendEngine;
use crate::local_assets::LocalAssetCatalog;
use crate::tray_icon::is_autostart_enabled;
use opengear_core::cooling::FanHeaderKind;
use opengear_core::driver::{DeviceKind, DeviceStatus, Setting, Value as DriverValue, setting_ids};
use opengear_core::{EffectKind, OnboardButtonBinding};
use opengear_input::{ALL_BINDABLE_KEYS, ALL_MOUSE_BUTTONS, ALL_MOUSE_TARGETS};
use opengear_mainboard::FanRpmReading;
use serde_json::{Value, json};

pub(crate) fn fan_short_label(h: FanHeaderKind) -> &'static str {
    match h {
        FanHeaderKind::CpuFan => "CPU_FAN",
        FanHeaderKind::AioPump => "AIO_PUMP",
        FanHeaderKind::ChassisFan1 => "CHA_FAN1",
        FanHeaderKind::ChassisFan2 => "CHA_FAN2",
        FanHeaderKind::ChassisFan3 => "CHA_FAN3",
    }
}

fn battery_state_label(st: &DeviceStatus) -> &'static str {
    match st.charging {
        Some(true) => "En charge",
        Some(false) if st.battery_pct.is_some() => "Sur batterie",
        _ => "Inconnu",
    }
}

fn parse_vid_pid_from_id(id: &str) -> u16 {
    let mut parts = id.split(':').skip(1);
    let vid = parts.next().unwrap_or("");
    let pid = parts.next().unwrap_or("");
    let is_hex4 = |s: &str| s.len() == 4 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if is_hex4(vid) && is_hex4(pid) {
        u16::from_str_radix(pid, 16).unwrap_or(0)
    } else {
        0
    }
}

fn setting_text(settings: &[Setting], id: &str) -> Option<String> {
    settings.iter().find_map(|s| {
        (s.id == id)
            .then(|| match &s.value {
                DriverValue::Text(v) => Some(v.clone()),
                _ => None,
            })
            .flatten()
    })
}

pub(crate) fn mouse_power_model_json(
    local_assets: &LocalAssetCatalog,
    device_id: &str,
    name: &str,
    pct: u8,
    hz: u16,
) -> Value {
    let pid = parse_vid_pid_from_id(device_id);
    let (capacity_mwh, sensor_mw, mcu_mw, radio_mw) =
        if let Some(bm) = local_assets.find_battery_model(pid, name) {
            (
                bm.capacity_mwh,
                bm.sensor_mw,
                bm.mcu_mw,
                bm.radio_mw_for_hz(hz),
            )
        } else {
            let radio_mw = match hz {
                0..=180 => 2.2036,
                181..=350 => 3.0738,
                351..=750 => 4.8142,
                _ => 8.2951,
            };
            (874.0, 1.5456, 2.3064, radio_mw)
        };
    let total_mw = (sensor_mw + mcu_mw + radio_mw).max(0.5);
    let remaining_mwh = capacity_mwh * (pct.min(100) as f32 / 100.0);
    let estimated_hours = remaining_mwh / total_mw;
    json!({
        "capacity_mwh": capacity_mwh,
        "sensor_mw": sensor_mw,
        "mcu_mw": mcu_mw,
        "radio_mw": radio_mw,
        "total_mw": total_mw,
        "estimated_hours": estimated_hours,
    })
}

pub(crate) const PRESETS: &[(&str, &str, &str)] = &[
    (
        "silent",
        "Silencieux",
        "Ventilateurs en profil silencieux, éclairage doux et tamisé, plan d'alimentation équilibré.",
    ),
    (
        "gaming",
        "Jeu",
        "Ventilateurs en profil turbo, éclairage arc-en-ciel, plan d'alimentation hautes performances.",
    ),
    (
        "office",
        "Bureau",
        "Ventilateurs en profil standard, éclairage blanc statique, plan d'alimentation équilibré.",
    ),
    (
        "lights_off",
        "Tout éteint",
        "Coupe l'éclairage de tous les appareils sans toucher aux ventilateurs.",
    ),
];

impl BackendEngine {
    /// Live fans of every device with `Cooling`, keyed by device id, plus the RPM readings of
    /// laptop fans (used when no motherboard sensor reports fans).
    fn device_fans(&mut self) -> (serde_json::Map<String, Value>, Vec<FanRpmReading>) {
        let mut by_device = serde_json::Map::new();
        let mut laptop_fans = Vec::new();
        for d in &mut self.driver_devices {
            let info = d.info();
            let Some(c) = d.cooling() else {
                continue;
            };
            if info.kind == DeviceKind::Laptop {
                laptop_fans.extend(c.fans().into_iter().filter_map(|f| {
                    Some(FanRpmReading {
                        name: f.name,
                        rpm: f.rpm?,
                    })
                }));
            }
            by_device.insert(info.id, super::devices::cooling_json(c));
        }
        (by_device, laptop_fans)
    }

    pub(crate) fn telemetry_json(&mut self) -> Value {
        let mut sensors = self.latest_sensors();
        let (device_fans, laptop_fans) = self.device_fans();
        if sensors.fans.is_empty() && !laptop_fans.is_empty() {
            if sensors.cpu_fan_rpm.is_none() {
                sensors.cpu_fan_rpm = laptop_fans.first().map(|f| f.rpm);
            }
            if !sensors.available && laptop_fans.iter().any(|f| f.rpm > 0) {
                sensors.available = true;
                if sensors.source.is_empty() {
                    sensors.source = "Ventilateurs du portable".into();
                } else {
                    sensors.source = format!("{} + ventilateurs du portable", sensors.source);
                }
            }
            sensors.fans = laptop_fans;
        }
        let mouse_bat = self
            .driver_devices
            .iter()
            .find(|d| d.info().kind == DeviceKind::Mouse && d.status().battery_pct.is_some())
            .and_then(|m| {
                let info = m.info();
                let st = m.status();
                let pct = st.battery_pct?;
                let settings = m.settings();
                let hz = setting_text(&settings, setting_ids::POLLING_RATE)
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(1000);
                let pm = mouse_power_model_json(&self.local_assets, &info.id, &info.model, pct, hz);
                Some(json!({
                    "percent": pct,
                    "state": battery_state_label(&st),
                    "is_charging": st.charging.unwrap_or(false),
                    "power_model": pm,
                }))
            });

        json!({
            "cpu_usage_pct": self.last_tel.cpu_usage_pct,
            "ram_used_gb": self.last_tel.ram_used_gb,
            "ram_total_gb": self.last_tel.ram_total_gb,
            "ram_usage_pct": self.last_tel.ram_usage_pct,
            "gpu": {
                "available": self.last_tel.gpu.available,
                "backend": self.last_tel.gpu.backend,
                "temp_c": self.last_tel.gpu.edge_temp_c.or(self.last_tel.gpu.hotspot_temp_c),
                "hotspot_c": self.last_tel.gpu.hotspot_temp_c,
                "activity_pct": self.last_tel.gpu.activity_pct,
                "fan_rpm": self.last_tel.gpu.fan_rpm,
                "core_clock_mhz": self.last_tel.gpu.core_clock_mhz,
                "power_watts": self.last_tel.gpu.power_watts,
            },
            "sensors": sensors,
            "audio": crate::audio_ext::read_audio_state(),
            "mouse_battery": mouse_bat,
            "laptop_battery": &self.laptop_battery,
            "device_fans": device_fans,
        })
    }

    pub(crate) fn catalog_json() -> Value {
        let button_presets: Vec<OnboardButtonBinding> = Vec::new();

        let effects: Vec<Value> = EffectKind::ALL
            .iter()
            .map(|&e| json!({ "id": format!("{:?}", e), "label": e.label() }))
            .collect();

        let blend_modes = vec![
            json!({ "id": "Normal", "label": "Normal" }),
            json!({ "id": "Add", "label": "Addition" }),
            json!({ "id": "Screen", "label": "Écran" }),
            json!({ "id": "Max", "label": "Maximum" }),
        ];

        let sensor_kinds = vec![
            json!({ "id": "Cpu", "label": "Processeur" }),
            json!({ "id": "GpuEdge", "label": "GPU cœur" }),
            json!({ "id": "GpuHotspot", "label": "GPU point chaud" }),
            json!({ "id": "Nvme", "label": "SSD NVMe" }),
        ];

        json!({
            "presets": PRESETS.iter().map(|(id, label, description)| json!({
                "id": id,
                "label": label,
                "description": description,
            })).collect::<Vec<_>>(),
            "bindable_keys": ALL_BINDABLE_KEYS,
            "bindable_mouse_buttons": ALL_MOUSE_BUTTONS.iter().map(|(id, label)| json!({
                "id": id,
                "label": label,
            })).collect::<Vec<_>>(),
            "bindable_mouse_targets": ALL_MOUSE_TARGETS.iter().map(|(id, label)| json!({
                "id": id,
                "label": label,
            })).collect::<Vec<_>>(),
            "effects": effects,
            "blend_modes": blend_modes,
            "sensor_kinds": sensor_kinds,
            "button_presets": button_presets,
        })
    }

    pub(crate) fn full_state_json(&mut self) -> Value {
        let active_profile = self.config.active_profile_mut().clone();
        let profile_names: Vec<String> = self
            .config
            .profiles
            .iter()
            .map(|p| p.name.clone())
            .collect();

        let button_presets = self
            .driver_devices
            .iter_mut()
            .find_map(|d| {
                d.onboard()
                    .map(|o| o.button_presets())
                    .filter(|p| !p.is_empty())
            })
            .unwrap_or_default();

        let cooling_channels: Vec<Value> = self
            .cooling_channels
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                json!({
                    "index": idx,
                    "header": format!("{:?}", c.header),
                    "label": c.header.label(),
                    "short_label": fan_short_label(c.header),
                    "is_pump": c.header == FanHeaderKind::AioPump,
                    "policy": format!("{:?}", c.policy),
                    "source": format!("{:?}", c.source),
                    "custom_mix": c.custom_mix,
                    "hysteresis_c": c.hysteresis_c,
                    "smoothing_secs": c.smoothing_secs,
                    "curve": c.curve,
                })
            })
            .collect();

        let devices: Vec<Value> = self
            .scanned_devices
            .iter()
            .map(|d| {
                let is_logi = d.vid == 0x046D
                    && ((d.usage_page == 0xFF00 && d.usage == 0x0002)
                        || (d.usage_page == 0xFF43 && d.usage == 0x0202));
                let is_asus = d.vid == 0x0B05 && d.usage_page == 0xFF72;
                let (status, driver_supported) = if is_asus {
                    ("Supporté (ASUS Aura USB)".to_string(), true)
                } else if is_logi {
                    ("Supporté (Logitech HID++ 2.0)".to_string(), true)
                } else {
                    let sup = d.driver_status.starts_with("Supporté");
                    (d.driver_status.clone(), sup)
                };
                json!({
                    "vendor": d.vendor.name(),
                    "vid_pid": format!("{:04X}:{:04X}", d.vid, d.pid),
                    "interface": d.interface_number,
                    "usage_page": format!("0x{:04X}", d.usage_page),
                    "usage": format!("0x{:04X}", d.usage),
                    "product": d.product_name,
                    "status": status,
                    "driver_supported": driver_supported,
                    "active": driver_supported,
                })
            })
            .collect();

        let (chatter_blocked, mouse_dblclick_blocked) = self.input_engine.stats();
        let visible_apps = self.visible_apps_cached();

        let mut fw_inventory: Vec<Value> = vec![json!({
            "device": self.telemetry.static_info.motherboard,
            "vendor": if self.is_laptop { "PC portable" } else { "Carte mère" },
            "version": format!("BIOS {}", self.telemetry.static_info.bios_version),
            "official_url": if self.is_laptop { "" } else { "https://rog.asus.com/support/" },
            "status": "Lecture BIOS directe"
        })];
        for dev in &self.driver_devices {
            let info = dev.info();
            let official_url = match info.vendor.as_str() {
                "Corsair" => "https://www.corsair.com/web-hub/",
                "Logitech" => "https://support.logi.com/",
                "ASUS" => "https://rog.asus.com/support/",
                "HP" => "https://support.hp.com/",
                _ => "",
            };
            fw_inventory.push(json!({
                "device": info.model,
                "vendor": info.vendor,
                "version": info.firmware.unwrap_or_else(|| "—".to_string()),
                "official_url": official_url,
                "status": "Pilote natif OpenGear",
            }));
        }

        let catalog = Self::catalog_json();
        let device_list = self.device_list_json();
        let rgb_layers_json: Vec<Value> = active_profile
            .rgb_layers
            .iter()
            .map(|l| {
                json!({
                    "enabled": l.enabled,
                    "blend": format!("{:?}", l.blend),
                    "opacity": l.opacity,
                    "key_filter": l.key_filter,
                    "effect": {
                        "kind": format!("{:?}", l.effect.kind),
                        "primary": l.effect.primary.to_hex(),
                        "secondary": l.effect.secondary.to_hex(),
                        "speed": l.effect.speed,
                        "brightness": l.effect.brightness,
                        "reverse": l.effect.reverse,
                    }
                })
            })
            .collect();

        let mut root = json!({
            "static_info": {
                "manufacturer": self.telemetry.static_info.manufacturer,
                "model": self.telemetry.static_info.model,
                "motherboard": self.telemetry.static_info.motherboard,
                "bios_version": self.telemetry.static_info.bios_version,
                "cpu_name": self.telemetry.static_info.cpu_name,
                "gpu_name": self.telemetry.static_info.gpu_name,
            },
            "app_version": env!("CARGO_PKG_VERSION"),
            "config_warning": self.config_warning,
            "ui": {
                "mode": self.config.ui_mode,
                "language": self.config.language,
                "onboarding_done": self.config.onboarding_done,
                "can_undo": !self.undo_stack.is_empty(),
                "undo_label": self.undo_stack.last().map(|(l, _)| l.clone()),
            },
            "presets": catalog["presets"],
            "system_prefs": {
                "autostart": is_autostart_enabled(),
                "minimize_to_tray": self.config.minimize_to_tray,
                "start_minimized": self.config.start_minimized,
                "auto_profile_switch": self.config.auto_profile_switch,
                "lights_off_on_lock": self.config.lights_off_on_lock,
                "idle_lights_off_min": self.config.idle_lights_off_min,
                "battery_alerts": self.config.battery_alerts,
                "laptop_auto_battery_saver": self.config.laptop_auto_battery_saver,
                "openrgb_server": self.config.openrgb_server_enabled,
                "openrgb_clients": self.openrgb_server.as_ref().map(|s| s.client_count()).unwrap_or(0),
                "openrgb_server_error": self.openrgb_server_error,
                "lights_blackout": self.blackout_lock || self.blackout_idle,
                "foreground_app": self.foreground_app,
                "auto_switched": self.auto_switched,
                "socd_suspended": self.input_engine.is_socd_suspended(),
                "chatter_blocked": chatter_blocked,
                "mouse_dblclick_blocked": mouse_dblclick_blocked,
                "macro_recording": self.input_engine.is_recording_macro(),
                "visible_apps": visible_apps,
            },
            "firmware_inventory": fw_inventory,
            "local_assets": self.local_assets.summary(),
            "telemetry": self.telemetry_json(),
            "config": {
                "active_profile": self.config.active_profile,
                "profiles": profile_names,
                "device_placements": self.config.device_placements,
                "profile": {
                    "name": active_profile.name,
                    "polling_rate_hz": active_profile.polling_rate.hz(),
                    "hw_brightness_pct": active_profile.hw_brightness_pct,
                    "enable_rgb": active_profile.enable_rgb,
                    "win_lock": active_profile.win_lock,
                    "bindings": active_profile.bindings,
                    "mouse_bindings": active_profile.mouse_bindings,
                    "layers": active_profile.layers,
                    "mouse_layers": active_profile.mouse_layers,
                    "socd": active_profile.socd,
                    "key_chatter_ms": active_profile.key_chatter_ms,
                    "mouse_debounce_ms": active_profile.mouse_debounce_ms,
                    "linked_apps": active_profile.linked_apps,
                    "device_lighting": active_profile.device_lighting,
                    "rgb_layers": rgb_layers_json,
                    "rgb": {
                        "kind": format!("{:?}", active_profile.rgb.kind),
                        "primary": active_profile.rgb.primary.to_hex(),
                        "secondary": active_profile.rgb.secondary.to_hex(),
                        "speed": active_profile.rgb.speed,
                        "brightness": active_profile.rgb.brightness,
                        "reverse": active_profile.rgb.reverse,
                        "per_key": active_profile.rgb.per_key.iter().map(|(k, v)| (k.clone(), v.to_hex())).collect::<std::collections::BTreeMap<_, _>>(),
                    }
                }
            },
            "input_running": self.input_engine.is_running(),
            "bindable_keys": catalog["bindable_keys"],
            "bindable_mouse_buttons": catalog["bindable_mouse_buttons"],
            "bindable_mouse_targets": catalog["bindable_mouse_targets"],
            "keyboard": self.keyboard_view().to_json(),
            "effects": catalog["effects"],
            "blend_modes": catalog["blend_modes"],
            "sensor_kinds": catalog["sensor_kinds"],
            "button_presets": button_presets,
            "power_plans": self.power_plans.iter().map(|p| json!({
                "guid": p.guid,
                "name": p.name,
                "active": p.active,
            })).collect::<Vec<_>>(),
            "cooling": {
                "is_admin": self.cooling_status.is_admin,
                "wmi_available": self.cooling_status.wmi_available,
                "message": self.cooling_status.message,
                "channels": cooling_channels,
                "superio": self.superio,
            },
            "selftest": self.last_selftest,
            "devices": devices,
            "device_list": device_list,
            "experimental_drivers": self.experimental_drivers_json(),
        });
        root["is_laptop"] = json!(self.is_laptop);
        root["laptop_battery"] = json!(&self.laptop_battery);
        root
    }
}
