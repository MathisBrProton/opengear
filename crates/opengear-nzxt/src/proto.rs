//! Clean-room NZXT HID protocol packets & parsers.

pub const NZXT_VID: u16 = 0x1E71;

pub const NZXT_SMART2_PIDS: &[u16] = &[
    0x2006, 0x2009, 0x200D, 0x200E, 0x200F, 0x2010, 0x2011, 0x2019, 0x2020,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KrakenSeries {
    /// Kraken X53/X63/X73 (coolant temp + pump PWM only).
    X53,
    /// Kraken Z53/Z63/Z73 (coolant temp + pump + fan).
    Z53,
    /// Kraken 2023 / 2023 Elite / 2024 Elite (coolant temp + pump + fan, extended `0x72` header).
    Kraken2023,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NzxtModel {
    SmartDeviceV2,
    Kraken(KrakenSeries),
}

pub fn identify_nzxt_pid(pid: u16) -> Option<(&'static str, NzxtModel)> {
    match pid {
        0x2006 | 0x200D | 0x200F => Some(("NZXT Smart Device V2", NzxtModel::SmartDeviceV2)),
        0x2009 | 0x200E | 0x2010 | 0x2011 | 0x2019 | 0x2020 => {
            Some(("NZXT RGB & Fan Controller", NzxtModel::SmartDeviceV2))
        }
        0x2007 | 0x2014 => Some((
            "NZXT Kraken X53/X63/X73",
            NzxtModel::Kraken(KrakenSeries::X53),
        )),
        0x3008 => Some((
            "NZXT Kraken Z53/Z63/Z73",
            NzxtModel::Kraken(KrakenSeries::Z53),
        )),
        0x300E => Some((
            "NZXT Kraken 2023",
            NzxtModel::Kraken(KrakenSeries::Kraken2023),
        )),
        0x300C => Some((
            "NZXT Kraken 2023 Elite",
            NzxtModel::Kraken(KrakenSeries::Kraken2023),
        )),
        0x3012 => Some((
            "NZXT Kraken 2024 Elite",
            NzxtModel::Kraken(KrakenSeries::Kraken2023),
        )),
        _ => None,
    }
}

pub const NZXT_REPORT_LEN: usize = 64;
pub const NZXT_PUMP_MIN_PCT: u8 = 45;
pub const NZXT_FAN_MIN_PCT: u8 = 30;
pub const KRAKEN_CURVE_POINTS: usize = 40;

pub const SMART2_CMD_SET_INTERVAL: [u8; 5] = [0x60, 0x02, 0x01, 0xE8, 0x03];
pub const SMART2_CMD_DETECT_FANS: [u8; 2] = [0x60, 0x03];
pub const SMART2_REPORT_FAN_CONFIG: u8 = 0x61;
pub const SMART2_REPORT_STATUS: u8 = 0x67;
pub const SMART2_STATUS_SPEED: u8 = 0x02;

pub const KRAKEN_CMD_GET_FW: [u8; 2] = [0x10, 0x01];
pub const KRAKEN_CMD_SET_INTERVAL: [u8; 5] = [0x70, 0x02, 0x01, 0xB8, 0x01];
pub const KRAKEN_CMD_FINISH_INIT: [u8; 2] = [0x70, 0x01];
pub const KRAKEN_CMD_GET_STATUS: [u8; 2] = [0x74, 0x01];
pub const KRAKEN_REPORT_FW: u8 = 0x11;
pub const KRAKEN_REPORT_STATUS: u8 = 0x75;

pub fn pad_nzxt_report(bytes: &[u8]) -> [u8; NZXT_REPORT_LEN] {
    let mut out = [0u8; NZXT_REPORT_LEN];
    let n = bytes.len().min(NZXT_REPORT_LEN);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

/// Encode Smart Device V2 set-fan output report (`0x62`).
pub fn encode_smart2_set_fans(mask: u8, duties: [u8; 3]) -> [u8; NZXT_REPORT_LEN] {
    let mut pkt = [0u8; NZXT_REPORT_LEN];
    pkt[0] = 0x62;
    pkt[1] = 0x01;
    pkt[2] = mask;
    pkt[3] = duties[0].clamp(NZXT_FAN_MIN_PCT, 100);
    pkt[4] = duties[1].clamp(NZXT_FAN_MIN_PCT, 100);
    pkt[5] = duties[2].clamp(NZXT_FAN_MIN_PCT, 100);
    pkt
}

/// Encode Kraken3 40-point thermal curve (`0x72`) over 20..=59 C; index 39 is always 100 % for safety.
pub fn encode_kraken_curve(
    series: KrakenSeries,
    channel: u8,
    curve: &[u8; KRAKEN_CURVE_POINTS],
) -> [u8; NZXT_REPORT_LEN] {
    let mut pkt = [0u8; NZXT_REPORT_LEN];
    pkt[0] = 0x72;
    pkt[1] = channel + 1; // 1 = pump, 2 = fan
    if series == KrakenSeries::Kraken2023 {
        pkt[2] = 1;
        if channel == 1 {
            pkt[3] = 1;
        }
    }
    let min_pct = if channel == 0 {
        NZXT_PUMP_MIN_PCT
    } else {
        NZXT_FAN_MIN_PCT
    };
    for (i, dst) in pkt[4..4 + KRAKEN_CURVE_POINTS].iter_mut().enumerate() {
        *dst = if i == KRAKEN_CURVE_POINTS - 1 {
            100
        } else {
            curve[i].clamp(min_pct, 100)
        };
    }
    pkt
}

/// Build a fixed-duty Kraken3 curve (`duty` across 20..=58 C, `100`% critical failsafe at 59 C).
pub fn fixed_kraken_curve(channel: u8, duty_pct: u8) -> [u8; KRAKEN_CURVE_POINTS] {
    let min_pct = if channel == 0 {
        NZXT_PUMP_MIN_PCT
    } else {
        NZXT_FAN_MIN_PCT
    };
    let clamped = duty_pct.clamp(min_pct, 100);
    let mut c = [clamped; KRAKEN_CURVE_POINTS];
    c[KRAKEN_CURVE_POINTS - 1] = 100;
    c
}

/// Balanced autonomous Kraken3 coolant curve (`20..=59 C`) used when returning control to firmware.
pub fn default_kraken_curve(channel: u8) -> [u8; KRAKEN_CURVE_POINTS] {
    let min_pct = if channel == 0 {
        NZXT_PUMP_MIN_PCT
    } else {
        NZXT_FAN_MIN_PCT
    };
    let mut curve = [min_pct; KRAKEN_CURVE_POINTS];
    for (i, slot) in curve.iter_mut().enumerate() {
        let temp_c = 20 + i;
        let pct = if temp_c <= 28 {
            min_pct
        } else if temp_c >= 48 {
            100
        } else {
            let span = (100 - min_pct) as usize;
            min_pct + ((temp_c - 28) * span / 20) as u8
        };
        *slot = pct.clamp(min_pct, 100);
    }
    curve[KRAKEN_CURVE_POINTS - 1] = 100;
    curve
}
