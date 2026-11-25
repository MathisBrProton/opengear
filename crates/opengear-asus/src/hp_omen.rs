//! Native HP OMEN & Victus BIOS WMI driver (`root\wmi\hpqBIntM`, zero bloatware).
//!
//! Communicates directly with the system BIOS / Embedded Controller via COM WMI
//! using `Sign = "SECU"`, supporting:
//! - Command `0x20008` (`HPWMI_GM`): system design data, thermal policies (V0/V1),
//!   live CPU/GPU fan RPMs, Max Fan boost, manual fan targets + EC keep-alive, GPU Dynamic Boost.
//! - Command `0x20009` (`HPWMI_KB`): keyboard type detection, backlight toggle, and 4-zone RGB table.

use anyhow::{Context, Result};
use opengear_core::Rgb;
use serde::{Deserialize, Serialize};

pub const HPWMI_GM: u32 = 0x20008;
pub const HPWMI_KB: u32 = 0x20009;

// 0x20008 (GM) command types (verified against Linux hp-wmi.c & 88D2 BIOS)
const GM_GET_FAN_COUNT: u32 = 0x10;
const GM_GET_FAN_SPEED: u32 = 0x11;
const GM_SET_PERFORMANCE_MODE: u32 = 0x1A;
const GM_GET_GPU_POWER: u32 = 0x21;
const GM_SET_GPU_POWER: u32 = 0x22;
const GM_GET_MAX_FAN: u32 = 0x26;
const GM_SET_MAX_FAN: u32 = 0x27;
const GM_GET_SYSTEM_DESIGN: u32 = 0x28;
const GM_GET_FAN_LEVEL: u32 = 0x2D;
const GM_SET_FAN_LEVEL: u32 = 0x2E; // write-only: never invoke during probes
const GM_GET_FAN_TABLE: u32 = 0x2F;

// 0x20009 (KB) command types
const KB_GET_TYPE: u32 = 0x01;
const KB_GET_COLOR_TABLE: u32 = 0x02;
const KB_SET_COLOR_TABLE: u32 = 0x03;
const KB_GET_BACKLIGHT: u32 = 0x04;
const KB_SET_BACKLIGHT: u32 = 0x05;

const COLOR_TABLE_LEN: usize = 128;
const COLOR_TABLE_ZONE_OFFSET: usize = 25;
const BACKLIGHT_ON: u8 = 0xE4;
const BACKLIGHT_OFF: u8 = 0x64;
const MANUAL_FAN_MIN_PCT: u8 = 30;
const DEFAULT_MAX_FAN_UNITS: u8 = 55; // 5500 RPM fallback cap

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OmenThermalMode {
    #[default]
    Default,
    Performance,
    Cool,
}

impl OmenThermalMode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Performance => "performance",
            Self::Cool => "cool",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "performance" => Self::Performance,
            "cool" => Self::Cool,
            _ => Self::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Équilibré",
            Self::Performance => "Performances",
            Self::Cool => "Silencieux / Éco",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OmenGpuPowerLevel {
    #[default]
    Minimum,
    Medium,
    Maximum,
}

impl OmenGpuPowerLevel {
    pub fn id(self) -> &'static str {
        match self {
            Self::Minimum => "min",
            Self::Medium => "med",
            Self::Maximum => "max",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s {
            "max" => Self::Maximum,
            "med" | "medium" => Self::Medium,
            _ => Self::Minimum,
        }
    }

    /// Build the 4-byte `[cTGP, PPAB, dState, peakTemp]` payload for `0x20008 / 0x22`.
    pub fn payload(self) -> [u8; 4] {
        match self {
            Self::Minimum => [0x00, 0x00, 0x01, 0x00],
            Self::Medium => [0x00, 0x01, 0x01, 0x00],
            Self::Maximum => [0x01, 0x01, 0x01, 0x00],
        }
    }

    /// Decode `[cTGP, PPAB, dState, peakTemp]` returned by `0x20008 / 0x21`.
    pub fn from_gpu_power_bytes(bytes: &[u8]) -> Self {
        let ctgp = bytes.first().copied().unwrap_or(0) != 0;
        let ppab = bytes.get(1).copied().unwrap_or(0) != 0;
        match (ctgp, ppab) {
            (true, true) => Self::Maximum,
            (true, false) | (false, true) => Self::Medium,
            (false, false) => Self::Minimum,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OmenKeyboardType {
    #[default]
    None,
    White,
    FourZoneRgb,
    PerKeyRgb,
    Unknown(u8),
}

impl OmenKeyboardType {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::None,
            1 => Self::FourZoneRgb,
            // 0x06 is reported by HP OMEN 15-en1 (88D2 / 4J8B4EA) with single-zone white Fn+F4 backlight
            2 | 6 => Self::White,
            3 => Self::PerKeyRgb,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Sans rétroéclairage WMI",
            Self::White => "Rétroéclairage blanc",
            Self::FourZoneRgb => "RGB 4 zones",
            Self::PerKeyRgb => "RGB par touche",
            Self::Unknown(_) => "Clavier OMEN",
        }
    }

    /// True only when the keyboard exposes software-controllable RGB via `hpqBIntM`.
    pub fn is_software_rgb(self) -> bool {
        matches!(self, Self::FourZoneRgb | Self::PerKeyRgb)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HpOmenStatus {
    pub detected: bool,
    pub is_admin: bool,
    pub wmi_accessible: bool,
    pub model: String,
    pub board_id: String,
    pub thermal_version: u8,
    pub thermal_mode: OmenThermalMode,
    pub max_fan: bool,
    pub manual_fan_pct: Option<u8>,
    pub fan_count: u8,
    pub fan_rpms: Vec<u32>,
    pub fan_max_rpms: Vec<u32>,
    pub gpu_power: OmenGpuPowerLevel,
    pub gpu_peak_temp_c: Option<u8>,
    pub keyboard_type: OmenKeyboardType,
    pub keyboard_label: String,
    pub backlight_on: bool,
    pub zone_colors: [Rgb; 4],
    pub diagnostic: String,
    pub probe_log: Vec<String>,
}

impl Default for HpOmenStatus {
    fn default() -> Self {
        Self {
            detected: false,
            is_admin: crate::is_elevated(),
            wmi_accessible: false,
            model: String::new(),
            board_id: String::new(),
            thermal_version: 1,
            thermal_mode: OmenThermalMode::Default,
            max_fan: false,
            manual_fan_pct: None,
            fan_count: 0,
            fan_rpms: Vec::new(),
            fan_max_rpms: Vec::new(),
            gpu_power: OmenGpuPowerLevel::Minimum,
            gpu_peak_temp_c: None,
            keyboard_type: OmenKeyboardType::None,
            keyboard_label: "—".into(),
            backlight_on: true,
            zone_colors: [Rgb::new(0, 212, 255); 4],
            diagnostic: String::new(),
            probe_log: Vec::new(),
        }
    }
}

pub(crate) fn thermal_policy_payload(version: u8, mode: OmenThermalMode) -> [u8; 2] {
    if version == 0 {
        match mode {
            OmenThermalMode::Default => [0xFF, 0x00],
            OmenThermalMode::Performance => [0xFF, 0x01],
            OmenThermalMode::Cool => [0xFF, 0x02],
        }
    } else {
        match mode {
            OmenThermalMode::Default => [0xFF, 0x30],
            OmenThermalMode::Performance => [0xFF, 0x31],
            OmenThermalMode::Cool => [0xFF, 0x50],
        }
    }
}

pub(crate) fn compute_fan_level_bytes(pct: Option<u8>, max_rpms: &[u32]) -> [u8; 2] {
    let Some(p) = pct else {
        return [0, 0];
    };
    let clamped = p.clamp(MANUAL_FAN_MIN_PCT, 100);
    let max_units = |idx: usize| -> u8 {
        max_rpms
            .get(idx)
            .copied()
            .filter(|&r| (1500..=10000).contains(&r))
            .map(|r| (r / 100).clamp(15, 100) as u8)
            .unwrap_or(DEFAULT_MAX_FAN_UNITS)
    };
    let u0 = ((clamped as u16 * max_units(0) as u16) / 100).clamp(15, 100) as u8;
    let u1 = ((clamped as u16 * max_units(1) as u16) / 100).clamp(15, 100) as u8;
    [u0, u1]
}

pub(crate) fn decode_zone_colors(table: &[u8]) -> Option<[Rgb; 4]> {
    if table.len() < COLOR_TABLE_ZONE_OFFSET + 12 {
        return None;
    }
    let slice = &table[COLOR_TABLE_ZONE_OFFSET..COLOR_TABLE_ZONE_OFFSET + 12];
    Some([
        Rgb::new(slice[0], slice[1], slice[2]),
        Rgb::new(slice[3], slice[4], slice[5]),
        Rgb::new(slice[6], slice[7], slice[8]),
        Rgb::new(slice[9], slice[10], slice[11]),
    ])
}

pub(crate) fn patch_color_table(base: Option<&[u8]>, colors: &[Rgb; 4]) -> Vec<u8> {
    let mut buf = match base {
        Some(b) if b.len() >= COLOR_TABLE_LEN => b[..COLOR_TABLE_LEN].to_vec(),
        Some(b) => {
            let mut v = vec![0u8; COLOR_TABLE_LEN];
            v[..b.len()].copy_from_slice(b);
            v
        }
        None => vec![0u8; COLOR_TABLE_LEN],
    };
    for (i, c) in colors.iter().enumerate() {
        let off = COLOR_TABLE_ZONE_OFFSET + i * 3;
        buf[off] = c.r;
        buf[off + 1] = c.g;
        buf[off + 2] = c.b;
    }
    buf
}

fn format_hex_preview(bytes: &[u8], limit: usize) -> String {
    bytes
        .iter()
        .take(limit)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn detect_hp_identity() -> (bool, String, String) {
    #[cfg(windows)]
    {
        let mut is_hp_gaming = false;
        let mut model = String::new();
        let mut board_id = String::new();
        if let Ok(cim) = crate::wmi_com::WmiConn::connect(r"root\cimv2") {
            let mut mfg = String::new();
            if let Ok(rows) = cim.query("SELECT Manufacturer, Model FROM Win32_ComputerSystem") {
                if let Some(row) = rows.first() {
                    mfg = row
                        .get("Manufacturer")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    model = row
                        .get("Model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
            }
            if let Ok(rows) = cim.query("SELECT Product FROM Win32_BaseBoard") {
                if let Some(p) = rows.first().and_then(|r| r.get("Product")).and_then(|v| v.as_str()) {
                    board_id = p.trim().to_string();
                }
            }
            let ml = model.to_ascii_lowercase();
            let fl = mfg.to_ascii_lowercase();
            if (fl.contains("hp") || fl.contains("hewlett-packard"))
                && (ml.contains("omen") || ml.contains("victus"))
            {
                is_hp_gaming = true;
            }
        }
        if !is_hp_gaming {
            if let Ok(wmi) = crate::wmi_com::WmiConn::connect(r"root\wmi") {
                if wmi.first_instance_path("hpqBIntM").ok().flatten().is_some() {
                    is_hp_gaming = true;
                }
            }
        }
        (is_hp_gaming, model, board_id)
    }
    #[cfg(not(windows))]
    (false, String::new(), String::new())
}

#[cfg(windows)]
fn query_hp_wmi(
    conn: &crate::wmi_com::WmiConn,
    method: &str,
    cmd: u32,
    cmd_type: u32,
    data: &[u8],
    log: &mut Vec<String>,
    label: &str,
) -> Option<(u32, Vec<u8>)> {
    match conn.exec_hpq_bios_int(method, cmd, cmd_type, data) {
        Ok((rc, bytes)) => {
            log.push(format!(
                "{label} (0x{cmd:X}/0x{cmd_type:02X}) -> rc=0x{rc:X} [{}]",
                format_hex_preview(&bytes, 24)
            ));
            Some((rc, bytes))
        }
        Err(e) => {
            log.push(format!("{label} (0x{cmd:X}/0x{cmd_type:02X}) -> ERR: {e}"));
            None
        }
    }
}

/// Non-destructive read-only probe of HP OMEN BIOS WMI capabilities and live fan/RGB state.
pub fn probe_hp_omen() -> HpOmenStatus {
    let mut st = HpOmenStatus::default();
    let (is_hp, model, board_id) = detect_hp_identity();
    st.detected = is_hp;
    st.model = model;
    st.board_id = board_id;
    st.is_admin = crate::is_elevated();

    if !is_hp {
        st.diagnostic = "Système non HP OMEN/Victus.".into();
        return st;
    }

    #[cfg(windows)]
    {
        if !st.is_admin {
            st.diagnostic =
                "HP OMEN détecté — relance OpenGear en Administrateur pour piloter les ventilateurs, la puissance GPU et le clavier 4 zones.".into();
            return st;
        }

        let conn = match crate::wmi_com::WmiConn::connect(r"root\wmi") {
            Ok(c) => c,
            Err(e) => {
                st.diagnostic = format!("Connexion COM root\\wmi impossible : {e}");
                return st;
            }
        };

        let mut log = Vec::new();
        let zero4 = [0u8; 4];

        // 1. System Design Data (thermal policy version + board flags)
        if let Some((rc, bytes)) =
            query_hp_wmi(&conn, "hpqBIOSInt128", HPWMI_GM, GM_GET_SYSTEM_DESIGN, &zero4, &mut log, "SystemDesign")
        {
            if rc == 0 {
                st.wmi_accessible = true;
                if bytes.len() > 3 {
                    // Boards like 88D2 use ThermalPolicy V1 (bytes[3] or fallback V1)
                    st.thermal_version = if bytes[3] == 0 { 0 } else { 1 };
                }
            }
        }

        // 2. Fan count
        if let Some((rc, bytes)) =
            query_hp_wmi(&conn, "hpqBIOSInt4", HPWMI_GM, GM_GET_FAN_COUNT, &zero4, &mut log, "FanCount")
        {
            if rc == 0 && !bytes.is_empty() {
                st.wmi_accessible = true;
                st.fan_count = bytes[0].min(4);
            }
        }

        // 3. Max Fan state
        if let Some((rc, bytes)) =
            query_hp_wmi(&conn, "hpqBIOSInt4", HPWMI_GM, GM_GET_MAX_FAN, &zero4, &mut log, "MaxFan")
        {
            if rc == 0 && !bytes.is_empty() {
                st.wmi_accessible = true;
                st.max_fan = bytes[0] != 0;
            }
        }

        // 4. Fan level (RPM/100) + Fan table fallback
        let (rpms, max_rpms) = read_fan_rpms_internal(&conn, Some(&mut log));
        if !rpms.is_empty() {
            st.wmi_accessible = true;
            st.fan_rpms = rpms;
            if st.fan_count == 0 {
                st.fan_count = st.fan_rpms.len() as u8;
            }
        }
        st.fan_max_rpms = max_rpms;

        // 5. GPU Dynamic Boost / Power mode ([cTGP, PPAB, dState, peakTemp])
        if let Some((rc, bytes)) =
            query_hp_wmi(&conn, "hpqBIOSInt4", HPWMI_GM, GM_GET_GPU_POWER, &zero4, &mut log, "GpuPower")
        {
            if rc == 0 && !bytes.is_empty() {
                st.gpu_power = OmenGpuPowerLevel::from_gpu_power_bytes(&bytes);
                if let Some(&peak) = bytes.get(3) {
                    if (50..=105).contains(&peak) {
                        st.gpu_peak_temp_c = Some(peak);
                    }
                }
            }
        }

        // 6. Keyboard type, backlight state, color table
        if let Some((rc, bytes)) =
            query_hp_wmi(&conn, "hpqBIOSInt128", HPWMI_KB, KB_GET_TYPE, &zero4, &mut log, "KbType")
        {
            if rc == 0 && !bytes.is_empty() {
                st.wmi_accessible = true;
                st.keyboard_type = OmenKeyboardType::from_byte(bytes[0]);
                st.keyboard_label = st.keyboard_type.label().to_string();
            }
        }

        if let Some((rc, bytes)) = query_hp_wmi(
            &conn,
            "hpqBIOSInt4",
            HPWMI_KB,
            KB_GET_BACKLIGHT,
            &zero4,
            &mut log,
            "KbBacklight",
        ) {
            if rc == 0 && !bytes.is_empty() {
                st.backlight_on = bytes[0] == BACKLIGHT_ON || (bytes[0] != BACKLIGHT_OFF && bytes[0] != 0);
            }
        }

        if let Some((rc, bytes)) = query_hp_wmi(
            &conn,
            "hpqBIOSInt128",
            HPWMI_KB,
            KB_GET_COLOR_TABLE,
            &zero4,
            &mut log,
            "KbColorTable",
        ) {
            if rc == 0 {
                if let Some(zones) = decode_zone_colors(&bytes) {
                    st.zone_colors = zones;
                    if matches!(st.keyboard_type, OmenKeyboardType::None | OmenKeyboardType::Unknown(_))
                    {
                        st.keyboard_type = OmenKeyboardType::FourZoneRgb;
                        st.keyboard_label = st.keyboard_type.label().to_string();
                    }
                }
            }
        }

        st.probe_log = log;
        st.diagnostic = if st.wmi_accessible {
            let low = st.model.to_ascii_lowercase();
            let prefix = if low.contains("omen") || low.contains("victus") {
                "HP"
            } else {
                "HP OMEN"
            };
            format!(
                "{prefix} {} ({}) connecté via BIOS WMI hpqBIntM (v{})",
                st.model, st.board_id, st.thermal_version
            )
        } else {
            "hpqBIntM inaccessible ou refusé par le BIOS.".into()
        };
    }

    st
}

pub(crate) fn parse_fan_level_rpms(bytes: &[u8]) -> Vec<u32> {
    bytes.iter().take(2).map(|&b| (b as u32) * 100).collect()
}

#[cfg(windows)]
fn read_fan_rpms_internal(
    conn: &crate::wmi_com::WmiConn,
    mut log: Option<&mut Vec<String>>,
) -> (Vec<u32>, Vec<u32>) {
    let zero4 = [0u8; 4];
    let mut rpms = Vec::new();
    let mut max_rpms = Vec::new();

    // 1. GM_GET_FAN_LEVEL (0x2D, read-only): returns RPM/100 in bytes[0], bytes[1]
    if let Ok((rc, ref bytes)) =
        conn.exec_hpq_bios_int("hpqBIOSInt128", HPWMI_GM, GM_GET_FAN_LEVEL, &zero4)
    {
        if let Some(ref mut l) = log {
            l.push(format!(
                "GetFanLevel (0x20008/0x2D) -> rc=0x{rc:X} [{}]",
                format_hex_preview(bytes, 16)
            ));
        }
        if rc == 0 && bytes.len() >= 2 {
            rpms = parse_fan_level_rpms(bytes);
        }
    }

    // 2. GM_GET_FAN_SPEED (0x11, read-only): prefer exact RPM (u16 LE) when supported
    for idx in 0..2u8 {
        if let Ok((rc, ref bytes)) =
            conn.exec_hpq_bios_int("hpqBIOSInt4", HPWMI_GM, GM_GET_FAN_SPEED, &[idx, 0, 0, 0])
        {
            if let Some(ref mut l) = log {
                l.push(format!(
                    "GetFanSpeed[{idx}] (0x20008/0x11) -> rc=0x{rc:X} [{}]",
                    format_hex_preview(bytes, 8)
                ));
            }
            if rc == 0 && bytes.len() >= 2 {
                let exact = u16::from_le_bytes([bytes[0], bytes[1]]) as u32;
                if (100..=10_000).contains(&exact) {
                    if let Some(slot) = rpms.get_mut(idx as usize) {
                        *slot = exact;
                    } else if rpms.len() == idx as usize {
                        rpms.push(exact);
                    }
                }
            }
        }
    }

    // 3. Query fan table (0x2F, read-only) on full probe to discover max RPM caps
    if log.is_some() {
        if let Ok((rc, ref bytes)) =
            conn.exec_hpq_bios_int("hpqBIOSInt128", HPWMI_GM, GM_GET_FAN_TABLE, &zero4)
        {
            if let Some(ref mut l) = log {
                l.push(format!(
                    "GetFanTable (0x20008/0x2F) -> rc=0x{rc:X} len={} [{}]",
                    bytes.len(),
                    format_hex_preview(bytes, 24)
                ));
            }
            if rc == 0 && bytes.len() >= 4 {
                let cap = bytes
                    .iter()
                    .copied()
                    .filter(|&b| (40..=75).contains(&b))
                    .max()
                    .unwrap_or(DEFAULT_MAX_FAN_UNITS) as u32
                    * 100;
                max_rpms = vec![cap, cap];
            }
        }
    }

    (rpms, max_rpms)
}

/// Fast periodic poll of HP OMEN fan RPMs and Max Fan state (returns `None` if unavailable/unprivileged).
pub fn read_hp_omen_live_fans() -> Option<(Vec<u32>, bool)> {
    #[cfg(windows)]
    {
        if !crate::is_elevated() {
            return None;
        }
        let conn = crate::wmi_com::WmiConn::connect(r"root\wmi").ok()?;
        let zero4 = [0u8; 4];
        let (rc_max, b_max) = conn
            .exec_hpq_bios_int("hpqBIOSInt4", HPWMI_GM, GM_GET_MAX_FAN, &zero4)
            .ok()?;
        let max_fan = rc_max == 0 && b_max.first().copied().unwrap_or(0) != 0;
        let (rpms, _) = read_fan_rpms_internal(&conn, None);
        if rpms.is_empty() && rc_max != 0 {
            return None;
        }
        Some((rpms, max_fan))
    }
    #[cfg(not(windows))]
    None
}

#[cfg(windows)]
fn check_omen_wmi_rc(action: &str, rc: u32) -> Result<()> {
    anyhow::ensure!(rc == 0, "HP OMEN BIOS WMI ({action}) a renvoyé le code erreur 0x{rc:X}");
    Ok(())
}

/// Set HP OMEN thermal/performance mode (Default / Performance / Cool).
pub fn set_hp_omen_thermal_mode(version: u8, mode: OmenThermalMode) -> Result<()> {
    #[cfg(windows)]
    {
        anyhow::ensure!(crate::is_elevated(), "Droits administrateur requis pour HP OMEN WMI");
        let conn = crate::wmi_com::WmiConn::connect(r"root\wmi")?;
        let payload = thermal_policy_payload(version, mode);
        let (rc, _) = conn.exec_hpq_bios_int("hpqBIOSInt0", HPWMI_GM, GM_SET_PERFORMANCE_MODE, &payload)?;
        if rc != 0 && version == 1 {
            // Fallback to V0 payload if a board expects 0x00/0x01/0x02
            let fallback = thermal_policy_payload(0, mode);
            let (rc2, _) =
                conn.exec_hpq_bios_int("hpqBIOSInt0", HPWMI_GM, GM_SET_PERFORMANCE_MODE, &fallback)?;
            return check_omen_wmi_rc("SetThermalMode(V0 fallback)", rc2);
        }
        check_omen_wmi_rc("SetThermalMode", rc)
    }
    #[cfg(not(windows))]
    {
        let _ = (version, mode);
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Toggle HP OMEN Max Fan boost mode.
pub fn set_hp_omen_max_fan(enabled: bool) -> Result<()> {
    #[cfg(windows)]
    {
        anyhow::ensure!(crate::is_elevated(), "Droits administrateur requis pour HP OMEN WMI");
        let conn = crate::wmi_com::WmiConn::connect(r"root\wmi")?;
        let payload = [u8::from(enabled), 0, 0, 0];
        let (rc, _) = conn.exec_hpq_bios_int("hpqBIOSInt0", HPWMI_GM, GM_SET_MAX_FAN, &payload)?;
        check_omen_wmi_rc("SetMaxFan", rc)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Set manual fan target percentage (`Some(30..=100)`) or release control to BIOS (`None`).
pub fn set_hp_omen_fan_level(pct: Option<u8>, max_rpms: &[u32]) -> Result<()> {
    #[cfg(windows)]
    {
        anyhow::ensure!(crate::is_elevated(), "Droits administrateur requis pour HP OMEN WMI");
        let conn = crate::wmi_com::WmiConn::connect(r"root\wmi")?;
        let levels = compute_fan_level_bytes(pct, max_rpms);
        let (rc, _) = conn.exec_hpq_bios_int("hpqBIOSInt0", HPWMI_GM, GM_SET_FAN_LEVEL, &levels)?;
        check_omen_wmi_rc("SetFanLevel", rc)
    }
    #[cfg(not(windows))]
    {
        let _ = (pct, max_rpms);
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Set NVIDIA Dynamic Boost / GPU power profile on HP OMEN (`Minimum`, `Medium`, `Maximum`).
pub fn set_hp_omen_gpu_power(level: OmenGpuPowerLevel) -> Result<()> {
    #[cfg(windows)]
    {
        anyhow::ensure!(crate::is_elevated(), "Droits administrateur requis pour HP OMEN WMI");
        let conn = crate::wmi_com::WmiConn::connect(r"root\wmi")?;
        let payload = level.payload();
        let (rc, _) = conn.exec_hpq_bios_int("hpqBIOSInt0", HPWMI_GM, GM_SET_GPU_POWER, &payload)?;
        check_omen_wmi_rc("SetGpuPower", rc)
    }
    #[cfg(not(windows))]
    {
        let _ = level;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Toggle HP OMEN keyboard backlight on (`0xE4`) or off (`0x64`), verifying read-back.
pub fn set_hp_omen_kb_backlight(enabled: bool) -> Result<()> {
    #[cfg(windows)]
    {
        anyhow::ensure!(crate::is_elevated(), "Droits administrateur requis pour HP OMEN WMI");
        let conn = crate::wmi_com::WmiConn::connect(r"root\wmi")?;
        let b = if enabled { BACKLIGHT_ON } else { BACKLIGHT_OFF };
        let payload4 = [b, 0, 0, 0];
        let (rc, _) = conn.exec_hpq_bios_int("hpqBIOSInt4", HPWMI_KB, KB_SET_BACKLIGHT, &payload4)?;
        if rc != 0 {
            let (rc2, _) =
                conn.exec_hpq_bios_int("hpqBIOSInt0", HPWMI_KB, KB_SET_BACKLIGHT, &[b])?;
            check_omen_wmi_rc("SetKbBacklight", rc2)?;
        }
        if let Ok((rc_rb, rb)) =
            conn.exec_hpq_bios_int("hpqBIOSInt4", HPWMI_KB, KB_GET_BACKLIGHT, &[0u8; 4])
        {
            if rc_rb == 0 && !rb.is_empty() {
                let actual_on =
                    rb[0] == BACKLIGHT_ON || (rb[0] != BACKLIGHT_OFF && rb[0] != 0);
                anyhow::ensure!(
                    actual_on == enabled,
                    "Ce clavier HP OMEN (rétroéclairage blanc seul) est géré matériellement par Fn + F4 : le BIOS ignore la commande logicielle 0x20009 (lu 0x{:02X}).",
                    rb[0]
                );
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        anyhow::bail!("Non supporté hors Windows")
    }
}

/// Write 4-zone RGB keyboard colors into the 128-byte BIOS color table and return the updated table.
pub fn set_hp_omen_kb_zones(colors: [Rgb; 4], cached_table: Option<&[u8]>) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        anyhow::ensure!(crate::is_elevated(), "Droits administrateur requis pour HP OMEN WMI");
        let conn = crate::wmi_com::WmiConn::connect(r"root\wmi")?;
        let current_owned;
        let base = match cached_table {
            Some(c) if c.len() >= COLOR_TABLE_LEN => Some(c),
            _ => {
                let (rc, tbl) = conn
                    .exec_hpq_bios_int("hpqBIOSInt128", HPWMI_KB, KB_GET_COLOR_TABLE, &[0u8; 4])
                    .context("Lecture ColorTable HP OMEN impossible")?;
                check_omen_wmi_rc("GetColorTable", rc)?;
                current_owned = tbl;
                Some(current_owned.as_slice())
            }
        };
        let patched = patch_color_table(base, &colors);
        let (rc, _) =
            conn.exec_hpq_bios_int("hpqBIOSInt128", HPWMI_KB, KB_SET_COLOR_TABLE, &patched)?;
        check_omen_wmi_rc("SetColorTable", rc)?;
        Ok(patched)
    }
    #[cfg(not(windows))]
    {
        let _ = (colors, cached_table);
        anyhow::bail!("Non supporté hors Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thermal_payloads_match_v0_and_v1_specs() {
        assert_eq!(thermal_policy_payload(0, OmenThermalMode::Default), [0xFF, 0x00]);
        assert_eq!(thermal_policy_payload(0, OmenThermalMode::Performance), [0xFF, 0x01]);
        assert_eq!(thermal_policy_payload(0, OmenThermalMode::Cool), [0xFF, 0x02]);
        assert_eq!(thermal_policy_payload(1, OmenThermalMode::Default), [0xFF, 0x30]);
        assert_eq!(thermal_policy_payload(1, OmenThermalMode::Performance), [0xFF, 0x31]);
        assert_eq!(thermal_policy_payload(1, OmenThermalMode::Cool), [0xFF, 0x50]);
    }

    #[test]
    fn color_table_patches_and_decodes_4_zones_at_offset_25() {
        let mut init = vec![0xAAu8; 128];
        let cols = [
            Rgb::new(10, 20, 30),
            Rgb::new(40, 50, 60),
            Rgb::new(70, 80, 90),
            Rgb::new(100, 110, 120),
        ];
        init[10] = 0x42;
        let out = patch_color_table(Some(&init), &cols);
        assert_eq!(out.len(), 128);
        assert_eq!(out[10], 0x42, "unrelated table bytes must stay intact");
        assert_eq!(decode_zone_colors(&out), Some(cols));
    }

    #[test]
    fn fan_level_enforces_safety_floor_and_auto_zero() {
        assert_eq!(compute_fan_level_bytes(None, &[5500, 5500]), [0, 0]);
        let floored = compute_fan_level_bytes(Some(5), &[5500, 5500]);
        assert!(floored[0] >= 15 && floored[1] >= 15);
        assert_eq!(compute_fan_level_bytes(Some(100), &[6000, 5000]), [60, 50]);
    }

    #[test]
    fn keyboard_type_and_gpu_power_round_trip() {
        assert_eq!(OmenKeyboardType::from_byte(1), OmenKeyboardType::FourZoneRgb);
        assert_eq!(OmenKeyboardType::from_byte(6), OmenKeyboardType::White);
        assert_eq!(OmenKeyboardType::from_byte(2), OmenKeyboardType::White);
        assert_eq!(
            OmenGpuPowerLevel::from_gpu_power_bytes(&[0x00, 0x01, 0x01, 0x57]),
            OmenGpuPowerLevel::Medium
        );
        assert_eq!(
            OmenGpuPowerLevel::from_gpu_power_bytes(&[0x01, 0x01, 0x01, 0x57]),
            OmenGpuPowerLevel::Maximum
        );
        assert_eq!(OmenGpuPowerLevel::Minimum.payload(), [0x00, 0x00, 0x01, 0x00]);
        assert_eq!(OmenGpuPowerLevel::Medium.payload(), [0x00, 0x01, 0x01, 0x00]);
        assert_eq!(OmenGpuPowerLevel::Maximum.payload(), [0x01, 0x01, 0x01, 0x00]);
        assert_eq!(parse_fan_level_rpms(&[0x1B, 0x00]), vec![2700, 0]);
        assert_eq!(BACKLIGHT_ON, 0xE4);
        assert_eq!(BACKLIGHT_OFF, 0x64);
    }
}
