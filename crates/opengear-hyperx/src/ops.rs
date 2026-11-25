//! Protocol operations, settings, discovery, and battery/status probing for HyperX devices.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use hidapi::HidApi;
use opengear_core::driver::{
    polling_rate_options, setting_ids, ChoiceOption, Device, DeviceKind, DeviceStatus, Section,
    Setting, Value,
};
use opengear_core::Rgb;
use tracing::{debug, warn};

use crate::model::HyperXDevice;
use crate::proto::{HidHxIo, HxIo};
use crate::proto_ngenuity::{
    angle_tune_wire_byte, cloud2_dts_pkt, cloud2_wireless_pkt, cloud3_wireless_pkt,
    cloud3s_auto_sleep_pkt, cloud3s_eq_band_pkt, cloud3s_eq_index_pkt, cloud3s_wireless_pkt,
    cloud_alpha_wireless_pkt, cloud_flight_battery_pct, cloud_flight_wireless_battery_req,
    cloud_stinger_core_wireless_battery_req, cloud_tune_eq_pkts, duocast_set_gain_pkt,
    duocast_set_highpass_pkt, duocast_set_lights_info_pkt, duocast_set_polar_pkt,
    duocast_set_sidetone_pkt, fury_ultra_brightness_pkts, legacy_haste_wired_profile_pkts,
    legacy_mouse_battery_req, legacy_mouse_dpi_pkts, legacy_mouse_low_battery_pkt,
    legacy_mouse_polling_pkt, parse_cloud2_dts_resp, parse_cloud2_wireless_resp,
    parse_cloud3_wireless_resp, parse_cloud_alpha_wireless_resp,
    parse_cloud_stinger_core_wireless_battery, parse_legacy_mouse_battery, parse_univ_battery,
    parse_univ_device_info, quadcast2_set_highpass_pkt, quadcast2_set_mute_pkt,
    quadcast2_set_polar_pkt, quadcast2s_set_balance_pkt, quadcast2s_set_mix_pkt,
    solocast_set_mute_pkt, univ_frame_light_pkts, univ_get_battery_pkt, univ_get_device_info_pkt,
    univ_set_ambient_brightness_pkt, univ_set_auto_sleep_pkt, univ_set_backlight_fade_pkt,
    univ_set_brightness_pkt, univ_set_game_mode_bits_pkt, univ_set_generic_audio_full_pkt,
    univ_set_low_battery_pkt, univ_set_mouse_dpi_pkt, univ_set_mouse_sensor_full_pkt,
    univ_set_noise_gate_pkt, univ_set_onboard_eq_pkt, univ_set_voice_prompt_pkt, UnivAudioConfig,
    UnivDeviceInfo, EQ_10_FREQS,
};
use crate::tables::{
    find_device, match_score, tio_child_name, EqHwMode, HxCapabilities, HxDeviceDef,
    ProtocolFamily, HP_HYPERX_VID, KINGSTON_VID,
};

/// Mutable device state cached across UI renders and incremental setting updates.
#[derive(Debug, Clone)]
pub struct HxState {
    pub brightness_pct: u8,
    pub tio_slot: u8,
    pub dpi_stages: [u16; 5],
    pub dpi_colors: [Rgb; 5],
    pub dpi_count: u8,
    pub active_dpi_stage: u8,
    pub polling_hz: u16,
    pub angle_snap: bool,
    pub angle_tune_deg: i16,
    pub lod_2mm: bool,
    pub win_lock: bool,
    pub game_mode_mask: u8,
    pub low_battery_pct: u8,
    pub backlight_fade_secs: u16,
    pub ambient_brightness: bool,
    pub sidetone_enabled: bool,
    pub sidetone_pct: u8,
    pub sidetone_db: i8,
    pub mic_muted: bool,
    pub mic_level: u8,
    pub polar_pattern: &'static str,
    pub high_pass: bool,
    pub balance_pct: u8,
    pub chatmix_pct: u8,
    pub playback_pct: u8,
    pub reverse_lights: bool,
    pub noise_gate: bool,
    pub voice_prompt: bool,
    pub eq_preset: u8,
    pub eq_bands: [f32; 10],
    pub auto_sleep_min: u16,
    pub last_frame: Vec<Rgb>,
    pub status: DeviceStatus,
    pub univ_info: Option<UnivDeviceInfo>,
    pub custom_name: Option<String>,
    pub tio_children: Vec<String>,
}

impl Default for HxState {
    fn default() -> Self {
        Self {
            brightness_pct: 100,
            tio_slot: 0,
            dpi_stages: [400, 800, 1600, 3200, 6400],
            dpi_colors: [
                Rgb::CYAN,
                Rgb::BLUE,
                Rgb::GREEN,
                Rgb::new(255, 0, 255),
                Rgb::new(255, 255, 0),
            ],
            dpi_count: 5,
            active_dpi_stage: 1,
            polling_hz: 1000,
            angle_snap: false,
            angle_tune_deg: 0,
            lod_2mm: false,
            win_lock: false,
            game_mode_mask: 0,
            low_battery_pct: 15,
            backlight_fade_secs: 0,
            ambient_brightness: false,
            sidetone_enabled: false,
            sidetone_pct: 50,
            sidetone_db: 0,
            mic_muted: false,
            mic_level: 100,
            polar_pattern: "cardioid",
            high_pass: false,
            balance_pct: 50,
            chatmix_pct: 50,
            playback_pct: 100,
            reverse_lights: false,
            noise_gate: false,
            voice_prompt: true,
            eq_preset: 0,
            eq_bands: [0.0; 10],
            auto_sleep_min: 20,
            last_frame: Vec::new(),
            status: DeviceStatus::default(),
            univ_info: None,
            custom_name: None,
            tio_children: Vec::new(),
        }
    }
}

/// Shared HID transport used to safely multiplex a 3-in-1 TIO dongle across child devices.
pub struct SharedHxIo(pub Arc<Mutex<HidHxIo>>);

impl HxIo for SharedHxIo {
    fn write_output(&mut self, data: &[u8]) -> Result<()> {
        let mut guard = self.0.lock().map_err(|_| anyhow::anyhow!("TIO mutex poisoned"))?;
        guard.write_output(data)
    }
    fn send_feature(&mut self, data: &[u8]) -> Result<()> {
        let mut guard = self.0.lock().map_err(|_| anyhow::anyhow!("TIO mutex poisoned"))?;
        guard.send_feature(data)
    }
    fn read_input(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
        let mut guard = self.0.lock().map_err(|_| anyhow::anyhow!("TIO mutex poisoned"))?;
        guard.read_input(buf, timeout_ms)
    }
    fn get_feature(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut guard = self.0.lock().map_err(|_| anyhow::anyhow!("TIO mutex poisoned"))?;
        guard.get_feature(buf)
    }
}

fn exchange_out(io: &mut dyn HxIo, req: &[u8], resp: &mut [u8]) -> Option<usize> {
    if io.write_output(req).is_err() {
        return None;
    }
    io.read_input(resp, 120).ok().filter(|&n| n > 0)
}

/// Read-only status & device identity probe executed at discovery and refresh.
pub fn probe_and_refresh(io: &mut dyn HxIo, def: &'static HxDeviceDef, st: &mut HxState) -> DeviceStatus {
    let caps = def.caps();
    let mut buf = [0u8; 65];
    if caps.universal {
        if st.univ_info.is_none() {
            if let Some(n) = exchange_out(io, &univ_get_device_info_pkt(st.tio_slot), &mut buf) {
                if let Some(info) = parse_univ_device_info(&buf[..n]) {
                    st.univ_info = Some(info);
                }
            }
        }
        if caps.battery {
            if let Some(n) = exchange_out(io, &univ_get_battery_pkt(st.tio_slot), &mut buf) {
                if let Some((pct, chg)) = parse_univ_battery(&buf[..n]) {
                    st.status.battery_pct = Some(pct);
                    st.status.charging = chg;
                    update_battery_hours(def, &mut st.status);
                }
            }
        }
        return st.status.clone();
    }

    match def.family {
        ProtocolFamily::PulsefireDart | ProtocolFamily::PulsefireHasteWireless => {
            if let Some(n) = exchange_out(io, &legacy_mouse_battery_req(), &mut buf) {
                if let Some((pct, chg)) = parse_legacy_mouse_battery(&buf[..n]) {
                    st.status.battery_pct = Some(pct);
                    st.status.charging = chg;
                }
            }
        }
        ProtocolFamily::CloudAlphaWireless => {
            if let Some(n) = exchange_out(io, &cloud_alpha_wireless_pkt(0x0B, 0), &mut buf) {
                let (bat, _) = parse_cloud_alpha_wireless_resp(&buf[..n]);
                if bat.is_some() {
                    st.status.battery_pct = bat;
                }
            }
            if let Some(n) = exchange_out(io, &cloud_alpha_wireless_pkt(0x0C, 0), &mut buf) {
                let (_, chg) = parse_cloud_alpha_wireless_resp(&buf[..n]);
                if chg.is_some() {
                    st.status.charging = chg;
                }
            }
        }
        ProtocolFamily::CloudIIWirelessDts | ProtocolFamily::CloudStinger2Wireless => {
            if let Some(n) = exchange_out(io, &cloud2_dts_pkt(0x02, 0), &mut buf) {
                let (bat, _) = parse_cloud2_dts_resp(&buf[..n]);
                if bat.is_some() {
                    st.status.battery_pct = bat;
                }
            }
            if let Some(n) = exchange_out(io, &cloud2_dts_pkt(0x03, 0), &mut buf) {
                let (_, chg) = parse_cloud2_dts_resp(&buf[..n]);
                if chg.is_some() {
                    st.status.charging = chg;
                }
            }
        }
        ProtocolFamily::CloudIIWireless | ProtocolFamily::CloudFlightS => {
            if let Some(n) = exchange_out(io, &cloud2_wireless_pkt(0x02, 0), &mut buf) {
                let (bat, _) = parse_cloud2_wireless_resp(&buf[..n]);
                if bat.is_some() {
                    st.status.battery_pct = bat;
                }
            }
            if let Some(n) = exchange_out(io, &cloud2_wireless_pkt(0x03, 0), &mut buf) {
                let (_, chg) = parse_cloud2_wireless_resp(&buf[..n]);
                if chg.is_some() {
                    st.status.charging = chg;
                }
            }
        }
        ProtocolFamily::CloudFlightWireless => {
            if let Some(n) = exchange_out(io, &cloud_flight_wireless_battery_req(), &mut buf) {
                if n >= 5 && buf[0] == 0x21 && buf[1] == 0xFF && buf[2] == 0x05 {
                    let (pct, chg) = cloud_flight_battery_pct(buf[3], buf[4]);
                    st.status.battery_pct = Some(pct);
                    st.status.charging = Some(chg);
                }
            }
        }
        ProtocolFamily::CloudIIICoreWireless => {
            if let Some(n) = exchange_out(io, &cloud3_wireless_pkt(0x89, 0), &mut buf) {
                let (bat, _) = parse_cloud3_wireless_resp(&buf[..n]);
                if bat.is_some() {
                    st.status.battery_pct = bat;
                }
            }
            if let Some(n) = exchange_out(io, &cloud3_wireless_pkt(0x8A, 0), &mut buf) {
                let (_, chg) = parse_cloud3_wireless_resp(&buf[..n]);
                if chg.is_some() {
                    st.status.charging = chg;
                }
            }
        }
        ProtocolFamily::CloudIIISWireless => {
            let mut resp = [0u8; 65];
            if let Some(n) = exchange_out(io, &cloud3s_wireless_pkt(0x06, 0, 0), &mut resp) {
                if n >= 7 && (resp[0] == 0x0C || resp[0] == 0x0D) {
                    st.status.battery_pct = Some(resp[6].min(100));
                }
            }
            if let Some(n) = exchange_out(io, &cloud3s_wireless_pkt(0x48, 0, 0), &mut resp) {
                if n >= 7 && (resp[0] == 0x0C || resp[0] == 0x0D) {
                    st.status.charging = Some(resp[6] == 1 || resp[6] == 2);
                }
            }
        }
        ProtocolFamily::CloudStingerCoreWireless => {
            if io.send_feature(&cloud_stinger_core_wireless_battery_req()).is_ok() {
                std::thread::sleep(std::time::Duration::from_millis(20));
                if let Ok(n) = io.get_feature(&mut buf) {
                    if let Some((pct, chg)) = parse_cloud_stinger_core_wireless_battery(&buf[..n]) {
                        st.status.battery_pct = Some(pct);
                        st.status.charging = chg;
                    }
                }
            }
        }
        _ => {}
    }
    update_battery_hours(def, &mut st.status);
    st.status.clone()
}

fn update_battery_hours(def: &'static HxDeviceDef, status: &mut DeviceStatus) {
    status.battery_hours = match (status.battery_pct, def.rated_battery_hours()) {
        (Some(pct), Some(max_h)) => Some(((u32::from(pct) * u32::from(max_h) + 50) / 100) as u16),
        _ => None,
    };
}

/// Build standardized + vendor-prefixed `Setting` definitions for a HyperX device.
pub fn build_settings(def: &'static HxDeviceDef, st: &HxState) -> Vec<Setting> {
    let caps = def.caps();
    let mut out = Vec::new();
    append_mouse_settings(&caps, st, &mut out);
    append_keyboard_settings(&caps, st, &mut out);
    append_lighting_and_power_settings(&caps, st, &mut out);
    append_audio_and_mic_settings(&caps, st, &mut out);
    out
}

fn rgb_to_hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn parse_hex_rgb(hex: &str) -> Result<Rgb> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 {
        anyhow::bail!("Couleur hexadécimale attendue (#RRGGBB)");
    }
    Ok(Rgb::new(
        u8::from_str_radix(&s[0..2], 16)?,
        u8::from_str_radix(&s[2..4], 16)?,
        u8::from_str_radix(&s[4..6], 16)?,
    ))
}

fn append_mouse_settings(caps: &HxCapabilities, st: &HxState, out: &mut Vec<Setting>) {
    if caps.dpi_stages > 0 {
        let count = st.dpi_count.clamp(1, caps.dpi_stages) as usize;
        let stage_opts: Vec<ChoiceOption> = (0..count)
            .map(|i| ChoiceOption::new(i.to_string(), format!("Palier {} ({} DPI)", i + 1, st.dpi_stages[i])))
            .collect();
        out.push(
            Setting::choice(
                "hyperx.active_dpi_stage",
                "Palier DPI actif",
                Section::Performance,
                stage_opts,
                &st.active_dpi_stage.min((count - 1) as u8).to_string(),
            )
            .help("Sélectionne le palier de sensibilité actif."),
        );
        let cur_dpi = st.dpi_stages[st.active_dpi_stage.min(4) as usize] as f64;
        out.push(
            Setting::slider(
                "hyperx.dpi",
                "Sensibilité du palier actif",
                Section::Performance,
                (100.0, caps.max_dpi as f64, 50.0),
                "DPI",
                cur_dpi,
            )
            .help("Ajuste la sensibilité du palier sélectionné (pas de 50 DPI)."),
        );
        out.push(
            Setting::slider(
                "hyperx.dpi_count",
                "Nombre de paliers DPI actifs",
                Section::Performance,
                (1.0, caps.dpi_stages as f64, 1.0),
                "",
                count as f64,
            )
            .expert()
            .help("Nombre de paliers parcourus avec le bouton DPI de la souris."),
        );
        for i in 0..caps.dpi_stages as usize {
            out.push(
                Setting::slider(
                    &format!("hyperx.dpi_{}", i + 1),
                    &format!("Palier DPI {}", i + 1),
                    Section::Performance,
                    (100.0, caps.max_dpi as f64, 50.0),
                    "DPI",
                    st.dpi_stages[i] as f64,
                )
                .expert(),
            );
            out.push(
                Setting::color(
                    &format!("hyperx.dpi_color_{}", i + 1),
                    &format!("Couleur indicateur palier {}", i + 1),
                    Section::Performance,
                    &rgb_to_hex(st.dpi_colors[i]),
                )
                .expert(),
            );
        }
    }

    if !caps.polling_rates.is_empty() {
        out.push(
            Setting::choice(
                setting_ids::POLLING_RATE,
                "Fréquence d'envoi USB",
                Section::Performance,
                polling_rate_options(caps.polling_rates.iter().copied()),
                &st.polling_hz.to_string(),
            )
            .help("Fréquence de rafraîchissement entre le périphérique et le PC."),
        );
    }

    if caps.lod_and_snap {
        out.push(
            Setting::choice(
                "hyperx.lod",
                "Distance de décrochage (LOD)",
                Section::Performance,
                vec![
                    ChoiceOption::new("1", "Basse (1 mm)"),
                    ChoiceOption::new("2", "Haute (2 mm)"),
                ],
                if st.lod_2mm { "2" } else { "1" },
            )
            .expert(),
        );
        out.push(
            Setting::toggle(
                setting_ids::ANGLE_SNAPPING,
                "Alignement de trajectoire (Angle Snapping)",
                Section::Performance,
                st.angle_snap,
            )
            .expert(),
        );
    }

    if caps.angle_tuning {
        out.push(
            Setting::choice(
                "hyperx.angle_tuning",
                "Rotation du capteur (Angle Tuning)",
                Section::Performance,
                vec![
                    ChoiceOption::new("-30", "-30°"),
                    ChoiceOption::new("-10", "-10°"),
                    ChoiceOption::new("0", "0° (neutre)"),
                    ChoiceOption::new("15", "+15°"),
                    ChoiceOption::new("30", "+30°"),
                ],
                &st.angle_tune_deg.to_string(),
            )
            .expert()
            .help("Compense l'inclinaison naturelle de votre prise en main."),
        );
    }

    if caps.dart_save {
        out.push(
            Setting::toggle(
                "hyperx.save_onboard",
                "Enregistrer en mémoire interne",
                Section::Performance,
                false,
            )
            .expert()
            .help("Écrit les réglages courants dans la flash de la Pulsefire Dart (action explicite)."),
        );
    }
}

fn append_keyboard_settings(caps: &HxCapabilities, st: &HxState, out: &mut Vec<Setting>) {
    if caps.win_lock {
        out.push(Setting::toggle(
            setting_ids::WIN_LOCK,
            "Bloquer la touche Windows (Mode Jeu)",
            Section::Input,
            st.win_lock,
        ));
    }
    if caps.game_mode_keys {
        for (id, label, bit) in [
            ("hyperx.gm_alt_tab", "Mode Jeu : bloquer Alt + Tab", 1u8),
            ("hyperx.gm_alt_f4", "Mode Jeu : bloquer Alt + F4", 2u8),
            ("hyperx.gm_shift_tab", "Mode Jeu : bloquer Shift + Tab", 3u8),
            ("hyperx.gm_ctrl_esc", "Mode Jeu : bloquer Ctrl + Échap", 4u8),
        ] {
            out.push(
                Setting::toggle(id, label, Section::Input, (st.game_mode_mask & (1 << bit)) != 0)
                    .expert(),
            );
        }
    }
}

fn append_lighting_and_power_settings(caps: &HxCapabilities, st: &HxState, out: &mut Vec<Setting>) {
    if caps.brightness || caps.brightness_feature_pad {
        out.push(Setting::slider(
            setting_ids::HW_BRIGHTNESS,
            "Luminosité matérielle",
            Section::Lighting,
            (0.0, 100.0, 5.0),
            "%",
            st.brightness_pct as f64,
        ));
    }
    if caps.backlight_fade {
        out.push(
            Setting::choice(
                "hyperx.backlight_fade",
                "Extinction du rétroéclairage en veille",
                Section::Lighting,
                vec![
                    ChoiceOption::new("0", "Jamais"),
                    ChoiceOption::new("15", "15 secondes"),
                    ChoiceOption::new("30", "30 secondes"),
                    ChoiceOption::new("60", "1 minute"),
                    ChoiceOption::new("120", "2 minutes"),
                ],
                &st.backlight_fade_secs.to_string(),
            )
            .expert(),
        );
    }
    if caps.ambient_brightness {
        out.push(
            Setting::toggle(
                "hyperx.ambient_brightness",
                "Luminosité adaptative (capteur ambiant)",
                Section::Lighting,
                st.ambient_brightness,
            )
            .expert(),
        );
    }
    if caps.reverse_lights {
        out.push(
            Setting::toggle(
                "hyperx.reverse_lights",
                "Inverser les anneaux LED",
                Section::Lighting,
                st.reverse_lights,
            )
            .expert(),
        );
    }
    if caps.auto_sleep {
        out.push(Setting::choice(
            "hyperx.auto_sleep",
            "Extinction automatique sans fil",
            Section::Power,
            vec![
                ChoiceOption::new("0", "Jamais"),
                ChoiceOption::new("10", "10 minutes"),
                ChoiceOption::new("20", "20 minutes"),
                ChoiceOption::new("30", "30 minutes"),
            ],
            &st.auto_sleep_min.to_string(),
        ));
    }
    if caps.low_battery_alert {
        out.push(
            Setting::slider(
                "hyperx.low_battery",
                "Seuil d'alerte batterie faible",
                Section::Power,
                (5.0, 25.0, 5.0),
                "%",
                st.low_battery_pct as f64,
            )
            .expert(),
        );
    }
}

fn append_audio_and_mic_settings(caps: &HxCapabilities, st: &HxState, out: &mut Vec<Setting>) {
    if caps.sidetone_pct || caps.sidetone_signed_db {
        let cur_val = if !st.sidetone_enabled {
            0.0
        } else if caps.sidetone_signed_db {
            ((st.sidetone_db as i32 + 5).clamp(0, 10) * 10) as f64
        } else {
            st.sidetone_pct as f64
        };
        out.push(
            Setting::slider(
                setting_ids::SIDETONE,
                "Retour micro (Sidetone)",
                Section::Audio,
                (0.0, 100.0, if caps.sidetone_signed_db { 10.0 } else { 5.0 }),
                "%",
                cur_val,
            )
            .help("0 % coupe le retour micro ; au-delà, active et règle le niveau."),
        );
    } else if caps.sidetone_toggle {
        out.push(Setting::toggle(
            "hyperx.sidetone_enable",
            "Retour micro (Sidetone)",
            Section::Audio,
            st.sidetone_enabled,
        ));
    }

    if caps.mic_mute {
        out.push(Setting::toggle(
            "hyperx.mic_mute",
            "Couper le micro (Mute)",
            Section::Audio,
            st.mic_muted,
        ));
    }
    if caps.mic_gain {
        out.push(Setting::slider(
            "hyperx.mic_gain",
            "Volume / gain du micro",
            Section::Audio,
            (0.0, 100.0, 5.0),
            "%",
            st.mic_level as f64,
        ));
    }
    if caps.mic_polar_pattern > 0 {
        let (opts, cur) = if caps.mic_polar_pattern >= 4 {
            (
                vec![
                    ChoiceOption::new("stereo", "Stéréo"),
                    ChoiceOption::new("omni", "Omnidirectionnel"),
                    ChoiceOption::new("cardioid", "Cardioïde"),
                    ChoiceOption::new("bidir", "Bidirectionnel"),
                ],
                st.polar_pattern,
            )
        } else {
            let cur = if st.polar_pattern == "omni" { "omni" } else { "cardioid" };
            (
                vec![
                    ChoiceOption::new("cardioid", "Cardioïde"),
                    ChoiceOption::new("omni", "Omnidirectionnel"),
                ],
                cur,
            )
        };
        out.push(Setting::choice(
            "hyperx.polar_pattern",
            "Directivité du micro (Polar Pattern)",
            Section::Audio,
            opts,
            cur,
        ));
    }
    if caps.mic_high_pass {
        out.push(
            Setting::toggle(
                "hyperx.high_pass",
                "Filtre passe-haut micro",
                Section::Audio,
                st.high_pass,
            )
            .expert(),
        );
    }
    if caps.mic_monitor_balance || caps.universal_audio_full {
        out.push(Setting::slider(
            "hyperx.mic_balance",
            "Balance retour micro / PC",
            Section::Audio,
            (0.0, 100.0, 5.0),
            "%",
            st.balance_pct as f64,
        ));
    }
    if caps.universal_audio_full {
        out.push(
            Setting::slider(
                "hyperx.playback_level",
                "Volume de lecture casque",
                Section::Audio,
                (0.0, 100.0, 5.0),
                "%",
                st.playback_pct as f64,
            )
            .expert(),
        );
        out.push(Setting::slider(
            "hyperx.chatmix",
            "Balance Jeu / Chat",
            Section::Audio,
            (0.0, 100.0, 5.0),
            "%",
            st.chatmix_pct as f64,
        ));
    }
    if caps.noise_gate {
        out.push(Setting::toggle(
            "hyperx.noise_gate",
            "Réduction de bruit micro (Noise Gate)",
            Section::Audio,
            st.noise_gate,
        ));
    }
    if caps.voice_prompt {
        out.push(
            Setting::choice(
                "hyperx.voice_prompt",
                "Alertes sonores du casque",
                Section::Audio,
                vec![
                    ChoiceOption::new("beep", "Bips"),
                    ChoiceOption::new("voice", "Messages vocaux"),
                ],
                if st.voice_prompt { "voice" } else { "beep" },
            )
            .expert(),
        );
    }
    if caps.eq_preset {
        out.push(Setting::choice(
            "hyperx.eq_preset",
            "Égaliseur intégré",
            Section::Audio,
            vec![
                ChoiceOption::new("0", "Plat / Neutre"),
                ChoiceOption::new("1", "Basses renforcées"),
                ChoiceOption::new("2", "FPS / Précision"),
                ChoiceOption::new("3", "Voix"),
            ],
            &st.eq_preset.to_string(),
        ));
    }
    if caps.eq_10band != EqHwMode::None {
        for (k, freq) in EQ_10_FREQS.iter().enumerate() {
            let lbl = if *freq >= 1000 {
                format!("Égaliseur {} kHz", freq / 1000)
            } else {
                format!("Égaliseur {} Hz", freq)
            };
            out.push(
                Setting::slider(
                    &format!("hyperx.eq_band_{k}"),
                    &lbl,
                    Section::Audio,
                    (-12.0, 12.0, 0.5),
                    "dB",
                    st.eq_bands[k] as f64,
                )
                .expert(),
            );
        }
    }
}

fn univ_audio_cfg(st: &HxState) -> UnivAudioConfig {
    UnivAudioConfig {
        mic_muted: st.mic_muted,
        mic_level: st.mic_level,
        sidetone_on: st.sidetone_enabled,
        sidetone_pct: st.sidetone_pct,
        balance_pct: st.balance_pct,
        chatmix_pct: st.chatmix_pct,
        playback_pct: st.playback_pct,
        playback_eq_idx: st.eq_preset,
        reverse_light: st.reverse_lights,
        ..UnivAudioConfig::default()
    }
}

fn eq_preset_curve(idx: u8) -> [f32; 10] {
    match idx {
        1 => [5.0, 4.5, 3.0, 1.5, 0.0, 0.0, 0.0, 0.5, 1.0, 1.5],
        2 => [-2.0, -1.5, -1.0, 0.0, 1.0, 2.0, 3.5, 4.0, 3.5, 2.0],
        3 => [-2.5, -2.0, -1.0, 1.0, 2.5, 3.5, 3.0, 1.5, 0.0, -1.0],
        _ => [0.0; 10],
    }
}

/// Apply one validated `Setting` update to the device via its HyperX HID protocol.
pub fn apply_device_setting(
    io: &mut dyn HxIo,
    def: &'static HxDeviceDef,
    st: &mut HxState,
    id: &str,
    value: &Value,
) -> Result<()> {
    let caps = def.caps();
    match id {
        "hyperx.active_dpi_stage" => {
            let s = value.as_str().ok_or_else(|| anyhow::anyhow!("valeur palier attendue"))?;
            let max_idx = st.dpi_count.clamp(1, caps.dpi_stages).saturating_sub(1);
            st.active_dpi_stage = s.parse::<u8>()?.min(max_idx);
            flush_mouse_dpi(io, def, st)?;
        }
        "hyperx.dpi" => {
            let idx = st.active_dpi_stage.min(4) as usize;
            st.dpi_stages[idx] = snap_dpi(value, caps.max_dpi)?;
            flush_mouse_dpi(io, def, st)?;
        }
        "hyperx.dpi_count" => {
            let c = value.as_f64().ok_or_else(|| anyhow::anyhow!("entier attendu"))?.round() as u8;
            st.dpi_count = c.clamp(1, caps.dpi_stages.max(1));
            st.active_dpi_stage = st.active_dpi_stage.min(st.dpi_count - 1);
            flush_mouse_dpi(io, def, st)?;
        }
        "hyperx.dpi_1" | "hyperx.dpi_2" | "hyperx.dpi_3" | "hyperx.dpi_4" | "hyperx.dpi_5" => {
            let idx = id[10..].parse::<usize>()? - 1;
            st.dpi_stages[idx] = snap_dpi(value, caps.max_dpi)?;
            flush_mouse_dpi(io, def, st)?;
        }
        "hyperx.dpi_color_1"
        | "hyperx.dpi_color_2"
        | "hyperx.dpi_color_3"
        | "hyperx.dpi_color_4"
        | "hyperx.dpi_color_5" => {
            let idx = id[16..].parse::<usize>()? - 1;
            let hex = value.as_str().ok_or_else(|| anyhow::anyhow!("couleur #RRGGBB attendue"))?;
            st.dpi_colors[idx] = parse_hex_rgb(hex)?;
            flush_mouse_dpi(io, def, st)?;
        }
        setting_ids::POLLING_RATE => {
            let hz = value.as_str().ok_or_else(|| anyhow::anyhow!("fréquence attendue"))?.parse::<u16>()?;
            st.polling_hz = hz;
            flush_polling_rate(io, def, st)?;
        }
        "hyperx.lod" => {
            st.lod_2mm = value.as_str().ok_or_else(|| anyhow::anyhow!("LOD attendu"))? == "2";
            flush_mouse_sensor(io, st)?;
        }
        setting_ids::ANGLE_SNAPPING => {
            st.angle_snap = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            flush_mouse_sensor(io, st)?;
        }
        "hyperx.angle_tuning" => {
            let s = value.as_str().ok_or_else(|| anyhow::anyhow!("angle attendu"))?;
            st.angle_tune_deg = s.parse::<i16>()?;
            flush_mouse_sensor(io, st)?;
        }
        "hyperx.save_onboard" => {
            if value.as_bool().unwrap_or(false) && matches!(def.family, ProtocolFamily::PulsefireDart) {
                let mut sv = [0u8; 65];
                sv[1] = 0xDE;
                sv[2] = 0xFF;
                io.write_output(&sv)?;
            }
        }
        setting_ids::WIN_LOCK => {
            st.win_lock = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            if st.win_lock {
                st.game_mode_mask |= 1;
            } else {
                st.game_mode_mask &= !1;
            }
            flush_game_mode(io, st)?;
        }
        "hyperx.gm_alt_tab" | "hyperx.gm_alt_f4" | "hyperx.gm_shift_tab" | "hyperx.gm_ctrl_esc" => {
            let on = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            let bit = match id {
                "hyperx.gm_alt_tab" => 1u8,
                "hyperx.gm_alt_f4" => 2u8,
                "hyperx.gm_shift_tab" => 3u8,
                _ => 4u8,
            };
            if on {
                st.game_mode_mask |= 1 << bit;
            } else {
                st.game_mode_mask &= !(1 << bit);
            }
            flush_game_mode(io, st)?;
        }
        "hyperx.backlight_fade" => {
            st.backlight_fade_secs = value.as_str().ok_or_else(|| anyhow::anyhow!("durée attendue"))?.parse::<u16>()?;
            io.write_output(&univ_set_backlight_fade_pkt(st.tio_slot, 1, st.backlight_fade_secs))?;
        }
        "hyperx.ambient_brightness" => {
            st.ambient_brightness = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            io.write_output(&univ_set_ambient_brightness_pkt(st.tio_slot, 1, st.ambient_brightness))?;
        }
        "hyperx.reverse_lights" => {
            st.reverse_lights = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            if caps.universal {
                io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)))?;
            } else if matches!(def.family, ProtocolFamily::DuoCast) {
                io.send_feature(&duocast_set_lights_info_pkt(st.brightness_pct, st.reverse_lights))?;
            }
        }
        "hyperx.low_battery" => {
            let pct = value.as_f64().ok_or_else(|| anyhow::anyhow!("seuil attendu"))?.round().clamp(5.0, 25.0) as u8;
            st.low_battery_pct = pct;
            if caps.universal {
                io.write_output(&univ_set_low_battery_pkt(st.tio_slot, pct, true))?;
            } else if matches!(
                def.family,
                ProtocolFamily::PulsefireDart | ProtocolFamily::PulsefireHasteWireless
            ) {
                io.write_output(&legacy_mouse_low_battery_pkt(pct))?;
            }
        }
        setting_ids::SIDETONE => {
            let pct = value.as_f64().ok_or_else(|| anyhow::anyhow!("niveau attendu"))?.round() as u8;
            st.sidetone_enabled = pct > 0;
            st.sidetone_pct = pct.min(100);
            st.sidetone_db = ((pct.min(100) as i32 / 10) - 5).clamp(-5, 5) as i8;
            flush_headset_sidetone(io, def, st)?;
        }
        "hyperx.sidetone_enable" => {
            st.sidetone_enabled = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            flush_headset_sidetone(io, def, st)?;
        }
        "hyperx.mic_mute" => {
            st.mic_muted = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            flush_mic_mute(io, def, st)?;
        }
        "hyperx.mic_gain" => {
            st.mic_level = value.as_f64().ok_or_else(|| anyhow::anyhow!("niveau attendu"))?.round().clamp(0.0, 100.0) as u8;
            flush_mic_gain(io, def, st)?;
        }
        "hyperx.polar_pattern" => {
            let s = value.as_str().ok_or_else(|| anyhow::anyhow!("directivité attendue"))?;
            flush_polar_pattern(io, def, st, s)?;
        }
        "hyperx.high_pass" => {
            st.high_pass = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            match def.family {
                ProtocolFamily::QuadCast2 | ProtocolFamily::QuadCast2S => {
                    io.write_output(&quadcast2_set_highpass_pkt(st.high_pass))?;
                }
                ProtocolFamily::DuoCast => {
                    io.send_feature(&duocast_set_highpass_pkt(st.high_pass))?;
                }
                _ => {}
            }
        }
        "hyperx.mic_balance" => {
            st.balance_pct = value.as_f64().ok_or_else(|| anyhow::anyhow!("balance attendue"))?.round().clamp(0.0, 100.0) as u8;
            if caps.universal {
                io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)))?;
            } else if matches!(def.family, ProtocolFamily::QuadCast2S) {
                io.write_output(&quadcast2s_set_balance_pkt(st.balance_pct))?;
            }
        }
        "hyperx.playback_level" => {
            st.playback_pct = value.as_f64().ok_or_else(|| anyhow::anyhow!("volume attendu"))?.round().clamp(0.0, 100.0) as u8;
            io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)))?;
        }
        "hyperx.chatmix" => {
            st.chatmix_pct = value.as_f64().ok_or_else(|| anyhow::anyhow!("mix attendu"))?.round().clamp(0.0, 100.0) as u8;
            io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)))?;
        }
        "hyperx.noise_gate" => {
            st.noise_gate = value.as_bool().ok_or_else(|| anyhow::anyhow!("booléen attendu"))?;
            io.write_output(&univ_set_noise_gate_pkt(st.tio_slot, st.noise_gate))?;
        }
        "hyperx.voice_prompt" => {
            st.voice_prompt = value.as_str().ok_or_else(|| anyhow::anyhow!("choix attendu"))? == "voice";
            io.write_output(&univ_set_voice_prompt_pkt(st.tio_slot, st.voice_prompt))?;
        }
        "hyperx.eq_preset" => {
            let s = value.as_str().ok_or_else(|| anyhow::anyhow!("EQ attendu"))?;
            st.eq_preset = s.parse::<u8>()?.min(3);
            st.eq_bands = eq_preset_curve(st.eq_preset);
            if caps.universal {
                io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)))?;
            } else {
                flush_eq_bands(io, def, st, None, true)?;
            }
        }
        _ if id.starts_with("hyperx.eq_band_") => {
            let k = id[15..].parse::<usize>()?;
            if k >= 10 {
                anyhow::bail!("Bande EQ hors limites : {k}");
            }
            let db = value.as_f64().ok_or_else(|| anyhow::anyhow!("dB numérique attendu"))?;
            st.eq_bands[k] = ((db * 2.0).round() as f32 / 2.0).clamp(-12.0, 12.0);
            flush_eq_bands(io, def, st, Some(k), false)?;
        }
        "hyperx.auto_sleep" => {
            st.auto_sleep_min = value.as_str().ok_or_else(|| anyhow::anyhow!("délai attendu"))?.parse::<u16>()?;
            flush_auto_sleep(io, def, st)?;
        }
        _ => anyhow::bail!("Réglage HyperX inconnu : {id}"),
    }
    Ok(())
}

fn snap_dpi(value: &Value, max_dpi: u16) -> Result<u16> {
    let v = value.as_f64().ok_or_else(|| anyhow::anyhow!("DPI numérique attendu"))?;
    Ok((((v.round() as i32).clamp(100, max_dpi as i32) / 50) * 50) as u16)
}

fn flush_mouse_sensor(io: &mut dyn HxIo, st: &HxState) -> Result<()> {
    io.write_output(&univ_set_mouse_sensor_full_pkt(
        st.tio_slot,
        1,
        st.angle_snap,
        angle_tune_wire_byte(st.angle_tune_deg),
        st.lod_2mm,
    ))
}

fn flush_game_mode(io: &mut dyn HxIo, st: &HxState) -> Result<()> {
    io.write_output(&univ_set_game_mode_bits_pkt(
        st.tio_slot,
        1,
        st.game_mode_mask != 0,
        st.game_mode_mask,
    ))
}

fn flush_polling_rate(io: &mut dyn HxIo, def: &'static HxDeviceDef, st: &HxState) -> Result<()> {
    if def.caps().universal {
        if def.kind == DeviceKind::Mouse {
            return flush_mouse_dpi(io, def, st);
        }
        return io.write_output(&univ_set_mouse_dpi_pkt(st.tio_slot, 1, st.polling_hz, &[], 0));
    }
    match def.family {
        ProtocolFamily::PulsefireDart
        | ProtocolFamily::PulsefireFpsPro2
        | ProtocolFamily::PulsefireHasteWireless => io.write_output(&legacy_mouse_polling_pkt(st.polling_hz)),
        ProtocolFamily::PulsefireHaste => flush_mouse_dpi(io, def, st),
        _ => Ok(()),
    }
}

fn flush_mouse_dpi(io: &mut dyn HxIo, def: &'static HxDeviceDef, st: &HxState) -> Result<()> {
    let count = st.dpi_count.clamp(1, 5) as usize;
    let stages: Vec<(u16, Rgb)> = st.dpi_stages.iter().copied().zip(st.dpi_colors).take(count).collect();
    if def.caps().universal {
        return io.write_output(&univ_set_mouse_dpi_pkt(
            st.tio_slot,
            1,
            st.polling_hz,
            &stages,
            st.active_dpi_stage,
        ));
    }
    match def.family {
        ProtocolFamily::PulsefireDart
        | ProtocolFamily::PulsefireFpsPro2
        | ProtocolFamily::PulsefireHasteWireless => {
            for pkt in legacy_mouse_dpi_pkts(&stages, st.active_dpi_stage) {
                io.write_output(&pkt)?;
            }
            if matches!(def.family, ProtocolFamily::PulsefireHasteWireless) {
                let mut commit = [0u8; 65];
                commit[1] = 0xDE;
                commit[2] = 0x03;
                io.write_output(&commit)?;
            }
        }
        ProtocolFamily::PulsefireHaste => {
            for pkt in legacy_haste_wired_profile_pkts(st.polling_hz, st.brightness_pct, &stages) {
                io.write_output(&pkt)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn flush_headset_sidetone(io: &mut dyn HxIo, def: &'static HxDeviceDef, st: &HxState) -> Result<()> {
    let on = u8::from(st.sidetone_enabled);
    if def.caps().universal {
        return io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)));
    }
    match def.family {
        ProtocolFamily::CloudAlphaWireless => {
            io.write_output(&cloud_alpha_wireless_pkt(0x10, on))?;
            io.write_output(&cloud_alpha_wireless_pkt(0x11, st.sidetone_pct))?;
        }
        ProtocolFamily::CloudIIWirelessDts | ProtocolFamily::CloudStinger2Wireless => {
            io.write_output(&cloud2_dts_pkt(0x21, on))?;
            io.write_output(&cloud2_dts_pkt(0x23, st.sidetone_pct))?;
        }
        ProtocolFamily::CloudIIWireless => io.write_output(&cloud2_wireless_pkt(0x19, on))?,
        ProtocolFamily::CloudFlightS => {
            io.write_output(&cloud2_wireless_pkt(0x19, on))?;
            io.write_output(&cloud2_wireless_pkt(0x0C, st.sidetone_pct))?;
        }
        ProtocolFamily::CloudIIICoreWireless => {
            io.write_output(&cloud3_wireless_pkt(0x01, on))?;
            io.write_output(&cloud3_wireless_pkt(0x05, st.sidetone_db as u8))?;
        }
        ProtocolFamily::CloudIIISWireless => io.write_output(&cloud3s_wireless_pkt(0x0D, on, 0))?,
        ProtocolFamily::CloudIII | ProtocolFamily::CloudAlphaS => {
            let mut pkt = [0u8; 65];
            pkt[0] = 0x06;
            pkt[1] = if matches!(def.family, ProtocolFamily::CloudIII) { 0x01 } else { 0x00 };
            pkt[2] = on;
            let _ = io.write_output(&pkt);
        }
        ProtocolFamily::DuoCast => io.send_feature(&duocast_set_sidetone_pkt(st.sidetone_enabled))?,
        _ => {}
    }
    Ok(())
}

fn flush_mic_mute(io: &mut dyn HxIo, def: &'static HxDeviceDef, st: &HxState) -> Result<()> {
    let m = u8::from(st.mic_muted);
    if def.caps().universal {
        return io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)));
    }
    match def.family {
        ProtocolFamily::CloudAlphaWireless => io.write_output(&cloud_alpha_wireless_pkt(0x15, m))?,
        ProtocolFamily::CloudIIICoreWireless => io.write_output(&cloud3_wireless_pkt(0x03, m))?,
        ProtocolFamily::CloudIIISWireless => io.write_output(&cloud3s_wireless_pkt(0x01, m, 0))?,
        ProtocolFamily::QuadCast2 | ProtocolFamily::QuadCast2S => {
            io.write_output(&quadcast2_set_mute_pkt(st.mic_muted))?;
        }
        ProtocolFamily::SoloCast => io.send_feature(&solocast_set_mute_pkt(st.mic_muted))?,
        _ => {}
    }
    Ok(())
}

fn flush_mic_gain(io: &mut dyn HxIo, def: &'static HxDeviceDef, st: &HxState) -> Result<()> {
    if def.caps().universal {
        return io.write_output(&univ_set_generic_audio_full_pkt(st.tio_slot, &univ_audio_cfg(st)));
    }
    match def.family {
        ProtocolFamily::QuadCast2S => io.write_output(&quadcast2s_set_mix_pkt(st.mic_level)),
        ProtocolFamily::DuoCast => io.send_feature(&duocast_set_gain_pkt(st.mic_level)),
        _ => Ok(()),
    }
}

fn flush_polar_pattern(
    io: &mut dyn HxIo,
    def: &'static HxDeviceDef,
    st: &mut HxState,
    choice: &str,
) -> Result<()> {
    let (canon, qc_byte) = match choice {
        "stereo" => ("stereo", 0u8),
        "omni" => ("omni", 1u8),
        "bidir" => ("bidir", 3u8),
        _ => ("cardioid", 2u8),
    };
    st.polar_pattern = canon;
    match def.family {
        ProtocolFamily::QuadCast2 | ProtocolFamily::QuadCast2S => {
            io.write_output(&quadcast2_set_polar_pkt(qc_byte))
        }
        ProtocolFamily::DuoCast => io.send_feature(&duocast_set_polar_pkt(canon != "omni")),
        _ => Ok(()),
    }
}

fn flush_eq_bands(
    io: &mut dyn HxIo,
    def: &'static HxDeviceDef,
    st: &HxState,
    band_only: Option<usize>,
    is_preset_select: bool,
) -> Result<()> {
    match def.caps().eq_10band {
        EqHwMode::Universal0x66 => {
            io.write_output(&univ_set_onboard_eq_pkt(
                st.tio_slot,
                1,
                true,
                st.eq_preset,
                &st.eq_bands,
            ))
        }
        EqHwMode::Cloud3S => {
            if is_preset_select {
                io.write_output(&cloud3s_eq_index_pkt(st.eq_preset))
            } else if let Some(k) = band_only {
                io.write_output(&cloud3s_eq_band_pkt(k as u8, st.eq_bands[k]))
            } else {
                Ok(())
            }
        }
        EqHwMode::TuneEqAlphaS | EqHwMode::TuneEqFlightS => {
            let is_alpha = matches!(def.caps().eq_10band, EqHwMode::TuneEqAlphaS);
            for pkt in cloud_tune_eq_pkts(is_alpha, &st.eq_bands) {
                io.write_output(&pkt)?;
            }
            Ok(())
        }
        EqHwMode::None => Ok(()),
    }
}

/// Apply legacy wired/feature pad & microphone hardware brightness updates.
pub fn apply_legacy_brightness_hw(
    io: &mut dyn HxIo,
    def: &'static HxDeviceDef,
    st: &HxState,
) -> Result<()> {
    let raw = ((u16::from(st.brightness_pct) * 255 + 50) / 100).min(255) as u8;
    match def.family {
        ProtocolFamily::OriginsCore => {
            let mut pkt = [0u8; 65];
            pkt[1] = 0xA7;
            pkt[4] = 0x01;
            pkt[5] = raw;
            io.write_output(&pkt)
        }
        ProtocolFamily::Report0x44Eve1800 => {
            let mut pkt = [0u8; 65];
            pkt[0] = 0x40;
            pkt[1] = 0x01;
            pkt[4] = raw;
            io.write_output(&pkt)
        }
        ProtocolFamily::DuoCast => {
            io.send_feature(&duocast_set_lights_info_pkt(st.brightness_pct, st.reverse_lights))
        }
        ProtocolFamily::Mat0x81 { .. } if def.caps().brightness_feature_pad => {
            for pkt in fury_ultra_brightness_pkts(st.brightness_pct) {
                io.send_feature(&pkt)?;
            }
            Ok(())
        }
        ProtocolFamily::PulsefireHaste => flush_mouse_dpi(io, def, st),
        _ => Ok(()),
    }
}

fn flush_auto_sleep(io: &mut dyn HxIo, def: &'static HxDeviceDef, st: &HxState) -> Result<()> {
    let min = st.auto_sleep_min as u8;
    if def.caps().universal {
        return io.write_output(&univ_set_auto_sleep_pkt(st.tio_slot, 1, st.auto_sleep_min));
    }
    match def.family {
        ProtocolFamily::CloudAlphaWireless => io.write_output(&cloud_alpha_wireless_pkt(0x12, min))?,
        ProtocolFamily::CloudIIWirelessDts | ProtocolFamily::CloudStinger2Wireless => {
            io.write_output(&cloud2_dts_pkt(0x22, min))?;
        }
        ProtocolFamily::CloudIIWireless | ProtocolFamily::CloudFlightS => {
            io.write_output(&cloud2_wireless_pkt(0x18, min))?;
        }
        ProtocolFamily::CloudIIICoreWireless => io.write_output(&cloud3_wireless_pkt(0x02, min))?,
        ProtocolFamily::CloudIIISWireless => io.write_output(&cloud3s_auto_sleep_pkt(st.auto_sleep_min))?,
        _ => {}
    }
    Ok(())
}

/// Apply hardware brightness to a Universal device (`0x40`).
pub fn apply_universal_brightness(io: &mut dyn HxIo, st: &HxState) -> Result<()> {
    io.write_output(&univ_set_brightness_pkt(st.tio_slot, 1, st.brightness_pct))
}

/// Apply direct per-LED / single-zone frame to a Universal device (`0x44`).
pub fn send_universal_frame(
    io: &mut dyn HxIo,
    def: &'static HxDeviceDef,
    st: &HxState,
    colors: &[Rgb],
) -> Result<()> {
    let n = def.led_count.max(1);
    let mut frame = vec![Rgb::BLACK; n];
    for (dst, src) in frame.iter_mut().zip(colors.iter()) {
        *dst = *src;
    }
    for pkt in univ_frame_light_pkts(st.tio_slot, &frame) {
        io.write_output(&pkt)?;
    }
    Ok(())
}

/// Discover all HyperX peripherals matching `DEVICES`, deduplicating multi-collection HID entries.
pub fn discover_hyperx_devices() -> Vec<Box<dyn Device>> {
    let Ok(api) = HidApi::new() else {
        return Vec::new();
    };
    let mut best: BTreeMap<(u16, u16, String), (i32, &hidapi::DeviceInfo, &'static HxDeviceDef)> =
        BTreeMap::new();

    let all: Vec<_> = api
        .device_list()
        .filter(|d| matches!(d.vendor_id(), KINGSTON_VID | HP_HYPERX_VID))
        .collect();

    for d in &all {
        let Some(def) = find_device(d.vendor_id(), d.product_id()) else {
            continue;
        };
        let score = match_score(def, d.interface_number(), d.usage_page(), d.usage());
        if score < 0 {
            continue;
        }
        let path_str = d.path().to_string_lossy().into_owned();
        let dev_group = d
            .serial_number()
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                path_str
                    .split("&MI_")
                    .next()
                    .unwrap_or(&path_str)
                    .to_string()
            });
        let key = (d.vendor_id(), d.product_id(), dev_group);
        let replace = best.get(&key).is_none_or(|&(old_score, _, _)| score > old_score);
        if replace {
            best.insert(key, (score, *d, def));
        }
    }

    let mut out: Vec<Box<dyn Device>> = Vec::new();
    for ((vid, pid, _), (_, d, def)) in best {
        let path_tag = d.path().to_string_lossy().into_owned();
        match d.open_device(&api) {
            Ok(hd) => {
                if matches!(def.family, ProtocolFamily::UniversalTioDongle) {
                    let shared = Arc::new(Mutex::new(HidHxIo(hd)));
                    spawn_tio_children(def, shared, &path_tag, &mut out);
                } else {
                    debug!(name = def.name, vid, pid, "hyperx device opened");
                    let mut dev = HyperXDevice::new(def, Box::new(HidHxIo(hd)), path_tag);
                    let _ = dev.probe_on_discover();
                    out.push(Box::new(dev));
                }
            }
            Err(e) => warn!(name = def.name, vid, pid, error = %e, "hyperx open failed"),
        }
    }
    out
}

fn spawn_tio_children(
    def: &'static HxDeviceDef,
    shared: Arc<Mutex<HidHxIo>>,
    path_tag: &str,
    out: &mut Vec<Box<dyn Device>>,
) {
    let mut found_child = false;
    let mut buf = [0u8; 64];
    for slot in 1..=3u8 {
        let mut io = SharedHxIo(shared.clone());
        if let Some(n) = exchange_out(&mut io, &univ_get_device_info_pkt(slot), &mut buf) {
            if let Some(info) = parse_univ_device_info(&buf[..n]) {
                if info.primary_pid != 0 {
                    let child_def = find_device(HP_HYPERX_VID, info.primary_pid).unwrap_or(def);
                    let mut dev = HyperXDevice::new(
                        child_def,
                        Box::new(SharedHxIo(shared.clone())),
                        format!("{path_tag}#slot{slot}"),
                    );
                    dev.set_tio_slot(slot, tio_child_name(info.primary_pid).map(ToOwned::to_owned), info);
                    let _ = dev.probe_on_discover();
                    out.push(Box::new(dev));
                    found_child = true;
                }
            }
        }
    }
    if !found_child {
        let mut dev = HyperXDevice::new(def, Box::new(SharedHxIo(shared)), path_tag.to_string());
        let _ = dev.probe_on_discover();
        out.push(Box::new(dev));
    }
}

/// Format diagnostic lines for UI expert view cleanly.
pub fn build_diagnostic_lines(def: &'static HxDeviceDef, st: &HxState, path_tag: &str) -> Vec<String> {
    let mut lines = vec![
        format!("USB {:04X}:{:04X} (interface {})", def.vid, def.pid, def.interface),
        format!("Protocole : {:?}", def.family),
        format!("Chemin HID : {path_tag}"),
    ];
    if let Some(info) = &st.univ_info {
        lines.push(format!(
            "Universel MCU={} | LED={} | FW={:#06X}",
            info.mcu, info.supported_leds, info.primary_fw
        ));
    }
    if st.tio_slot > 0 {
        lines.push(format!("Slot dongle 3-en-1 : {}", st.tio_slot));
    }
    lines
}
