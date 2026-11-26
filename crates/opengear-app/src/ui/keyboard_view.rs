//! Keyboard drawn in the key editor (remap, macros, per-key painting).
//!
//! Geometry: the exact map of a connected keyboard when its driver provides one (Corsair today),
//! otherwise a generic board chosen by form factor and physical standard. Labels and binding
//! identifiers always come from the **active Windows layout**, so the editor matches any keyboard
//! in any language, and the key the user clicks is the key the input hook will match.

use opengear_core::keyboard::{char_key_scan, default_label};
use opengear_core::{AppConfig, FormFactor, KeyboardLayout, physical_layout};
use opengear_input::{fixed_vk_for_code, us_vk_for_code, vk_to_key_name};
use opengear_platform::KeyResolver;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub(crate) struct KeyView {
    /// Physical position (W3C code), `None` for LED-only zones.
    pub code: Option<&'static str>,
    /// Binding identifier understood by the input engine (`None`: not remappable).
    pub id: Option<&'static str>,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// LED index in the device frame, when the keyboard has per-key lighting.
    pub led_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct KeyboardView {
    /// Name of the keyboard whose exact map is drawn, `None` for a generic board.
    pub device_name: Option<String>,
    pub form_factor: FormFactor,
    pub form_factor_auto: bool,
    pub standard: KeyboardLayout,
    pub standard_auto: bool,
    pub os_top_row: String,
    pub os_lang_id: u16,
    pub width: f32,
    pub height: f32,
    pub keys: Vec<KeyView>,
}

fn standard_id(s: KeyboardLayout) -> &'static str {
    match s {
        KeyboardLayout::Ansi => "ansi",
        KeyboardLayout::Iso => "iso",
        KeyboardLayout::Abnt => "abnt",
        KeyboardLayout::Jis => "jis",
        KeyboardLayout::Unknown(_) => "unknown",
    }
}

/// `"ansi"` / `"iso"` / `"auto"` (-> `None`) from the UI.
pub(crate) fn parse_standard(id: &str) -> Option<KeyboardLayout> {
    match id {
        "ansi" => Some(KeyboardLayout::Ansi),
        "iso" => Some(KeyboardLayout::Iso),
        "abnt" => Some(KeyboardLayout::Abnt),
        "jis" => Some(KeyboardLayout::Jis),
        _ => None,
    }
}

/// Binding id and label of one physical key under the active Windows layout.
fn resolve_key(
    code: &'static str,
    resolver: &KeyResolver,
    iso: bool,
) -> (Option<&'static str>, String) {
    if let Some(vk) = fixed_vk_for_code(code) {
        let label = if code == "AltRight" && iso {
            "AltGr"
        } else {
            default_label(code)
        };
        return (vk_to_key_name(vk), label.to_string());
    }
    if let Some(scan) = char_key_scan(code) {
        if let Some((vk, label)) = resolver.resolve(scan) {
            return (vk_to_key_name(vk), label);
        }
        return (
            us_vk_for_code(code).and_then(vk_to_key_name),
            default_label(code).to_string(),
        );
    }
    (None, default_label(code).to_string())
}

impl KeyboardView {
    pub(crate) fn build(
        config: &AppConfig,
        devices: &[Box<dyn opengear_core::driver::Device>],
        is_laptop: bool,
    ) -> Self {
        let resolver = KeyResolver::for_foreground();
        let os = resolver.describe();

        let form_factor_auto = config.keyboard_form_factor.is_none();
        let form_factor = config.keyboard_form_factor.unwrap_or(if is_laptop {
            FormFactor::Compact75
        } else {
            FormFactor::Full
        });

        let device_layout = devices.iter().find_map(|dev| dev.keyboard_layout());

        // The device knows its own physical standard; otherwise the user choice, otherwise the
        // usual standard for the Windows language.
        let device_std = device_layout
            .as_ref()
            .map(|l| l.standard)
            .filter(|l| !matches!(l, KeyboardLayout::Unknown(_)));
        let standard_auto = device_std.is_some() || config.keyboard_standard.is_none();
        let standard = device_std
            .or(config.keyboard_standard)
            .unwrap_or(os.suggested_standard);
        let iso = !matches!(standard, KeyboardLayout::Ansi);

        let (device_name, keys, width, height) = match device_layout {
            Some(layout) => {
                let keys: Vec<KeyView> = layout
                    .keys
                    .into_iter()
                    .map(|k| {
                        let (id, label) = match k.code {
                            Some(code) => resolve_key(code, &resolver, iso),
                            None => (None, k.fallback_label.to_string()),
                        };
                        KeyView {
                            code: k.code,
                            id,
                            label,
                            x: k.x,
                            y: k.y,
                            w: k.width,
                            h: k.height,
                            led_index: Some(k.led_index),
                        }
                    })
                    .collect();
                let width = keys.iter().map(|k| k.x + k.w).fold(0.0, f32::max);
                let height = keys.iter().map(|k| k.y + k.h).fold(0.0, f32::max);
                (Some(layout.product_name), keys, width, height)
            }
            None => {
                let (phys, width, height) = physical_layout(form_factor, standard);
                let keys = phys
                    .into_iter()
                    .map(|p| {
                        let (id, label) = resolve_key(p.code, &resolver, iso);
                        KeyView {
                            code: Some(p.code),
                            id,
                            label,
                            x: p.x,
                            y: p.y,
                            w: p.w,
                            h: p.h,
                            led_index: None,
                        }
                    })
                    .collect();
                (None, keys, width, height)
            }
        };

        Self {
            device_name,
            form_factor,
            form_factor_auto,
            standard,
            standard_auto,
            os_top_row: os.top_row,
            os_lang_id: os.lang_id,
            width,
            height,
            keys,
        }
    }

    /// Normalised centre (0..1) of the key bound to `id`, for position-based effects.
    pub(crate) fn norm_center(&self, id: &str) -> Option<(f32, f32)> {
        self.keys
            .iter()
            .find(|k| k.id.is_some_and(|kid| kid.eq_ignore_ascii_case(id)))
            .map(|k| self.norm(k))
    }

    fn norm(&self, k: &KeyView) -> (f32, f32) {
        (
            ((k.x + k.w * 0.5) / self.width.max(1.0)).clamp(0.0, 1.0),
            ((k.y + k.h * 0.5) / self.height.max(1.0)).clamp(0.0, 1.0),
        )
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "device_name": self.device_name,
            "form_factor": self.form_factor.id(),
            "form_factor_auto": self.form_factor_auto,
            "form_factors": FormFactor::ALL.iter().map(|f| json!({ "id": f.id(), "label": f.label() })).collect::<Vec<_>>(),
            "standard": standard_id(self.standard),
            "standard_auto": self.standard_auto,
            "os_layout": { "top_row": self.os_top_row, "lang_id": format!("{:04X}", self.os_lang_id) },
            "width": self.width,
            "height": self.height,
            "keys": self.keys.iter().map(|k| {
                let (nx, ny) = self.norm(k);
                json!({
                    "code": k.code,
                    "id": k.id,
                    "label": k.label,
                    "x": k.x, "y": k.y, "w": k.w, "h": k.h,
                    "led_index": k.led_index,
                    "norm_x": nx, "norm_y": ny,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_board_follows_config_and_every_real_key_is_bindable() {
        let cfg = AppConfig {
            keyboard_form_factor: Some(FormFactor::Tkl),
            keyboard_standard: Some(KeyboardLayout::Iso),
            ..AppConfig::default()
        };
        let v = KeyboardView::build(&cfg, &[], false);
        assert!(v.device_name.is_none());
        assert_eq!(v.keys.len(), 88);
        assert!(!v.form_factor_auto && !v.standard_auto);
        for k in &v.keys {
            assert!(
                k.id.is_some(),
                "{:?} sans identifiant d'assignation",
                k.code
            );
            assert!(k.led_index.is_none());
        }
        let esc = v.norm_center("ESC").unwrap();
        assert!(esc.0 < 0.1 && esc.1 < 0.2);
    }

    #[test]
    fn auto_form_factor_depends_on_chassis() {
        let cfg = AppConfig::default();
        assert_eq!(
            KeyboardView::build(&cfg, &[], true).form_factor,
            FormFactor::Compact75
        );
        assert_eq!(
            KeyboardView::build(&cfg, &[], false).form_factor,
            FormFactor::Full
        );
        assert_eq!(parse_standard("auto"), None);
        assert_eq!(parse_standard("iso"), Some(KeyboardLayout::Iso));
    }

    /// Writes the view built on this machine to `$OPENGEAR_KB_JSON` (UI rendering checks).
    #[test]
    #[ignore]
    fn dump_view_json() {
        let path = std::env::var("OPENGEAR_KB_JSON").expect("OPENGEAR_KB_JSON");
        let laptop = std::env::var("OPENGEAR_KB_LAPTOP").is_ok();
        let v = KeyboardView::build(&AppConfig::default(), &[], laptop);
        std::fs::write(path, v.to_json().to_string()).unwrap();
    }
}
