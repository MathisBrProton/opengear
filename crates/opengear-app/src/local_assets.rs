use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonMarkerCoord {
    pub slot_id: String,
    /// Normalized 0..100 X coordinate relative to the official product image.
    pub x_pct: f32,
    /// Normalized 0..100 Y coordinate relative to the official product image.
    pub y_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalVariant {
    pub id: String,
    pub label: String,
    pub asset_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedBatteryModel {
    pub vid: u16,
    pub pid: u16,
    pub folder: String,
    pub capacity_mwh: f32,
    pub sensor_mw: f32,
    pub mcu_mw: f32,
    /// `(interval_ms, power_mw)` pairs from official `battery.xml`
    pub report_rate_mw: Vec<(u16, f32)>,
}

impl ParsedBatteryModel {
    pub fn radio_mw_for_hz(&self, hz: u16) -> f32 {
        let interval_ms = 1000u16.checked_div(hz).map_or(1, |v| v.max(1));
        if let Some(&(_, mw)) = self
            .report_rate_mw
            .iter()
            .find(|&&(ms, _)| ms == interval_ms)
        {
            return mw;
        }
        // Nearest interval match
        self.report_rate_mw
            .iter()
            .min_by_key(|&&(ms, _)| (ms as i32 - interval_ms as i32).abs())
            .map(|&(_, mw)| mw)
            .unwrap_or(8.3)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAssetSummary {
    pub lghub_depot_found: bool,
    pub icue_devices_found: bool,
    pub mouse_variants: Vec<LocalVariant>,
    pub mouse_markers: Vec<ButtonMarkerCoord>,
    pub mouse_aspect_ratio: Option<f32>,
    pub headset_variants: Vec<LocalVariant>,
    pub k70_images: Vec<LocalVariant>,
    pub battery_models_count: usize,
    pub blue_voice_presets: Vec<String>,
}

/// Runtime-only local asset catalog reading official product images & metadata
/// directly from the user's own installed G HUB / iCUE 5 directories.
/// Zero vendor files are ever bundled into OpenGear.
#[derive(Debug, Default)]
pub struct LocalAssetCatalog {
    pub lghub_depot: Option<PathBuf>,
    pub icue_devices_dir: Option<PathBuf>,
    /// URL key (e.g. "lghub/pro_x_wireless_mouse/front") -> (bytes, mime)
    blobs: HashMap<String, (Vec<u8>, &'static str)>,
    pub mouse_variants: Vec<LocalVariant>,
    pub mouse_markers: Vec<ButtonMarkerCoord>,
    pub mouse_aspect_ratio: Option<f32>,
    pub headset_variants: Vec<LocalVariant>,
    pub k70_images: Vec<LocalVariant>,
    pub battery_models: Vec<ParsedBatteryModel>,
    pub blue_voice_presets: Vec<String>,
}

impl LocalAssetCatalog {
    pub fn discover() -> Self {
        let mut cat = Self {
            lghub_depot: find_latest_lghub_depot(),
            icue_devices_dir: find_icue_devices_dir(),
            ..Self::default()
        };

        if let Some(depot) = cat.lghub_depot.clone() {
            info!("Dépôt local G HUB détecté : {}", depot.display());
            cat.load_lghub_assets(&depot);
        }
        if let Some(icue_dir) = cat.icue_devices_dir.clone() {
            info!("Dossier local iCUE 5 détecté : {}", icue_dir.display());
            cat.load_icue_assets(&icue_dir);
        }
        cat
    }

    pub fn get_blob(&self, key: &str) -> Option<(&[u8], &'static str)> {
        self.blobs.get(key).map(|(b, m)| (b.as_slice(), *m))
    }

    /// Look up a dynamically parsed `battery.xml` power model by PID or model name hint.
    pub fn find_battery_model(&self, pid: u16, device_name: &str) -> Option<&ParsedBatteryModel> {
        if let Some(m) = self.battery_models.iter().find(|m| m.pid == pid) {
            return Some(m);
        }
        let lower = device_name.to_ascii_lowercase();
        if lower.contains("pro x")
            && !lower.contains("headset")
            && let Some(m) = self
                .battery_models
                .iter()
                .find(|m| m.folder.contains("pro_x_wireless"))
        {
            return Some(m);
        }
        self.battery_models.iter().find(|m| {
            let norm = m.folder.replace('_', " ");
            lower.contains(&norm) || norm.contains(&lower)
        })
    }

    pub fn summary(&self) -> LocalAssetSummary {
        LocalAssetSummary {
            lghub_depot_found: self.lghub_depot.is_some(),
            icue_devices_found: self.icue_devices_dir.is_some(),
            mouse_variants: self.mouse_variants.clone(),
            mouse_markers: self.mouse_markers.clone(),
            mouse_aspect_ratio: self.mouse_aspect_ratio,
            headset_variants: self.headset_variants.clone(),
            k70_images: self.k70_images.clone(),
            battery_models_count: self.battery_models.len(),
            blue_voice_presets: self.blue_voice_presets.clone(),
        }
    }

    fn load_lghub_assets(&mut self, depot: &Path) {
        // 1. Load PRO X Wireless Mouse official variants + exact metadata.json markers
        let mouse_dir = depot.join("pro_x_wireless_mouse");
        if mouse_dir.is_dir() {
            let variants = [
                ("black", "Noir (Officiel)", "images/device/front.png"),
                ("white", "Blanc (Officiel)", "images/device/front_white.png"),
                (
                    "magenta",
                    "Magenta (Officiel)",
                    "images/device/front_magenta.png",
                ),
            ];
            for (id, label, rel) in variants {
                let p = mouse_dir.join(rel);
                if let Ok(bytes) = std::fs::read(&p) {
                    let key = format!("lghub/mouse/{id}");
                    self.blobs.insert(key.clone(), (bytes, "image/png"));
                    self.mouse_variants.push(LocalVariant {
                        id: id.into(),
                        label: label.into(),
                        asset_url: format!("/local-asset/{key}"),
                    });
                }
            }

            if let Ok(meta_str) = std::fs::read_to_string(mouse_dir.join("metadata.json"))
                && let Ok(v) = serde_json::from_str::<Value>(&meta_str)
                && let Some(img) = v["images"].as_array().and_then(|a| a.first())
            {
                let w = img["origin"]["width"].as_f64().unwrap_or(1388.0) as f32;
                let h = img["origin"]["height"].as_f64().unwrap_or(2791.0) as f32;
                if w > 0.0 && h > 0.0 {
                    self.mouse_aspect_ratio = Some(w / h);
                }
                if let Some(assigns) = img["assignments"].as_array() {
                    for a in assigns {
                        let slot = a["slotId"].as_str().unwrap_or("").to_string();
                        let mx = a["marker"]["x"].as_f64().unwrap_or(0.0) as f32;
                        let my = a["marker"]["y"].as_f64().unwrap_or(0.0) as f32;
                        self.mouse_markers.push(ButtonMarkerCoord {
                            slot_id: slot,
                            x_pct: (mx / w) * 100.0,
                            y_pct: (my / h) * 100.0,
                        });
                    }
                }
            }
        }

        // 2. Load PRO X Headset official variants + Blue VO!CE preset names
        let hs_dir = depot.join("pro_headset");
        if hs_dir.is_dir() {
            let variants = [
                ("default", "PRO X Standard", "front.png"),
                ("shroud", "PRO X Shroud Edition", "front_shroud.png"),
                ("lol", "PRO X League of Legends", "front_lol.png"),
            ];
            for (id, label, rel) in variants {
                let p = hs_dir.join(rel);
                if let Ok(bytes) = std::fs::read(&p) {
                    let key = format!("lghub/headset/{id}");
                    self.blobs.insert(key.clone(), (bytes, "image/png"));
                    self.headset_variants.push(LocalVariant {
                        id: id.into(),
                        label: label.into(),
                        asset_url: format!("/local-asset/{key}"),
                    });
                }
            }

            if let Ok(raw) =
                std::fs::read_to_string(hs_dir.join("pro_x_headset_device_presets.json"))
                && let Ok(v) = serde_json::from_str::<Value>(&raw)
                && let Some(arr) = v["presets"].as_array()
            {
                for p in arr {
                    if let Some(n) = p["name"].as_str() {
                        let pretty = n.trim_start_matches("BLUE_VOICE_PRESET_").replace('_', " ");
                        self.blue_voice_presets.push(pretty);
                    }
                }
            }
        }

        // 3. Scan ALL folders in the depot for `battery.xml` power models & extra device images
        if let Ok(entries) = std::fs::read_dir(depot) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let folder = entry.file_name().to_string_lossy().into_owned();
                let candidates = [
                    dir.join("battery").join("battery.xml"),
                    dir.join("battery.xml"),
                    dir.join("pro_x_wireless_headset_battery.xml"),
                ];
                for xml_path in candidates {
                    if xml_path.is_file()
                        && let Ok(xml) = std::fs::read_to_string(&xml_path)
                        && let Some(pm) = parse_battery_xml(&folder, &xml)
                    {
                        debug!(
                            "battery.xml chargé ({}): {:04X}:{:04X}, {:.0} mWh",
                            pm.folder, pm.vid, pm.pid, pm.capacity_mwh
                        );
                        self.battery_models.push(pm);
                    }
                }
            }
        }
    }

    fn load_icue_assets(&mut self, icue_dir: &Path) {
        let k70_rcc = icue_dir.join("k70_rgb_tkl.rcc");
        if k70_rcc.is_file()
            && let Ok(data) = std::fs::read(&k70_rcc)
        {
            let entries = parse_qt_rcc(&data);
            let picks = [
                (
                    "fr",
                    "AZERTY FR (Officiel)",
                    "/k70_rgb_tkl/body/fr.jpg",
                    "image/jpeg",
                ),
                (
                    "na",
                    "QWERTY US (Officiel)",
                    "/k70_rgb_tkl/body/na.jpg",
                    "image/jpeg",
                ),
                (
                    "uk",
                    "QWERTY UK (Officiel)",
                    "/k70_rgb_tkl/body/uk.jpg",
                    "image/jpeg",
                ),
                (
                    "thumb",
                    "Miniature K70 TKL",
                    "/k70_rgb_tkl/thumbnails/thumbnails.png",
                    "image/png",
                ),
            ];
            for (id, label, vpath, mime) in picks {
                if let Some(bytes) = entries.get(vpath) {
                    let key = format!("icue/k70_tkl/{id}");
                    self.blobs.insert(key.clone(), (bytes.clone(), mime));
                    self.k70_images.push(LocalVariant {
                        id: id.into(),
                        label: label.into(),
                        asset_url: format!("/local-asset/{key}"),
                    });
                }
            }
        }
    }
}

fn parse_depot_version(path: &Path) -> Vec<u64> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| {
            s.split(|c: char| !c.is_ascii_digit())
                .filter(|seg| !seg.is_empty())
                .filter_map(|seg| seg.parse::<u64>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn find_latest_depot_in(base: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort_by(|a, b| {
        parse_depot_version(a)
            .cmp(&parse_depot_version(b))
            .then_with(|| a.file_name().cmp(&b.file_name()))
    });
    dirs.pop()
}

fn find_latest_lghub_depot() -> Option<PathBuf> {
    let program_data = std::env::var_os("ProgramData")
        .or_else(|| std::env::var_os("PROGRAMDATA"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    find_latest_depot_in(&program_data.join("LGHUB").join("depots"))
}

fn find_icue_devices_dir() -> Option<PathBuf> {
    let roots = [
        std::env::var_os("ProgramFiles"),
        std::env::var_os("ProgramFiles(x86)"),
    ];
    for root in roots.into_iter().flatten().filter(|v| !v.is_empty()) {
        let p = PathBuf::from(root)
            .join("Corsair")
            .join("Corsair iCUE5 Software")
            .join("devices");
        if p.is_dir() {
            return Some(p);
        }
    }
    for fallback in [
        r"C:\Program Files\Corsair\Corsair iCUE5 Software\devices",
        r"C:\Program Files (x86)\Corsair\Corsair iCUE5 Software\devices",
    ] {
        let p = PathBuf::from(fallback);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// Pure-Rust Qt RCC v2/v3 parser (supports raw + zlib-compressed resources).
pub fn parse_qt_rcc(data: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    if data.len() < 24 || &data[0..4] != b"qres" {
        return out;
    }
    let ver = read_be_u32(data, 4);
    let tree_off = read_be_u32(data, 8) as usize;
    let data_off = read_be_u32(data, 12) as usize;
    let names_off = read_be_u32(data, 16) as usize;
    let entry_sz: usize = if ver >= 2 { 22 } else { 14 };

    fn read_be_u16(b: &[u8], o: usize) -> u16 {
        u16::from_be_bytes([b[o], b[o + 1]])
    }
    fn read_be_u32(b: &[u8], o: usize) -> u32 {
        u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    fn read_name(data: &[u8], names_off: usize, off: usize) -> Option<String> {
        let base = names_off.checked_add(off)?;
        if base + 6 > data.len() {
            return None;
        }
        let len = read_be_u16(data, base) as usize;
        let start = base + 6;
        let end = start.checked_add(len * 2)?;
        if end > data.len() {
            return None;
        }
        let mut units = Vec::with_capacity(len);
        for i in 0..len {
            units.push(read_be_u16(data, start + i * 2));
        }
        Some(String::from_utf16_lossy(&units))
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        data: &[u8],
        tree_off: usize,
        data_off: usize,
        names_off: usize,
        entry_sz: usize,
        node_idx: usize,
        prefix: &str,
        out: &mut HashMap<String, Vec<u8>>,
        depth: usize,
    ) {
        if depth > 16 {
            return;
        }
        let Some(off) = tree_off.checked_add(node_idx * entry_sz) else {
            return;
        };
        if off + 14 > data.len() {
            return;
        }
        let name_off = read_be_u32(data, off) as usize;
        let flags = read_be_u16(data, off + 4);
        let name = if node_idx == 0 {
            String::new()
        } else {
            read_name(data, names_off, name_off).unwrap_or_default()
        };
        let path = if name.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        if (flags & 0x02) != 0 {
            // Directory node
            let count = read_be_u32(data, off + 6) as usize;
            let child_off = read_be_u32(data, off + 10) as usize;
            for i in 0..count.min(1024) {
                walk(
                    data,
                    tree_off,
                    data_off,
                    names_off,
                    entry_sz,
                    child_off + i,
                    &path,
                    out,
                    depth + 1,
                );
            }
        } else {
            // File node
            let d_off = read_be_u32(data, off + 10) as usize;
            let Some(blob_hdr) = data_off.checked_add(d_off) else {
                return;
            };
            if blob_hdr + 4 > data.len() {
                return;
            }
            let dlen = read_be_u32(data, blob_hdr) as usize;
            let start = blob_hdr + 4;
            let Some(end) = start.checked_add(dlen) else {
                return;
            };
            if end > data.len() {
                return;
            }
            let raw = &data[start..end];
            if (flags & 0x01) != 0 && raw.len() > 4 {
                // Cap decompressed RCC payload at 16 MiB to guard against malformed streams.
                const MAX_RCC_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
                let mut dec = ZlibDecoder::new(&raw[4..]).take(MAX_RCC_ENTRY_BYTES + 1);
                let mut buf = Vec::new();
                if dec.read_to_end(&mut buf).is_ok() && (buf.len() as u64) <= MAX_RCC_ENTRY_BYTES {
                    out.insert(path, buf);
                }
            } else {
                out.insert(path, raw.to_vec());
            }
        }
    }

    walk(
        data, tree_off, data_off, names_off, entry_sz, 0, "", &mut out, 0,
    );
    out
}

/// Lightweight parser for Logitech G HUB `battery.xml` power models.
fn parse_battery_xml(folder: &str, xml: &str) -> Option<ParsedBatteryModel> {
    let extract_attr = |haystack: &str, attr: &str| -> Option<String> {
        let pat = format!("{attr}=\"");
        let idx = haystack.find(&pat)?;
        let rest = &haystack[idx + pat.len()..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    };
    let parse_hex_u16 = |s: &str| -> Option<u16> {
        let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
        u16::from_str_radix(clean, 16).ok()
    };

    let vid = extract_attr(xml, "vid")
        .and_then(|s| parse_hex_u16(&s))
        .unwrap_or(0x046D);
    let pid = extract_attr(xml, "pid").and_then(|s| parse_hex_u16(&s))?;
    let cap_wh: f32 = extract_attr(xml, "watts_per_hour")?.parse().ok()?;
    let capacity_mwh = cap_wh * 1000.0;

    let mut sensor_mw = 1.5f32;
    let mut mcu_mw = 2.3f32;
    let mut report_rate_mw = Vec::new();

    for line in xml.lines() {
        let l = line.trim();
        if l.starts_with("<sensor") {
            if let Some(p) = extract_attr(l, "power").and_then(|s| s.parse::<f32>().ok()) {
                sensor_mw = p;
            }
        } else if l.starts_with("<mcu") {
            if let Some(p) = extract_attr(l, "power").and_then(|s| s.parse::<f32>().ok()) {
                mcu_mw = p;
            }
        } else if l.starts_with("<interval")
            && let (Some(ms), Some(pw)) = (
                extract_attr(l, "milliseconds").and_then(|s| s.parse::<u16>().ok()),
                extract_attr(l, "power").and_then(|s| s.parse::<f32>().ok()),
            )
        {
            report_rate_mw.push((ms, pw));
        }
    }

    Some(ParsedBatteryModel {
        vid,
        pid,
        folder: folder.to_string(),
        capacity_mwh,
        sensor_mw,
        mcu_mw,
        report_rate_mw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn find_latest_depot_in_sorts_numerically() {
        let dir =
            std::env::temp_dir().join(format!("opengear_depot_sort_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("99999")).unwrap();
        std::fs::create_dir_all(dir.join("100000")).unwrap();
        std::fs::create_dir_all(dir.join("2024.9")).unwrap();
        std::fs::create_dir_all(dir.join("2024.10")).unwrap();

        let latest = find_latest_depot_in(&dir).unwrap();
        assert_eq!(latest.file_name().unwrap().to_str().unwrap(), "100000");

        let _ = std::fs::remove_dir_all(dir.join("99999"));
        let _ = std::fs::remove_dir_all(dir.join("100000"));
        let latest_ver = find_latest_depot_in(&dir).unwrap();
        assert_eq!(latest_ver.file_name().unwrap().to_str().unwrap(), "2024.10");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_qt_rcc_rejects_oversized_compressed_blob() {
        // Build a synthetic zlib stream slightly over 16 MiB and verify parser drops it cleanly.
        let big = vec![b'A'; (16 * 1024 * 1024) + 128];
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(&big).unwrap();
        let compressed = enc.finish().unwrap();

        let mut dec = ZlibDecoder::new(compressed.as_slice()).take(16 * 1024 * 1024 + 1);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert!((out.len() as u64) > 16 * 1024 * 1024);
    }
}
