//! Clean-room packet builders and parsers from `docs/protocols/hyperx_ngenuity.md`.

use opengear_core::Rgb;

/// Parsed Universal device identity / capability summary (`0x11` response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnivDeviceInfo {
    pub vid: u16,
    pub primary_pid: u16,
    pub secondary_pid: u16,
    pub primary_fw: u16,
    pub secondary_fw: u16,
    pub wireless: bool,
    pub category: u8,
    pub supported_leds: u8,
    pub mcu: u8,
}

/// Encode sub-report byte with 2-bit TIO 3-in-1 child slot (`0..=3`) in bits 7..6.
pub const fn univ_sub_byte(sub_id: u8, tio_slot: u8) -> u8 {
    (sub_id & 0x3F) | ((tio_slot & 0x03) << 6)
}

/// Decode `(sub_id, tio_slot)` from byte 1 of a Universal response.
pub const fn split_univ_sub_byte(b: u8) -> (u8, u8) {
    (b & 0x3F, (b & 0xC0) >> 6)
}

fn univ_buf(report_id: u8, sub_id: u8, tio_slot: u8) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0] = report_id;
    b[1] = univ_sub_byte(sub_id, tio_slot);
    b
}

/// Universal `0x10` device info query (64 bytes).
pub fn univ_get_device_info_pkt(tio_slot: u8) -> [u8; 64] {
    univ_buf(0x10, 0x01, tio_slot)
}

/// Parse Universal `0x11` device info response.
pub fn parse_univ_device_info(resp: &[u8]) -> Option<UnivDeviceInfo> {
    if resp.len() < 56 || resp[0] != 0x11 {
        return None;
    }
    let cat_raw = resp[53];
    Some(UnivDeviceInfo {
        vid: u16::from_le_bytes([resp[35], resp[36]]),
        primary_pid: u16::from_le_bytes([resp[41], resp[42]]),
        primary_fw: u16::from_le_bytes([resp[43], resp[44]]),
        secondary_pid: u16::from_le_bytes([resp[45], resp[46]]),
        secondary_fw: u16::from_le_bytes([resp[47], resp[48]]),
        wireless: (cat_raw & 0x80) != 0,
        category: cat_raw & 0x7F,
        supported_leds: resp[54],
        mcu: resp[55],
    })
}

/// Universal `0x50` battery query (64 bytes).
pub fn univ_get_battery_pkt(tio_slot: u8) -> [u8; 64] {
    univ_buf(0x50, 0x02, tio_slot)
}

/// Parse Universal `0x51` battery response -> `(battery_pct, Option<charging>)`.
pub fn parse_univ_battery(resp: &[u8]) -> Option<(u8, Option<bool>)> {
    if resp.len() < 4 || resp[0] != 0x51 {
        return None;
    }
    let pct = resp[2].min(100);
    let charging = match resp[3] {
        0 => Some(false),
        1 | 2 => Some(true),
        _ => None,
    };
    Some((pct, charging))
}

/// Universal `0x30` game mode / Win-lock configuration (`bitmask`: 1=Win, 2=Alt+Tab, 4=Alt+F4, 8=Shift+Tab, 16=Ctrl+Esc).
pub fn univ_set_game_mode_bits_pkt(
    tio_slot: u8,
    profile_mask: u16,
    enabled: bool,
    bitmask: u8,
) -> [u8; 64] {
    let mut b = univ_buf(0x30, 0x01, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = u8::from(enabled);
    b[5] = bitmask & 0x1F;
    b
}

/// Universal `0x30` game mode / Win-lock convenience wrapper.
pub fn univ_set_game_mode_pkt(
    tio_slot: u8,
    profile_mask: u16,
    enabled: bool,
    win_lock: bool,
) -> [u8; 64] {
    univ_set_game_mode_bits_pkt(tio_slot, profile_mask, enabled, u8::from(win_lock))
}

/// Convert Hz to Universal `PollingRates` wire code.
pub const fn univ_polling_code(hz: u16) -> u8 {
    match hz {
        8000 => 1,
        4000 => 2,
        2000 => 4,
        1000 => 8,
        500 => 16,
        250 => 32,
        125 => 64,
        _ => 8,
    }
}

/// Universal `0x32` mouse DPI & polling rate configuration (up to 5 stages).
pub fn univ_set_mouse_dpi_pkt(
    tio_slot: u8,
    profile_mask: u16,
    polling_hz: u16,
    stages: &[(u16, Rgb)],
    active_idx: u8,
) -> [u8; 64] {
    let mut b = univ_buf(0x32, 0x01, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = univ_polling_code(polling_hz);
    let count = stages.len().min(5);
    let enable_mask = if count == 0 { 0 } else { (1u8 << count) - 1 };
    b[5] = enable_mask;
    b[6] = active_idx.min(count.saturating_sub(1) as u8);
    for (i, &(dpi, col)) in stages.iter().take(5).enumerate() {
        let off = 7 + i * 5;
        let reg = (dpi.clamp(50, 26000) / 50).saturating_sub(1);
        let le = reg.to_le_bytes();
        b[off] = le[0];
        b[off + 1] = le[1];
        b[off + 2] = col.r;
        b[off + 3] = col.g;
        b[off + 4] = col.b;
    }
    b
}

/// Universal `0x36` mouse sensor configuration (angle snapping + angle tuning + LOD).
pub fn univ_set_mouse_sensor_full_pkt(
    tio_slot: u8,
    profile_mask: u16,
    angle_snapping: bool,
    angle_tune: u8,
    lod_2mm: bool,
) -> [u8; 64] {
    let mut b = univ_buf(0x36, 0x01, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = u8::from(angle_snapping);
    b[5] = angle_tune;
    b[6] = u8::from(angle_tune != 0);
    b[7] = if lod_2mm { 2 } else { 0 };
    b
}

/// Universal `0x36` mouse sensor configuration (angle snapping + lift-off distance).
pub fn univ_set_mouse_sensor_pkt(
    tio_slot: u8,
    profile_mask: u16,
    angle_snapping: bool,
    lod_2mm: bool,
) -> [u8; 64] {
    univ_set_mouse_sensor_full_pkt(tio_slot, profile_mask, angle_snapping, 0, lod_2mm)
}

/// Convert signed degree choice (`"-30"`, `"-10"`, `"0"`, `"15"`, `"30"`) to `AngleTunes` byte.
pub fn angle_tune_wire_byte(deg: i16) -> u8 {
    match deg {
        -30 => 226,
        -10 => 246,
        15 => 15,
        30 => 30,
        _ => 0,
    }
}

/// Universal `0x42` preset hardware lighting (`Layer1Effect`), single-layer configuration.
pub fn univ_preset_light_pkt(
    tio_slot: u8,
    profile_mask: u16,
    mode: u8,
    speed: u16,
    direction: u8,
    color_type: u8,
    colors: &[Rgb],
) -> [u8; 64] {
    let mut b = univ_buf(0x42, 0x02, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = mode;
    let sp = speed.to_le_bytes();
    b[5] = sp[0];
    b[6] = sp[1];
    b[7] = direction;
    b[8] = color_type;
    for (i, c) in colors.iter().take(3).enumerate() {
        let off = 9 + i * 3;
        b[off] = c.r;
        b[off + 1] = c.g;
        b[off + 2] = c.b;
    }
    b
}

/// Universal `0x46 0x01` keyboard backlight idle fade timeout (`secs == 0` disables idle off).
pub fn univ_set_backlight_fade_pkt(tio_slot: u8, profile_mask: u16, secs: u16) -> [u8; 64] {
    let mut b = univ_buf(0x46, 0x01, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = u8::from(secs > 0);
    let s = secs.to_le_bytes();
    b[5] = s[0];
    b[6] = s[1];
    b
}

/// Universal `0x46 0x03` ambient light sensor brightness control (Alloy Rise series).
pub fn univ_set_ambient_brightness_pkt(tio_slot: u8, profile_mask: u16, enabled: bool) -> [u8; 64] {
    let mut b = univ_buf(0x46, 0x03, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = u8::from(enabled);
    // Standard 25/50/75/100 step curve from `BrightnessAmbientCMD.GetAmbientLevels`.
    b[5..9].copy_from_slice(&[25, 50, 75, 100]);
    b
}

/// Universal `0x50 0x01` low-battery warning threshold (`5..=25%`).
pub fn univ_set_low_battery_pkt(tio_slot: u8, threshold_pct: u8, save: bool) -> [u8; 64] {
    let mut b = univ_buf(0x50, 0x01, tio_slot);
    b[2] = threshold_pct.clamp(5, 25);
    b[3] = u8::from(save);
    b
}

/// Universal `0x40` lighting brightness (`0..=100`% mapped to `0..=255`).
pub fn univ_set_brightness_pkt(tio_slot: u8, profile_mask: u16, pct: u8) -> [u8; 64] {
    let mut b = univ_buf(0x40, 0x01, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = ((pct.min(100) as u16 * 255) / 100) as u8;
    b
}

/// Universal `0x44` direct per-LED frame packets (1 header `sub=3` + 1 packet per LED `sub=4`).
pub fn univ_frame_light_pkts(tio_slot: u8, colors: &[Rgb]) -> Vec<[u8; 64]> {
    let n = colors.len().min(128);
    let mut out = Vec::with_capacity(n + 1);
    let mut hdr = univ_buf(0x44, 0x03, tio_slot);
    hdr[2] = n as u8;
    out.push(hdr);
    for (idx, c) in colors.iter().take(n).enumerate() {
        let mut p = univ_buf(0x44, 0x04, tio_slot);
        let id = (idx as u16).to_le_bytes();
        p[2] = id[0];
        p[3] = id[1];
        p[4] = c.r;
        p[5] = c.g;
        p[6] = c.b;
        out.push(p);
    }
    out
}

/// Universal `0x54` wireless auto-sleep timer (`minutes` converted to 24-bit LE seconds).
pub fn univ_set_auto_sleep_pkt(tio_slot: u8, profile_mask: u16, minutes: u16) -> [u8; 64] {
    let mut b = univ_buf(0x54, 0x01, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    let secs = (minutes.min(120) as u32) * 60;
    b[4] = (secs & 0xFF) as u8;
    b[5] = ((secs >> 8) & 0xFF) as u8;
    b[6] = ((secs >> 16) & 0xFF) as u8;
    b
}

/// Full Universal `0x60 0x01` audio configuration payload (`GenericAudioSettingConfigPayload`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnivAudioConfig {
    pub mic_muted: bool,
    pub mic_level: u8,
    pub polar_pattern: u8,
    pub global_filter: bool,
    pub sidetone_on: bool,
    pub sidetone_pct: u8,
    pub balance_on: bool,
    pub balance_pct: u8,
    pub chatmix_pct: u8,
    pub playback_on: bool,
    pub playback_pct: u8,
    pub playback_eq_idx: u8,
    pub mic_eq_idx: u8,
    pub reverse_light: bool,
}

impl Default for UnivAudioConfig {
    fn default() -> Self {
        Self {
            mic_muted: false,
            mic_level: 100,
            polar_pattern: 0,
            global_filter: false,
            sidetone_on: false,
            sidetone_pct: 50,
            balance_on: true,
            balance_pct: 50,
            chatmix_pct: 50,
            playback_on: true,
            playback_pct: 100,
            playback_eq_idx: 0,
            mic_eq_idx: 0,
            reverse_light: false,
        }
    }
}

/// Full Universal `0x60 0x01` generic audio settings packet.
pub fn univ_set_generic_audio_full_pkt(tio_slot: u8, cfg: &UnivAudioConfig) -> [u8; 64] {
    let mut b = univ_buf(0x60, 0x01, tio_slot);
    b[2] = u8::from(cfg.mic_muted);
    b[3] = cfg.mic_level.min(100);
    b[4] = cfg.polar_pattern;
    b[5] = u8::from(cfg.global_filter);
    b[6] = u8::from(cfg.sidetone_on);
    b[7] = cfg.sidetone_pct.min(100);
    b[8] = u8::from(cfg.balance_on);
    b[9] = cfg.balance_pct.min(100);
    b[10] = cfg.chatmix_pct.min(100);
    b[11] = u8::from(cfg.playback_on);
    b[12] = cfg.playback_pct.min(100);
    b[13] = cfg.playback_eq_idx;
    b[14] = cfg.mic_eq_idx;
    b[15] = u8::from(cfg.reverse_light);
    b
}

/// Universal `0x60` generic audio settings (`mic_mute`, `sidetone`, `eq_idx`) convenience helper.
pub fn univ_set_generic_audio_pkt(
    tio_slot: u8,
    mic_muted: bool,
    sidetone_on: bool,
    sidetone_pct: u8,
    playback_eq_idx: u8,
) -> [u8; 64] {
    univ_set_generic_audio_full_pkt(
        tio_slot,
        &UnivAudioConfig {
            mic_muted,
            sidetone_on,
            sidetone_pct,
            playback_eq_idx,
            ..UnivAudioConfig::default()
        },
    )
}

/// Standard 10-band EQ center frequencies used by NGENUITY headsets (Hz).
pub const EQ_10_FREQS: [u16; 10] = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// Universal `0x66 0x01` onboard 10-band equalizer packet (`OnboardEQConfigurationCMD`).
/// Each band gain in dB (`-12.0..=12.0`) is scaled by `EQGainMaxDecibelFactor = 8`.
pub fn univ_set_onboard_eq_pkt(
    tio_slot: u8,
    profile_mask: u16,
    playback: bool,
    preset_idx: u8,
    bands_db: &[f32; 10],
) -> [u8; 64] {
    let mut b = univ_buf(0x66, 0x01, tio_slot);
    let pm = profile_mask.to_le_bytes();
    b[2] = pm[0];
    b[3] = pm[1];
    b[4] = u8::from(playback);
    b[5] = preset_idx;
    for (k, &db) in bands_db.iter().enumerate() {
        let off = 5 * (k + 1) + 1;
        let f = EQ_10_FREQS[k].to_be_bytes();
        let scaled = db.clamp(-12.0, 12.0) * 8.0;
        let int_part = scaled.trunc() as i32;
        let frac = (((scaled - int_part as f32).abs() * 10.0) as u8).min(9);
        let sign_bit = if int_part < 0 || (int_part == 0 && scaled < -0.05) {
            0x80u8
        } else {
            0u8
        };
        let int_byte = (int_part.unsigned_abs().min(127) as u8) | sign_bit;
        b[off] = f[0];
        b[off + 1] = f[1];
        b[off + 2] = frac;
        b[off + 3] = int_byte;
        b[off + 4] = 0;
    }
    b
}

/// Universal `0x62` advanced audio prompt (`false` = beep, `true` = voice).
pub fn univ_set_voice_prompt_pkt(tio_slot: u8, voice: bool) -> [u8; 64] {
    let mut b = univ_buf(0x62, 0x01, tio_slot);
    b[2] = u8::from(voice);
    b
}

/// Universal `0x64` noise gate (`VoiceMeter`) toggle.
pub fn univ_set_noise_gate_pkt(tio_slot: u8, enabled: bool) -> [u8; 64] {
    let mut b = univ_buf(0x64, 0x01, tio_slot);
    b[2] = u8::from(enabled);
    b
}

/// Legacy Pulsefire Dart / Haste Wireless battery query (`0x51`, 65 bytes).
pub fn legacy_mouse_battery_req() -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0x51;
    b
}

/// Parse legacy Pulsefire Dart / Haste Wireless `0x51` response -> `(pct, charging)`.
pub fn parse_legacy_mouse_battery(resp: &[u8]) -> Option<(u8, Option<bool>)> {
    if resp.len() < 7 {
        return None;
    }
    let pct = resp[5].min(100);
    let charging = match resp[6] {
        0 => Some(false),
        1 | 2 => Some(true),
        _ => None,
    };
    Some((pct, charging))
}

/// Legacy Pulsefire Dart / FPS Pro 2 / Haste Wireless polling rate (`0xD0`).
pub fn legacy_mouse_polling_pkt(hz: u16) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0xD0;
    b[4] = 0x01;
    b[5] = match hz {
        1000 => 3,
        500 => 2,
        250 => 1,
        125 => 0,
        _ => 3,
    };
    b
}

/// Legacy Pulsefire Dart / FPS Pro 2 DPI stage + active index command sequence (`0xD3`).
/// Never appends the flash commit `0xDE 0xFF`.
pub fn legacy_mouse_dpi_pkts(stages: &[(u16, Rgb)], active_idx: u8) -> Vec<[u8; 65]> {
    let count = stages.len().min(5);
    let mut pkts = Vec::with_capacity(count * 2 + 2);
    for (i, &(dpi, col)) in stages.iter().take(count).enumerate() {
        let mut p_dpi = [0u8; 65];
        p_dpi[1] = 0xD3;
        p_dpi[2] = 0x02;
        p_dpi[3] = i as u8;
        p_dpi[4] = 0x02;
        let reg = (dpi.clamp(50, 16000) / 50).to_le_bytes();
        p_dpi[5] = reg[0];
        p_dpi[6] = reg[1];
        pkts.push(p_dpi);

        let mut p_col = [0u8; 65];
        p_col[1] = 0xD3;
        p_col[2] = 0x03;
        p_col[3] = i as u8;
        p_col[4] = 0x03;
        p_col[5] = col.r;
        p_col[6] = col.g;
        p_col[7] = col.b;
        pkts.push(p_col);
    }
    let mut p_en = [0u8; 65];
    p_en[1] = 0xD3;
    p_en[2] = 0x01;
    p_en[4] = 0x01;
    p_en[5] = if count == 0 { 0 } else { (1u8 << count) - 1 };
    pkts.push(p_en);

    let mut p_act = [0u8; 65];
    p_act[1] = 0xD3;
    p_act[4] = 0x01;
    p_act[5] = active_idx.min(count.saturating_sub(1) as u8);
    pkts.push(p_act);
    pkts
}

/// Legacy Pulsefire Dart / Haste Wireless low-battery warning threshold (`0xD1`, `5..=25%`).
pub fn legacy_mouse_low_battery_pkt(threshold_pct: u8) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0xD1;
    b[4] = 0x01;
    b[5] = threshold_pct.clamp(5, 25);
    b
}

/// Legacy Pulsefire Dart / Haste Wireless built-in hardware effect (`0xD2 0x20`).
pub fn legacy_mouse_builtin_effect_pkt(
    effect_type: u8,
    c1: Rgb,
    c2: Rgb,
    brightness_pct: u8,
    speed: u8,
) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0xD2;
    b[2] = 0x20;
    b[3] = effect_type;
    b[4] = 0x08;
    b[5..8].copy_from_slice(&[c1.r, c1.g, c1.b]);
    b[8..11].copy_from_slice(&[c2.r, c2.g, c2.b]);
    b[11] = brightness_pct.min(100);
    b[12] = speed.min(100);
    b
}

/// Legacy Pulsefire wired Haste (`0x1727` / `0x0F8F`) 65-byte profile packet sequence.
pub fn legacy_haste_wired_profile_pkts(
    polling_hz: u16,
    brightness_pct: u8,
    stages: &[(u16, Rgb)],
) -> [[u8; 65]; 3] {
    let mut init = [0u8; 65];
    init[0] = 0x07;
    init[1] = 0x04;
    init[2] = 0x67;
    init[9] = 0x01;

    let mut cfg = [0u8; 65];
    cfg[0] = 0x07;
    cfg[1] = 0x03;
    cfg[13] = ((brightness_pct.min(100) as u16 * 255) / 100) as u8;
    cfg[17] = match polling_hz {
        125 => 3,
        250 => 2,
        500 => 1,
        _ => 0, // 1000 Hz
    };
    let count = stages.len().min(5);
    cfg[18] = if count == 0 { 0 } else { (1u8 << count) - 1 };
    for (i, &(dpi, col)) in stages.iter().take(5).enumerate() {
        let le = dpi.clamp(100, 16000).to_le_bytes();
        cfg[21 + i * 2] = le[0];
        cfg[21 + i * 2 + 1] = le[1];
        let c_off = 33 + i * 4;
        cfg[c_off] = col.r;
        cfg[c_off + 1] = col.g;
        cfg[c_off + 2] = col.b;
    }
    cfg[63] = 0xAA;
    cfg[64] = 0x55;

    let mut commit = [0u8; 65];
    commit[0] = 0x07;
    commit[1] = 0x04;
    commit[2] = 0x02;
    commit[63] = 0xAA;
    commit[64] = 0x55;

    [init, cfg, commit]
}

/// Legacy Cloud Alpha Wireless `0x21 0xBB` command packet (65 bytes).
pub fn cloud_alpha_wireless_pkt(cmd: u8, arg: u8) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[0] = 0x21;
    b[1] = 0xBB;
    b[2] = cmd;
    b[3] = arg;
    b
}

/// Parse Cloud Alpha Wireless `0x21 0xBB` battery (`cmd = 0x0B|0x25`) or charge (`0x0C|0x26`).
pub fn parse_cloud_alpha_wireless_resp(resp: &[u8]) -> (Option<u8>, Option<bool>) {
    if resp.len() < 4 || resp[0] != 0x21 || resp[1] != 0xBB {
        return (None, None);
    }
    match resp[2] {
        0x0B | 0x25 => (Some(resp[3].min(100)), None),
        0x0C | 0x26 => (None, Some(resp[3] == 1 || resp[3] == 2)),
        _ => (None, None),
    }
}

/// Legacy Cloud II Wireless DTS / Cloud Stinger 2 Wireless `0x06 0xFF 0xBB` packet (65 bytes).
pub fn cloud2_dts_pkt(cmd: u8, arg: u8) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[0] = 0x06;
    b[1] = 0xFF;
    b[2] = 0xBB;
    b[3] = cmd;
    b[4] = arg;
    b
}

/// Parse Cloud II Wireless DTS / Cloud Stinger 2 Wireless input report.
pub fn parse_cloud2_dts_resp(resp: &[u8]) -> (Option<u8>, Option<bool>) {
    if resp.len() < 8 || resp[0] != 0x06 || resp[1] != 0xFF || resp[2] != 0xBB {
        return (None, None);
    }
    match resp[3] {
        0x02 | 0x25 => (Some(resp[7].min(100)), None),
        0x03 => (None, Some(resp[4] == 1 || resp[4] == 2)),
        _ => (None, None),
    }
}

/// Legacy Cloud II Wireless (non-DTS) / Cloud Flight S `0x06 ... 0xBB` packet (65 bytes).
pub fn cloud2_wireless_pkt(cmd: u8, arg: u8) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[0] = 0x06;
    b[2] = 0x02;
    b[4] = 0x9A;
    b[7] = 0x68;
    b[8] = 0x4A;
    b[9] = 0x8E;
    b[10] = 0x0A;
    b[14] = 0xBB;
    b[15] = cmd;
    b[16] = arg;
    b
}

/// Parse Cloud II Wireless (non-DTS) / Cloud Flight S input report (`resp[2]==0xBB`).
pub fn parse_cloud2_wireless_resp(resp: &[u8]) -> (Option<u8>, Option<bool>) {
    if resp.len() < 8 || resp[2] != 0xBB {
        return (None, None);
    }
    match resp[3] {
        0x02 => (Some(resp[7].min(100)), None),
        0x03 => (None, Some(resp[4] == 1 || resp[4] == 2)),
        _ => (None, None),
    }
}

/// Cloud Flight Wireless (`0x21 0xFF 0x05`) battery query packet (65 bytes).
pub fn cloud_flight_wireless_battery_req() -> [u8; 65] {
    let mut b = [0u8; 65];
    b[0] = 0x21;
    b[1] = 0xFF;
    b[2] = 0x05;
    b
}

/// Map `(high_byte, low_byte)` from Cloud Flight Wireless `0x21 0xFF 0x05` to `(pct, charging)`.
pub fn cloud_flight_battery_pct(hi: u8, lo: u8) -> (u8, bool) {
    if hi == 20 {
        return (if lo == 1 { 100 } else { 50 }, true);
    }
    let pct = match (hi, lo) {
        (0..=13, _) => 0,
        (14, 0..=89) => 10,
        (14, 90..=119) => 15,
        (14, 120..=148) => 20,
        (14, 149..=159) => 25,
        (14, 160..=169) => 30,
        (14, 170..=179) => 35,
        (14, 180..=189) => 40,
        (14, 190..=199) => 45,
        (14, 200..=209) => 50,
        (14, 210..=219) => 55,
        (14, 220..=239) => 60,
        (14, 240..=255) => 65,
        (15, 0..=19) => 70,
        (15, 20..=49) => 75,
        (15, 50..=69) => 80,
        (15, 70..=99) => 85,
        (15, 100..=119) => 90,
        (15, 120..=129) => 95,
        _ => 100,
    };
    (pct, false)
}

/// Cloud III Wireless & Cloud II Core Wireless `0x07` command packet (264 bytes).
pub fn cloud3_wireless_pkt(cmd: u8, arg: u8) -> [u8; 264] {
    let mut b = [0u8; 264];
    b[0] = 0x07;
    b[1] = cmd;
    b[2] = arg;
    b
}

/// Parse Cloud III Wireless / Cloud II Core Wireless `0x66 0x07` input report.
pub fn parse_cloud3_wireless_resp(resp: &[u8]) -> (Option<u8>, Option<bool>) {
    if resp.len() < 5 || resp[0] != 0x66 || resp[1] != 0x07 {
        return (None, None);
    }
    if resp[2] == 1 && resp[3] == 0 {
        return (None, Some(true));
    }
    if resp[2] != 0 || resp[3] != 0 {
        return (Some(resp[4].min(100)), None);
    }
    (None, None)
}

/// Cloud III S Wireless `0x07` command packet with `02 03 00 00 sub` prefix (264 bytes).
pub fn cloud3s_wireless_pkt(sub: u8, arg0: u8, arg1: u8) -> [u8; 264] {
    let mut b = [0u8; 264];
    b[0] = 0x07;
    b[1] = 0x02;
    b[2] = 0x03;
    b[5] = sub;
    b[6] = arg0;
    b[7] = arg1;
    b
}

/// Build Cloud III S Wireless auto-sleep packet (`minutes` -> big-endian seconds).
pub fn cloud3s_auto_sleep_pkt(minutes: u16) -> [u8; 264] {
    let secs = minutes.min(120) * 60;
    let be = secs.to_be_bytes();
    cloud3s_wireless_pkt(0x4A, be[0], be[1])
}

/// Build Cloud III S Wireless active EQ preset index packet (`0..=5`).
pub fn cloud3s_eq_index_pkt(idx: u8) -> [u8; 264] {
    cloud3s_wireless_pkt(0x0F, idx.min(5), 0)
}

/// Build Cloud III S Wireless single-band EQ gain packet (`band_idx 0..=9`, `gain_db -12..=+12`).
pub fn cloud3s_eq_band_pkt(band_idx: u8, gain_db: f32) -> [u8; 264] {
    let raw = (gain_db.clamp(-12.0, 12.0) * 8.0 * 100.0).round() as i16;
    let be = raw.to_be_bytes();
    let mut p = cloud3s_wireless_pkt(0x5F, band_idx.min(9), be[0]);
    p[8] = be[1];
    p
}

/// Build 20-packet 10-band parametric EQ sequence for Cloud Alpha S / Flight S / Cloud II Wireless.
pub fn cloud_tune_eq_pkts(is_alpha_s: bool, bands_db: &[f32; 10]) -> Vec<[u8; 65]> {
    let base: u8 = if is_alpha_s { 4 } else { 10 };
    let mut pkts = Vec::with_capacity(20);
    for (k, &gain) in bands_db.iter().enumerate() {
        for pass in 0..2u8 {
            let mut b = [0u8; 65];
            b[0] = 0x06;
            b[2] = 0x0D;
            b[4] = 0x41;
            b[6] = base + pass;
            b[7..=9].copy_from_slice(&[0x30, 0x2B, 0xAE]);
            b[10..14].copy_from_slice(&(k as i32).to_le_bytes());
            b[14..18].copy_from_slice(&3i32.to_le_bytes());
            b[18..22].copy_from_slice(&(EQ_10_FREQS[k] as i32).to_le_bytes());
            b[22..26].copy_from_slice(&1.0f32.to_le_bytes());
            b[26..30].copy_from_slice(&gain.clamp(-12.0, 12.0).to_le_bytes());
            pkts.push(b);
        }
    }
    pkts
}

/// Cloud Stinger Core Wireless + 7.1 (`0x170B`) Feature battery request (65 bytes).
pub fn cloud_stinger_core_wireless_battery_req() -> [u8; 65] {
    let mut b = [0u8; 65];
    b[..12].copy_from_slice(&[
        0xFF, 0x07, 0x00, 0xFD, 0x04, 0x0C, 0xF1, 0x02, 0x01, 0x04, 0xF0, 0x0C,
    ]);
    b
}

/// Parse Cloud Stinger Core Wireless + 7.1 battery Feature response (`(pct, charging)`).
pub fn parse_cloud_stinger_core_wireless_battery(resp: &[u8]) -> Option<(u8, Option<bool>)> {
    const CHARGING_LUT: [f32; 11] = [
        3.4000, 3.8309, 3.8707, 3.9162, 3.9299, 3.9456, 3.9674, 4.0000, 4.0497, 4.0900, 4.1300,
    ];
    const DISCHARGING_LUT: [f32; 11] = [
        3.5174, 3.6508, 3.6998, 3.7381, 3.7633, 3.7847, 3.8381, 3.9052, 3.9542, 4.0423, 4.1205,
    ];

    if resp.len() < 13 {
        return None;
    }
    let charging = match resp[9] {
        3 => Some(false),
        5 | 6 => Some(true),
        _ => None,
    };
    let mv = u16::from_le_bytes([resp[11], resp[12]]);
    let v = f32::from(mv) / 1000.0;
    if !(2.5..=4.6).contains(&v) {
        return None;
    }
    if resp[9] == 6 {
        return Some((100, Some(true)));
    }
    let lut = if charging == Some(true) {
        &CHARGING_LUT
    } else {
        &DISCHARGING_LUT
    };
    let pct = if v <= lut[0] {
        0.0
    } else if v >= lut[10] {
        100.0
    } else {
        let mut out = 100.0;
        for i in 0..10 {
            if v <= lut[i + 1] {
                let span = (lut[i + 1] - lut[i]).max(0.0001);
                out = (i as f32 * 10.0) + ((v - lut[i]) / span) * 10.0;
                break;
            }
        }
        out
    };
    Some((pct.round().clamp(0.0, 100.0) as u8, charging))
}

/// Build QuadCast 2 / 2 S polar pattern packet (`0=Stereo, 1=Omni, 2=Cardioid, 3=Bidirectional`).
pub fn quadcast2_set_polar_pkt(pat: u8) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0] = 0x77;
    b[1] = 0x05;
    b[2] = pat.min(3);
    b
}

/// Build QuadCast 2 / 2 S mic mute packet (`0x77 0x06`).
pub fn quadcast2_set_mute_pkt(muted: bool) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0] = 0x77;
    b[1] = 0x06;
    b[2] = u8::from(muted);
    b
}

/// Build QuadCast 2 / 2 S high-pass filter packet (`0x77 0x07`).
pub fn quadcast2_set_highpass_pkt(on: bool) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0] = 0x77;
    b[1] = 0x07;
    b[2] = u8::from(on);
    b
}

/// Build QuadCast 2 S mic gain / mix packet (`0x77 0x08`, `0..=100`).
pub fn quadcast2s_set_mix_pkt(pct: u8) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0] = 0x77;
    b[1] = 0x08;
    b[2] = pct.min(100);
    b
}

/// Build QuadCast 2 S monitor balance packet (`0x77 0x0A`, `0..=100`).
pub fn quadcast2s_set_balance_pkt(pct: u8) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[0] = 0x77;
    b[1] = 0x0A;
    b[2] = pct.min(100);
    b
}

/// Build DuoCast polar pattern Feature packet (`cardioid -> 1`, `omnidirectional -> 2`).
pub fn duocast_set_polar_pkt(cardioid: bool) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0x04;
    b[2] = 0x05;
    b[3] = if cardioid { 1 } else { 2 };
    b
}

/// Build DuoCast high-pass filter Feature packet (`0x00 0x04 0x07`).
pub fn duocast_set_highpass_pkt(on: bool) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0x04;
    b[2] = 0x07;
    b[3] = u8::from(on);
    b
}

/// Build DuoCast mic gain Feature packet (`0x00 0x04 0x08`, `0..=100`).
pub fn duocast_set_gain_pkt(pct: u8) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0x04;
    b[2] = 0x08;
    b[3] = pct.min(100);
    b
}

/// Build DuoCast mic monitoring / sidetone Feature packet (`0x00 0x04 0x09`).
pub fn duocast_set_sidetone_pkt(on: bool) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0x04;
    b[2] = 0x09;
    b[3] = u8::from(on);
    b
}

/// Build DuoCast LED brightness & ring direction info Feature packet (`0x00 0x04 0x57`).
pub fn duocast_set_lights_info_pkt(br_pct: u8, reverse: bool) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[1] = 0x04;
    b[2] = 0x57;
    b[13] = ((u16::from(br_pct.min(100)) * 255) / 100) as u8;
    b[17] = u8::from(reverse);
    b
}

/// Build SoloCast mute state Feature packet (`0x66 0x01`).
pub fn solocast_set_mute_pkt(muted: bool) -> [u8; 65] {
    let mut b = [0u8; 65];
    b[0] = 0x66;
    b[1] = 0x01;
    b[2] = u8::from(muted);
    b
}

/// Build FURY Ultra 2-packet Feature sequence for pad brightness (`0..=100%`).
pub fn fury_ultra_brightness_pkts(br_pct: u8) -> [[u8; 65]; 2] {
    let mut info = [0u8; 65];
    info[1] = 0x04;
    info[2] = 0x57;
    info[13] = ((u16::from(br_pct.min(100)) * 255) / 100) as u8;
    info[63] = 0xAA;
    info[64] = 0x55;
    let mut commit = [0u8; 65];
    commit[1] = 0x04;
    commit[2] = 0x02;
    [info, commit]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universal_tio_sub_byte_roundtrips_slot_and_cmd() {
        let sub = univ_sub_byte(0x02, 2);
        assert_eq!(sub, 0x82);
        assert_eq!(split_univ_sub_byte(sub), (0x02, 2));
    }

    #[test]
    fn universal_mouse_dpi_encodes_polling_and_stages() {
        let pkt = univ_set_mouse_dpi_pkt(
            0,
            1,
            8000,
            &[(800, Rgb::new(1, 2, 3)), (1600, Rgb::new(4, 5, 6))],
            1,
        );
        assert_eq!(pkt[0], 0x32);
        assert_eq!(pkt[1], 0x01);
        assert_eq!(pkt[2..=3], [0x01, 0x00]);
        assert_eq!(pkt[4], 1); // 8000 Hz
        assert_eq!(pkt[5], 0b11);
        assert_eq!(pkt[6], 1);
        // 800 / 50 - 1 = 15
        assert_eq!(pkt[7..12], [15, 0, 1, 2, 3]);
        // 1600 / 50 - 1 = 31
        assert_eq!(pkt[12..17], [31, 0, 4, 5, 6]);
    }

    #[test]
    fn universal_sensor_and_sleep_match_protocol() {
        let s = univ_set_mouse_sensor_pkt(1, 1, true, true);
        assert_eq!(s[0], 0x36);
        assert_eq!(s[1], 0x41);
        assert_eq!(s[4], 1);
        assert_eq!(s[7], 2);

        let full = univ_set_mouse_sensor_full_pkt(0, 1, false, angle_tune_wire_byte(-30), true);
        assert_eq!(full[4], 0);
        assert_eq!(full[5], 226); // -30° = 0xE2
        assert_eq!(full[6], 1);
        assert_eq!(full[7], 2);

        let sl = univ_set_auto_sleep_pkt(0, 1, 20);
        assert_eq!(sl[0], 0x54);
        assert_eq!(sl[4..7], [0xB0, 0x04, 0x00]); // 1200 s LE
    }

    #[test]
    fn universal_extended_cmds_encode_expected_fields() {
        let gm = univ_set_game_mode_bits_pkt(0, 1, true, 0x1F);
        assert_eq!(gm[0], 0x30);
        assert_eq!(gm[4..=5], [1, 0x1F]);

        let fade = univ_set_backlight_fade_pkt(0, 1, 15);
        assert_eq!(fade[0], 0x46);
        assert_eq!(fade[1], 0x01);
        assert_eq!(fade[4..=6], [1, 15, 0]);

        let amb = univ_set_ambient_brightness_pkt(0, 1, true);
        assert_eq!(amb[0], 0x46);
        assert_eq!(amb[1], 0x03);
        assert_eq!(amb[4..=8], [1, 25, 50, 75, 100]);

        let low = univ_set_low_battery_pkt(0, 20, true);
        assert_eq!(low[0], 0x50);
        assert_eq!(low[1], 0x01);
        assert_eq!(low[2..=3], [20, 1]);

        let light = univ_preset_light_pkt(0, 1, 1, 80, 0, 1, &[Rgb::new(10, 20, 30)]);
        assert_eq!(light[0], 0x42);
        assert_eq!(light[1], 0x02);
        assert_eq!(light[4], 1); // static
        assert_eq!(light[5], 80);
        assert_eq!(light[8..=11], [1, 10, 20, 30]);

        let eq = univ_set_onboard_eq_pkt(
            0,
            1,
            true,
            2,
            &[6.0, -3.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        assert_eq!(eq[0], 0x66);
        assert_eq!(eq[1], 0x01);
        assert_eq!(eq[2], 1);
        assert_eq!(eq[4], 1);
        assert_eq!(eq[5], 2);
        assert_eq!(eq[6..=10], [0, 32, 0, 48, 0]); // 32 Hz, +6.0 * 8 = 48.0
        assert_eq!(eq[11..=15], [0, 64, 0, 0x9C, 0]); // 64 Hz, -3.5 * 8 = -28.0 -> int 28 | 0x80 = 0x9C

        let audio = univ_set_generic_audio_full_pkt(
            0,
            &UnivAudioConfig {
                mic_muted: true,
                mic_level: 85,
                sidetone_on: true,
                sidetone_pct: 70,
                balance_pct: 60,
                playback_pct: 90,
                chatmix_pct: 40,
                playback_eq_idx: 3,
                reverse_light: true,
                ..Default::default()
            },
        );
        assert_eq!(audio[0], 0x60);
        assert_eq!(audio[2], 1);
        assert_eq!(audio[3], 85);
        assert_eq!(audio[6], 1);
        assert_eq!(audio[7], 70);
        assert_eq!(audio[9], 60);
        assert_eq!(audio[10], 40);
        assert_eq!(audio[12], 90);
        assert_eq!(audio[13], 3);
        assert_eq!(audio[15], 1);
    }

    #[test]
    fn headset_and_mic_packets_match_rea_captures() {
        let eq_idx = cloud3s_eq_index_pkt(3);
        assert_eq!(eq_idx[0..=6], [0x07, 0x02, 0x03, 0x00, 0x00, 0x0F, 3]);
        let eq_band = cloud3s_eq_band_pkt(2, -6.0);
        // -6.0 * 800 = -4800 -> 0xED40 BE
        assert_eq!(eq_band[5..=8], [0x5F, 2, 0xED, 0x40]);

        let tune = cloud_tune_eq_pkts(true, &[0.0; 10]);
        assert_eq!(tune.len(), 20);
        assert_eq!(tune[0][6..=9], [4, 0x30, 0x2B, 0xAE]);
        assert_eq!(tune[1][6..=9], [5, 0x30, 0x2B, 0xAE]);

        let mut stinger_resp = [0u8; 65];
        stinger_resp[9] = 3; // not charging
        stinger_resp[11..13].copy_from_slice(&3838u16.to_le_bytes()); // 3.838 V = index 6 (60%)
        let parsed = parse_cloud_stinger_core_wireless_battery(&stinger_resp).unwrap();
        assert_eq!(parsed.1, Some(false));
        assert_eq!(parsed.0, 60);

        assert_eq!(&quadcast2_set_polar_pkt(2)[..3], &[0x77, 0x05, 2]);
        assert_eq!(&duocast_set_polar_pkt(false)[..4], &[0x00, 0x04, 0x05, 2]);
        assert_eq!(&solocast_set_mute_pkt(true)[..3], &[0x66, 0x01, 1]);
        let fury = fury_ultra_brightness_pkts(100);
        assert_eq!(fury[0][13], 255);
        assert_eq!(fury[0][63..=64], [0xAA, 0x55]);
        assert_eq!(&fury[1][..3], &[0x00, 0x04, 0x02]);
    }

    #[test]
    fn battery_parsers_decode_known_frames() {
        assert_eq!(
            parse_univ_battery(&[0x51, 0x02, 87, 1]),
            Some((87, Some(true)))
        );
        assert_eq!(
            parse_legacy_mouse_battery(&[0, 0x51, 0, 0, 0, 64, 0]),
            Some((64, Some(false)))
        );
        assert_eq!(
            parse_cloud_alpha_wireless_resp(&[0x21, 0xBB, 0x0B, 92]),
            (Some(92), None)
        );
        assert_eq!(cloud_flight_battery_pct(15, 55), (80, false));
        assert_eq!(cloud_flight_battery_pct(20, 1), (100, true));
        assert_eq!(
            parse_cloud3_wireless_resp(&[0x66, 0x07, 1, 1, 75]),
            (Some(75), None)
        );
    }

    #[test]
    fn legacy_mouse_dpi_never_emits_flash_save() {
        let pkts = legacy_mouse_dpi_pkts(&[(800, Rgb::new(10, 20, 30))], 0);
        assert!(pkts.iter().all(|p| p[1] != 0xDE));
        let eff = legacy_mouse_builtin_effect_pkt(2, Rgb::new(1, 2, 3), Rgb::BLACK, 80, 50);
        assert_eq!(eff[1..=3], [0xD2, 0x20, 2]);
    }
}
