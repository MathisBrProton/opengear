use super::backend::BackendEngine;
use crate::tray_icon::is_autostart_enabled;
use opengear_core::cooling::FanHeaderKind;
use opengear_core::driver::DeviceKind;
use opengear_core::{EffectKind, OperatingMode};
use opengear_corsair::CorsairDevice;
use opengear_input::{ALL_BINDABLE_KEYS, ALL_MOUSE_BUTTONS, ALL_MOUSE_TARGETS};
use opengear_logitech::{
    BatteryPowerModel, BatteryState, EQ_PRESETS, LogitechHeadset, LogitechMouse, MouseButtonAction,
    OnboardMode,
};
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

pub(crate) fn battery_is_charging(state: BatteryState) -> bool {
    matches!(
        state,
        BatteryState::Charging
            | BatteryState::SlowCharging
            | BatteryState::AlmostFull
            | BatteryState::Full
    )
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
    pub(crate) fn compute_mouse_power_model(&self, m: &LogitechMouse) -> Option<BatteryPowerModel> {
        let pct = m.info.battery.as_ref()?.percent?;
        let hz = m
            .info
            .report_rate
            .as_ref()
            .map(|r| r.current_hz)
            .unwrap_or(1000);
        if let Some(bm) = self
            .local_assets
            .find_battery_model(m.info.pid, &m.info.name)
        {
            Some(BatteryPowerModel::from_params(
                pct,
                bm.capacity_mwh,
                bm.sensor_mw,
                bm.mcu_mw,
                bm.radio_mw_for_hz(hz),
            ))
        } else {
            Some(BatteryPowerModel::for_pro_x_wireless(pct, hz))
        }
    }

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
        let logi_mice = self.typed_devices::<LogitechMouse>();
        let logi_bat = logi_mice.first().and_then(|m| {
            let b = m.info.battery.as_ref()?;
            let pm = self.compute_mouse_power_model(m);
            Some(json!({
                "percent": b.percent,
                "state": b.state.label(),
                "is_charging": battery_is_charging(b.state),
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
            "logi_battery": logi_bat,
            "laptop_battery": &self.laptop_battery,
            "device_fans": device_fans,
        })
    }

    pub(crate) fn serialize_corsair(
        &self,
        dev: &opengear_corsair::CorsairDevice,
        idx: usize,
    ) -> Value {
        let i = &dev.info;
        let hwp_profiles: Vec<Value> = i
            .hwp_profiles
            .iter()
            .map(|hp| {
                let lighting = hp.lighting.as_ref().map(|l| {
                    let cols: Vec<String> = l.colors.iter().map(|c| c.to_hex()).collect();
                    json!({
                        "file_id": format!("0x{:04X}", l.file_id),
                        "kind": l.kind.id(),
                        "kind_label": l.kind.label(),
                        "speed": l.speed,
                        "direction": l.direction,
                        "colors": cols,
                        "led_count": l.led_indices.len(),
                    })
                });
                json!({
                    "slot_index": hp.slot_index,
                    "profile_file_id": format!("0x{:04X}", hp.profile_file_id),
                    "name": hp.name,
                    "assignment_count": hp.assignment_count,
                    "property_count": hp.property_count,
                    "lighting": lighting,
                })
            })
            .collect();
        json!({
            "index": idx,
            "device_id": opengear_core::driver::Device::info(dev).id,
            "id": i.id,
            "product_name": i.product_name,
            "kind": format!("{:?}", i.kind),
            "kind_label": i.kind.label(),
            "vid": format!("{:04X}", i.vid),
            "pid": format!("{:04X}", i.pid),
            "subdevice_id": i.subdevice_id,
            "serial": i.serial,
            "firmware": i.firmware_version,
            "bootloader": i.bootloader_version,
            "layout": i.layout.label(),
            "mode": if i.mode == OperatingMode::Software { "sw" } else { "hw" },
            "pollrate_hz": i.pollrate.map(|p| p.hz()),
            "max_pollrate_hz": i.max_pollrate.map(|p| p.hz()),
            "hw_brightness_pct": dev.hw_brightness_pct(),
            "out_report_len": i.out_report_len,
            "battery_pct": i.battery_pct,
            "hw_winlock": i.hw_winlock,
            "winlock_shortcuts_mask": i.winlock_shortcuts_mask,
            "debounce_enabled": i.debounce_enabled,
            "debounce_timeout_ms": i.debounce_timeout_ms,
            "esports_switch_on": i.esports_switch_on,
            "active_hw_profile": i.active_hw_profile,
            "storage_total_kb": i.storage_total_kb,
            "storage_free_kb": i.storage_free_kb,
            "flashtap_supported": i.flashtap_supported,
            "flashtap_enabled": i.flashtap_enabled,
            "rapid_trigger_supported": i.rapid_trigger_supported,
            "hwp_profiles": hwp_profiles,
            "mouse": i.mouse,
            "headset": i.headset,
            "cooler": i.cooler,
        })
    }

    pub(crate) fn serialize_logi_mouse(&self, m: &LogitechMouse, idx: usize) -> Value {
        let i = &m.info;
        let power_model = self.compute_mouse_power_model(m);
        let profiles: Vec<Value> = i
            .onboard_profiles
            .iter()
            .map(|p| {
                let btns: Vec<Value> = p
                    .buttons
                    .iter()
                    .map(|b| {
                        let raw = b.encode();
                        json!({
                            "hex": format!("{:02X}{:02X}{:02X}{:02X}", raw[0], raw[1], raw[2], raw[3]),
                            "label": b.label(),
                        })
                    })
                    .collect();
                json!({
                    "slot_index": p.slot_index,
                    "sector_addr": format!("0x{:04X}", p.sector_addr),
                    "from_rom": p.from_rom,
                    "enabled": p.enabled,
                    "crc_valid": p.crc_valid,
                    "name": p.name,
                    "report_rate_hz": p.report_rate_hz,
                    "default_dpi_idx": p.default_dpi_idx,
                    "dpi_stages": p.dpi_stages,
                    "buttons": btns,
                })
            })
            .collect();
        json!({
            "index": idx,
            "device_id": opengear_core::driver::Device::info(m).id,
            "name": i.name,
            "vid": format!("{:04X}", i.vid),
            "pid": format!("{:04X}", i.pid),
            "device_index": format!("0x{:02X}", i.device_index),
            "proto_version": i.proto_version,
            "firmware": i.firmware,
            "battery": i.battery.as_ref().map(|b| json!({
                "percent": b.percent,
                "voltage_mv": b.voltage_mv,
                "state": b.state.label(),
                "is_charging": battery_is_charging(b.state),
            })),
            "power_model": power_model,
            "dpi": i.dpi,
            "report_rate": i.report_rate,
            "onboard": i.onboard.as_ref().map(|o| json!({
                "mode": match o.mode { OnboardMode::Host => "Host", _ => "Onboard" },
                "mode_label": o.mode.label(),
                "profile_count": o.profile_count,
                "button_count": o.button_count,
                "active_profile": o.active_profile,
                "active_dpi_index": o.active_dpi_index,
            })),
            "onboard_profiles": profiles,
            "angle_snapping": i.angle_snapping,
            "led_zones": i.led_zones,
            "features": i.features.iter().map(|f| json!({
                "index": f.index,
                "id": format!("0x{:04X}", f.id),
                "name": f.name,
            })).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn serialize_logi_headset(&self, h: &LogitechHeadset, idx: usize) -> Value {
        let i = &h.info;
        json!({
            "index": idx,
            "device_id": opengear_core::driver::Device::info(h).id,
            "name": i.name,
            "vid": format!("{:04X}", i.vid),
            "pid": format!("{:04X}", i.pid),
            "proto_version": i.proto_version,
            "firmware": i.firmware,
            "battery": i.battery.as_ref().map(|b| json!({
                "percent": b.percent,
                "voltage_mv": b.voltage_mv,
                "state": b.state.label(),
                "is_charging": battery_is_charging(b.state),
            })),
            "supports_sidetone": i.supports_sidetone,
            "sidetone_pct": i.sidetone_pct,
            "equalizer": i.equalizer,
            "presets": EQ_PRESETS.iter().enumerate().map(|(pi, p)| json!({
                "id": if pi == 0 { "flat" } else if pi == 1 { "fps" } else { "preset" },
                "name": p.name,
                "gains": p.gains_5,
                "is_flat": pi == 0 || p.gains_5.iter().all(|&g| g == 0),
            })).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn catalog_json() -> Value {
        let button_presets: Vec<Value> = MouseButtonAction::PRESET_ACTIONS
            .iter()
            .map(|(lbl, act)| {
                let raw = act.encode();
                json!({
                    "label": lbl,
                    "hex": format!("{:02X}{:02X}{:02X}{:02X}", raw[0], raw[1], raw[2], raw[3]),
                })
            })
            .collect();

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

        let corsair_list: Vec<Value> = self
            .typed_devices::<CorsairDevice>()
            .into_iter()
            .enumerate()
            .map(|(i, d)| self.serialize_corsair(d, i))
            .collect();
        let corsair_primary = corsair_list.first().cloned();

        let logi_mice_list: Vec<Value> = self
            .typed_devices::<LogitechMouse>()
            .into_iter()
            .enumerate()
            .map(|(i, m)| self.serialize_logi_mouse(m, i))
            .collect();
        let logi_headsets_list: Vec<Value> = self
            .typed_devices::<LogitechHeadset>()
            .into_iter()
            .enumerate()
            .map(|(i, h)| self.serialize_logi_headset(h, i))
            .collect();

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
            "button_presets": catalog["button_presets"],
            "power_plans": self.power_plans.iter().map(|p| json!({
                "guid": p.guid,
                "name": p.name,
                "active": p.active,
            })).collect::<Vec<_>>(),
            "corsair": corsair_primary,
            "corsair_devices": corsair_list,
            "logi_mouse": logi_mice_list.first().cloned(),
            "logi_mice": logi_mice_list,
            "logi_headset": logi_headsets_list.first().cloned(),
            "logi_headsets": logi_headsets_list,
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
