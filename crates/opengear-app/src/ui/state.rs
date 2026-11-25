use super::backend::BackendEngine;
use crate::tray_icon::is_autostart_enabled;
use opengear_asus::{AuraChannelKind, AuraHwMode, EneRamMode, FanHeaderKind, FanRpmReading};
use opengear_core::{EffectKind, OperatingMode};
use opengear_corsair::HwpLightingKind;
use opengear_input::ALL_BINDABLE_KEYS;
use opengear_logitech::{
    BatteryPowerModel, BatteryState, EQ_PRESETS, LogitechHeadset, LogitechMouse, MouseButtonAction,
    OnboardMode,
};
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
        if let Some(bm) = self.local_assets.find_battery_model(m.info.pid, &m.info.name) {
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

    pub(crate) fn telemetry_json(&self) -> Value {
        let mut sensors = self.latest_sensors();
        if sensors.fans.is_empty() && !self.hp_omen.fan_rpms.is_empty() {
            let names = ["Ventilo CPU (OMEN)", "Ventilo GPU (OMEN)"];
            sensors.fans = self
                .hp_omen
                .fan_rpms
                .iter()
                .enumerate()
                .map(|(idx, &rpm)| FanRpmReading {
                    name: names
                        .get(idx)
                        .copied()
                        .unwrap_or("Ventilo")
                        .to_string(),
                    rpm,
                })
                .collect();
            if sensors.cpu_fan_rpm.is_none() {
                sensors.cpu_fan_rpm = self.hp_omen.fan_rpms.first().copied();
            }
            if !sensors.available && self.hp_omen.fan_rpms.iter().any(|&r| r > 0) {
                sensors.available = true;
                if sensors.source.is_empty() {
                    sensors.source = "HP OMEN BIOS WMI".into();
                } else {
                    sensors.source = format!("{} + HP OMEN WMI", sensors.source);
                }
            }
        }
        let logi_bat = self.logi_mice.first().and_then(|m| {
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
            "hp_omen": {
                "fan_rpms": &self.hp_omen.fan_rpms,
                "max_fan": self.hp_omen.max_fan,
                "manual_fan_pct": self.hp_omen.manual_fan_pct,
                "thermal_mode": self.hp_omen.thermal_mode.id(),
            },
        })
    }

    pub(crate) fn hp_omen_json(&self) -> Value {
        let o = &self.hp_omen;
        let zones: Vec<String> = o.zone_colors.iter().map(|c| c.to_hex()).collect();
        json!({
            "detected": o.detected,
            "is_admin": o.is_admin,
            "wmi_accessible": o.wmi_accessible,
            "model": o.model,
            "board_id": o.board_id,
            "thermal_version": o.thermal_version,
            "thermal_mode": o.thermal_mode.id(),
            "thermal_mode_label": o.thermal_mode.label(),
            "max_fan": o.max_fan,
            "manual_fan_pct": o.manual_fan_pct,
            "fan_count": o.fan_count,
            "fan_rpms": o.fan_rpms,
            "fan_max_rpms": o.fan_max_rpms,
            "gpu_power": o.gpu_power.id(),
            "gpu_peak_temp_c": o.gpu_peak_temp_c,
            "keyboard_type": format!("{:?}", o.keyboard_type),
            "keyboard_label": o.keyboard_label,
            "backlight_on": o.backlight_on,
            "zone_colors": zones,
            "sync_rgb": self.config.omen_sync_rgb,
            "saved_zones": self.config.omen_zone_colors,
            "diagnostic": o.diagnostic,
        })
    }

    pub(crate) fn serialize_corsair(&self, dev: &opengear_corsair::CorsairDevice, idx: usize) -> Value {
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

        let keys_layout: Vec<Value> = opengear_corsair::K70_TKL_KEYS
            .iter()
            .map(|k| {
                json!({
                    "id": k.id,
                    "qwerty": k.label_qwerty,
                    "azerty": k.label_azerty,
                    "led_index": k.led_index,
                    "x": k.x,
                    "y": k.y,
                    "w": k.width,
                    "h": k.height,
                    "norm_x": k.norm_x(),
                    "norm_y": k.norm_y(),
                })
            })
            .collect();

        let effects: Vec<Value> = EffectKind::ALL
            .iter()
            .map(|&e| json!({ "id": format!("{:?}", e), "label": e.label() }))
            .collect();

        let aura_modes: Vec<Value> = AuraHwMode::HARDWARE_MODES
            .iter()
            .map(|&m| {
                json!({
                    "id": format!("{:?}", m),
                    "label": m.label(),
                    "uses_color": m.uses_color(),
                })
            })
            .collect();

        let ram_rgb_modes: Vec<Value> = EneRamMode::ALL
            .iter()
            .map(|&m| {
                json!({
                    "id": m.id(),
                    "label": m.label(),
                    "uses_color": m.uses_color(),
                })
            })
            .collect();

        json!({
            "presets": PRESETS.iter().map(|(id, label, description)| json!({
                "id": id,
                "label": label,
                "description": description,
            })).collect::<Vec<_>>(),
            "bindable_keys": ALL_BINDABLE_KEYS,
            "keys_layout": keys_layout,
            "effects": effects,
            "hwp_lighting_kinds": HwpLightingKind::ALL_SUPPORTED.iter().map(|&k| json!({
                "id": k.id(),
                "label": k.label(),
                "color_count": k.color_count(),
            })).collect::<Vec<_>>(),
            "aura_modes": aura_modes,
            "ram_rgb_modes": ram_rgb_modes,
            "button_presets": button_presets,
        })
    }

    pub(crate) fn full_state_json(&mut self) -> Value {
        let active_profile = self.config.active_profile_mut().clone();
        let profile_names: Vec<String> = self.config.profiles.iter().map(|p| p.name.clone()).collect();

        let corsair_list: Vec<Value> = self
            .corsair_devices
            .iter()
            .enumerate()
            .map(|(i, d)| self.serialize_corsair(d, i))
            .collect();
        let corsair_primary = corsair_list.first().cloned();

        let asus_json = self.asus_aura.as_ref().map(|a| {
            let i = &a.info;
            let channels: Vec<Value> = i
                .channels
                .iter()
                .map(|c| {
                    json!({
                        "index": c.index,
                        "name": c.name,
                        "kind": if c.kind == AuraChannelKind::AddressableGen2 { "argb" } else { "mainboard" },
                        "led_count": c.led_count,
                        "default_led_count": c.default_led_count,
                    })
                })
                .collect();
            json!({
                "product_name": i.product_name,
                "vid": format!("{:04X}", i.vid),
                "pid": format!("{:04X}", i.pid),
                "firmware": i.firmware_version,
                "mainboard_fixed_leds": i.mainboard_fixed_leds,
                "rgb_12v_headers": i.rgb_12v_headers,
                "argb_5v_headers": i.argb_5v_headers,
                "channels": channels,
                "hw_mode": format!("{:?}", self.aura_hw_mode),
                "hw_color": self.aura_hw_color.to_hex(),
                "shutdown_mode": format!("{:?}", self.aura_shutdown_mode),
                "shutdown_color": self.aura_shutdown_color.to_hex(),
            })
        });

        let logi_mice_list: Vec<Value> = self
            .logi_mice
            .iter()
            .enumerate()
            .map(|(i, m)| self.serialize_logi_mouse(m, i))
            .collect();
        let logi_kbs_list: Vec<Value> = self
            .logi_keyboards
            .iter()
            .enumerate()
            .map(|(idx, k)| {
                let i = &k.info;
                json!({
                    "index": idx,
                    "id": i.id,
                    "name": i.name,
                    "vid": format!("{:04X}", i.vid),
                    "pid": format!("{:04X}", i.pid),
                    "device_index": format!("0x{:02X}", i.device_index),
                    "proto_version": i.proto_version,
                    "firmware": i.firmware,
                    "battery": i.battery.as_ref().map(|b| json!({
                        "percent": b.percent,
                        "state": b.state.label(),
                        "is_charging": battery_is_charging(b.state),
                    })),
                    "supports_rgb_effects": i.supports_rgb_effects,
                    "supports_brightness": i.supports_brightness,
                    "brightness_pct": i.brightness_pct,
                    "supports_game_mode": i.supports_game_mode,
                })
            })
            .collect();
        let logi_headsets_list: Vec<Value> = self
            .logi_headsets
            .iter()
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
                    "hysteresis_c": c.hysteresis_c,
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
                let is_asus = d.vid == 0x0B05 && d.pid == 0x19AF && d.usage_page == 0xFF72;
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
            "vendor": if self.hp_omen.detected { "HP OMEN" } else { "Carte mère" },
            "version": format!("BIOS {}", self.telemetry.static_info.bios_version),
            "official_url": if self.hp_omen.detected { "https://support.hp.com/" } else { "https://rog.asus.com/support/" },
            "status": "Lecture BIOS directe"
        })];
        if let Some(aura) = self.asus_aura.as_ref() {
            fw_inventory.push(json!({
                "device": "ASUS AURA LED Controller (0B05:19AF)",
                "vendor": "ASUS ROG",
                "version": aura.info.firmware_version,
                "official_url": "https://rog.asus.com/support/",
                "status": "MCU USB vérifié"
            }));
        }
        for d in &self.corsair_devices {
            fw_inventory.push(json!({
                "device": format!("{} ({:04X}:{:04X})", d.info.product_name, d.info.vid, d.info.pid),
                "vendor": "Corsair",
                "version": d.info.firmware_version,
                "official_url": "https://www.corsair.com/web-hub/",
                "status": "Bragi v2 vérifié"
            }));
        }
        for m in &self.logi_mice {
            fw_inventory.push(json!({
                "device": format!("{} ({:04X}:{:04X})", m.info.name, m.info.vid, m.info.pid),
                "vendor": "Logitech G",
                "version": if m.info.firmware.is_empty() { "HID++ 2.0".to_string() } else { m.info.firmware.join(" • ") },
                "official_url": "https://support.logi.com/hc/articles/360035037273",
                "status": "HID++ 2.0 vérifié"
            }));
        }
        for h in &self.logi_headsets {
            fw_inventory.push(json!({
                "device": format!("{} ({:04X}:{:04X})", h.info.name, h.info.vid, h.info.pid),
                "vendor": "Logitech G",
                "version": if h.info.firmware.is_empty() { "USB Audio".to_string() } else { h.info.firmware.join(" • ") },
                "official_url": "https://support.logi.com/",
                "status": "HID++ Audio vérifié"
            }));
        }

        let catalog = Self::catalog_json();

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
                "use_azerty_labels": self.config.use_azerty_labels,
                "profile": {
                    "name": active_profile.name,
                    "polling_rate_hz": active_profile.polling_rate.hz(),
                    "hw_brightness_pct": active_profile.hw_brightness_pct,
                    "enable_rgb": active_profile.enable_rgb,
                    "sync_asus_aura": active_profile.sync_asus_aura,
                    "win_lock": active_profile.win_lock,
                    "bindings": active_profile.bindings,
                    "socd": active_profile.socd,
                    "key_chatter_ms": active_profile.key_chatter_ms,
                    "mouse_debounce_ms": active_profile.mouse_debounce_ms,
                    "linked_apps": active_profile.linked_apps,
                    "silent_loop3_argb_channel": active_profile.silent_loop3_argb_channel,
                    "silent_loop3_thermal_rgb": active_profile.silent_loop3_thermal_rgb,
                    "rgb": {
                        "kind": format!("{:?}", active_profile.rgb.kind),
                        "primary": active_profile.rgb.primary.to_hex(),
                        "secondary": active_profile.rgb.secondary.to_hex(),
                        "speed": active_profile.rgb.speed,
                        "brightness": active_profile.rgb.brightness,
                        "reverse": active_profile.rgb.reverse,
                        "per_key": active_profile.rgb.per_key.iter().map(|(k, v)| (k.to_string(), v.to_hex())).collect::<std::collections::BTreeMap<_, _>>(),
                    }
                }
            },
            "input_running": self.input_engine.is_running(),
            "bindable_keys": catalog["bindable_keys"],
            "keys_layout": catalog["keys_layout"],
            "effects": catalog["effects"],
            "hwp_lighting_kinds": catalog["hwp_lighting_kinds"],
            "aura_modes": catalog["aura_modes"],
            "ram_rgb": self.ram_rgb,
            "ram_rgb_modes": catalog["ram_rgb_modes"],
            "button_presets": catalog["button_presets"],
            "power_plans": self.power_plans.iter().map(|p| json!({
                "guid": p.guid,
                "name": p.name,
                "active": p.active,
            })).collect::<Vec<_>>(),
            "corsair": corsair_primary,
            "corsair_devices": corsair_list,
            "corsair_err": if self.corsair_devices.is_empty() { Some("Aucun périphérique Corsair Bragi v2 détecté") } else { None },
            "asus": asus_json,
            "asus_err": self.asus_err,
            "logi_mouse": logi_mice_list.first().cloned(),
            "logi_mice": logi_mice_list,
            "logi_keyboards": logi_kbs_list,
            "logi_headset": logi_headsets_list.first().cloned(),
            "logi_headsets": logi_headsets_list,
            "logi_mouse_err": if self.logi_mice.is_empty() { Some("Aucune souris Logitech détectée (en veille ?)") } else { None },
            "logi_headset_err": if self.logi_headsets.is_empty() { Some("Aucun casque Logitech détecté") } else { None },
            "cooling": {
                "is_admin": self.cooling_status.is_admin,
                "wmi_available": self.cooling_status.wmi_available,
                "message": self.cooling_status.message,
                "channels": cooling_channels,
                "superio": self.superio,
            },
            "selftest": self.last_selftest,
            "devices": devices,
        });
        root["is_laptop"] = json!(self.is_laptop);
        root["laptop_battery"] = json!(&self.laptop_battery);
        root["laptop_display"] = json!(&self.laptop_display);
        root["hp_omen"] = self.hp_omen_json();
        root
    }
}
