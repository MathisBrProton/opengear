use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
    };
    pub const RED: Self = Self { r: 255, g: 0, b: 0 };
    pub const GREEN: Self = Self { r: 0, g: 255, b: 0 };
    pub const BLUE: Self = Self { r: 0, g: 0, b: 255 };
    pub const CYAN: Self = Self {
        r: 0,
        g: 210,
        b: 255,
    };
    pub const CORSAIR_YELLOW: Self = Self {
        r: 255,
        g: 200,
        b: 0,
    };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        // Byte-indexed slicing below: reject non-ASCII input instead of panicking on a char boundary.
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Self { r, g, b })
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn scale(self, factor: f32) -> Self {
        let f = factor.clamp(0.0, 1.0);
        Self {
            r: (self.r as f32 * f).round() as u8,
            g: (self.g as f32 * f).round() as u8,
            b: (self.b as f32 * f).round() as u8,
        }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: (self.r as f32 + (other.r as f32 - self.r as f32) * t).round() as u8,
            g: (self.g as f32 + (other.g as f32 - self.g as f32) * t).round() as u8,
            b: (self.b as f32 + (other.b as f32 - self.b as f32) * t).round() as u8,
        }
    }

    /// Hue in [0.0, 360.0), sat in [0.0, 1.0], val in [0.0, 1.0]
    pub fn from_hsv(hue: f32, sat: f32, val: f32) -> Self {
        let h = ((hue % 360.0) + 360.0) % 360.0;
        let s = sat.clamp(0.0, 1.0);
        let v = val.clamp(0.0, 1.0);
        let c = v * s;
        let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
        let m = v - c;
        let (r1, g1, b1) = match h as u32 {
            0..=59 => (c, x, 0.0),
            60..=119 => (x, c, 0.0),
            120..=179 => (0.0, c, x),
            180..=239 => (0.0, x, c),
            240..=299 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Self {
            r: ((r1 + m) * 255.0).round() as u8,
            g: ((g1 + m) * 255.0).round() as u8,
            b: ((b1 + m) * 255.0).round() as u8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectKind {
    Static,
    PerKey,
    RainbowWave,
    SpiralRainbow,
    SpectrumCycle,
    Breathing,
    ColorWave,
    Visor,
    Rain,
    Gradient,
    Ambilight,
    ReactiveTyping,
    AudioVisualizer,
    Off,
}

impl EffectKind {
    pub const ALL: &'static [Self] = &[
        Self::Static,
        Self::PerKey,
        Self::RainbowWave,
        Self::SpiralRainbow,
        Self::SpectrumCycle,
        Self::Breathing,
        Self::ColorWave,
        Self::Visor,
        Self::Rain,
        Self::Gradient,
        Self::Ambilight,
        Self::ReactiveTyping,
        Self::AudioVisualizer,
        Self::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Statique",
            Self::PerKey => "Couleur par touche",
            Self::RainbowWave => "Vague Arc-en-ciel",
            Self::SpiralRainbow => "Spirale Arc-en-ciel",
            Self::SpectrumCycle => "Cycle de spectre",
            Self::Breathing => "Respiration",
            Self::ColorWave => "Vague 2 couleurs",
            Self::Visor => "Visière (K2000)",
            Self::Rain => "Pluie néon",
            Self::Gradient => "Dégradé 2 couleurs",
            Self::Ambilight => "Ambilight (Écran live)",
            Self::ReactiveTyping => "Frappe Réactive (Ripple)",
            Self::AudioVisualizer => "Visualiseur Audio (WASAPI)",
            Self::Off => "Éteint",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RgbEffectConfig {
    pub kind: EffectKind,
    pub primary: Rgb,
    pub secondary: Rgb,
    /// Speed multiplier (0.1 .. 5.0)
    pub speed: f32,
    /// Software brightness multiplier (0.0 .. 1.0)
    pub brightness: f32,
    /// Reverse wave direction
    pub reverse: bool,
    /// Per-key overrides keyed by LED index (stored with string keys: TOML has no integer keys)
    #[serde(default, with = "crate::serde_util::usize_key_map")]
    pub per_key: BTreeMap<usize, Rgb>,
}

impl Default for RgbEffectConfig {
    fn default() -> Self {
        Self {
            kind: EffectKind::RainbowWave,
            primary: Rgb::CYAN,
            secondary: Rgb::new(255, 0, 140),
            speed: 1.0,
            brightness: 1.0,
            reverse: false,
            per_key: BTreeMap::new(),
        }
    }
}

impl RgbEffectConfig {
    /// Compute the RGB color for a key at normalized position (x in 0..1, y in 0..1)
    /// and LED index `led_idx` at elapsed time `t_sec`.
    pub fn sample(&self, led_idx: usize, norm_x: f32, norm_y: f32, t_sec: f32) -> Rgb {
        let base = match self.kind {
            EffectKind::Off => Rgb::BLACK,
            EffectKind::Static => self.primary,
            EffectKind::PerKey => *self.per_key.get(&led_idx).unwrap_or(&self.primary),
            EffectKind::RainbowWave => {
                let dir = if self.reverse { -1.0 } else { 1.0 };
                let hue = (norm_x * 360.0 - dir * t_sec * 120.0 * self.speed).rem_euclid(360.0);
                Rgb::from_hsv(hue, 1.0, 1.0)
            }
            EffectKind::SpiralRainbow => {
                let dir = if self.reverse { -1.0 } else { 1.0 };
                let dx = norm_x - 0.5;
                let dy = (norm_y - 0.5) * 0.45;
                let angle = dy.atan2(dx).to_degrees();
                let dist = (dx * dx + dy * dy).sqrt();
                let hue =
                    (angle + dist * 240.0 - dir * t_sec * 140.0 * self.speed).rem_euclid(360.0);
                Rgb::from_hsv(hue, 1.0, 1.0)
            }
            EffectKind::SpectrumCycle => {
                let hue = (t_sec * 45.0 * self.speed).rem_euclid(360.0);
                Rgb::from_hsv(hue, 1.0, 1.0)
            }
            EffectKind::Breathing => {
                let phase = (t_sec * 2.0 * self.speed).sin() * 0.5 + 0.5;
                self.primary
                    .lerp(self.secondary, phase)
                    .scale(0.15 + 0.85 * phase)
            }
            EffectKind::ColorWave => {
                let dir = if self.reverse { -1.0 } else { 1.0 };
                let wave =
                    ((norm_x * std::f32::consts::TAU) - dir * t_sec * 3.0 * self.speed).sin() * 0.5
                        + 0.5;
                self.primary.lerp(self.secondary, wave)
            }
            EffectKind::Visor => {
                let scan = (t_sec * 1.8 * self.speed).sin() * 0.5 + 0.5;
                let dist = (norm_x - scan).abs();
                let glow = (1.0 - dist * 5.0).clamp(0.0, 1.0);
                self.secondary.scale(0.15).lerp(self.primary, glow * glow)
            }
            EffectKind::Rain => {
                // Deterministic pseudo-random vertical drops per column
                let col_seed = ((norm_x * 23.0).floor() * 37.7 + 11.3).sin().abs();
                let drop_y = (t_sec * 0.85 * self.speed + col_seed * 7.0).fract();
                let dy = (drop_y - norm_y).rem_euclid(1.0);
                let tail = (1.0 - dy * 3.2).clamp(0.0, 1.0);
                self.secondary.scale(0.10).lerp(self.primary, tail * tail)
            }
            EffectKind::Gradient => {
                let dir = if self.reverse { 1.0 - norm_x } else { norm_x };
                self.primary.lerp(self.secondary, dir)
            }
            EffectKind::Ambilight | EffectKind::ReactiveTyping | EffectKind::AudioVisualizer => {
                self.primary.scale(0.25)
            }
        };

        base.scale(self.brightness)
    }
}

#[cfg(test)]
mod tests {
    use super::Rgb;

    #[test]
    fn from_hex_parses_valid_colors() {
        assert_eq!(Rgb::from_hex("#00D2FF"), Some(Rgb::new(0, 0xD2, 0xFF)));
        assert_eq!(Rgb::from_hex(" ff1848 "), Some(Rgb::new(0xFF, 0x18, 0x48)));
    }

    #[test]
    fn from_hex_rejects_invalid_and_non_ascii_without_panicking() {
        assert_eq!(Rgb::from_hex("#12345"), None);
        assert_eq!(Rgb::from_hex("zzzzzz"), None);
        // 6 bytes but 5 chars: used to panic on a char boundary
        assert_eq!(Rgb::from_hex("aé123"), None);
        assert_eq!(Rgb::from_hex("éééé"), None);
    }

    #[test]
    fn hex_round_trip() {
        let c = Rgb::new(12, 200, 7);
        assert_eq!(Rgb::from_hex(&c.to_hex()), Some(c));
    }
}
