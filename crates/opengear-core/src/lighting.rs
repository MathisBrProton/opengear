//! Lighting capability of a device, shared by every vendor.
//!
//! A device with LEDs describes them as **zones** of positioned LEDs (a key, a mouse logo, an ARGB
//! strip, a RAM stick…) and may offer **hardware effects** (run by the device itself, kept when
//! OpenGear is closed). Software effects are computed by OpenGear for every LED from its position
//! on a shared *desk canvas*, so one wave really crosses keyboard → mouse → case.
//!
//! The model follows the usual zone / mode structure of RGB hardware (single LED, linear strip,
//! matrix; modes with speed, direction, brightness and colour parameters), so native drivers and
//! bridges to other tools map onto it directly.

use crate::color::{Rgb, RgbEffectConfig};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// One LED, positioned in the device's own space (`0.0..=1.0` on both axes, origin top-left).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Led {
    pub name: String,
    pub x: f32,
    pub y: f32,
    /// Physical key (W3C `KeyboardEvent.code`) for keyboard LEDs, so per-key colours follow the
    /// key whatever the keyboard.
    pub key_code: Option<String>,
}

impl Led {
    pub fn at(name: impl Into<String>, x: f32, y: f32) -> Self {
        Self {
            name: name.into(),
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
            key_code: None,
        }
    }

    pub fn key(mut self, code: impl Into<String>) -> Self {
        self.key_code = Some(code.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    /// One colour for the whole zone (logo, single-colour backlight).
    Single,
    /// LEDs in a line (strip, fan ring, RAM stick).
    Linear,
    /// LEDs on a grid (keyboard, mouse pad).
    Matrix,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub kind: ZoneKind,
    pub leds: Vec<Led>,
    /// `(min, max)` LED count for zones whose length the user sets (ARGB headers).
    pub resizable: Option<(u16, u16)>,
}

impl Zone {
    /// A linear zone of `count` LEDs spread from left to right along `y`.
    pub fn linear(id: &str, name: &str, count: usize, y: f32) -> Self {
        let leds = (0..count)
            .map(|i| {
                let x = if count <= 1 {
                    0.5
                } else {
                    i as f32 / (count - 1) as f32
                };
                Led::at(format!("{name} {}", i + 1), x, y)
            })
            .collect();
        Self {
            id: id.into(),
            name: name.into(),
            kind: ZoneKind::Linear,
            leds,
            resizable: None,
        }
    }

    /// A single-colour zone centred on `(x, y)`.
    pub fn single(id: &str, name: &str, x: f32, y: f32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ZoneKind::Single,
            leds: vec![Led::at(name, x, y)],
            resizable: None,
        }
    }
}

/// Parameters a hardware effect accepts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EffectParams {
    /// `(min, max)` number of user colours (`(0, 0)` = the effect picks its own colours).
    pub colors: (u8, u8),
    /// `(min, max)` speed in the device's own units (higher = faster), if adjustable.
    pub speed: Option<(u8, u8)>,
    /// `(min, max)` brightness, if adjustable.
    pub brightness: Option<(u8, u8)>,
    /// Direction can be reversed.
    pub direction: bool,
    /// Can be stored in the device memory (survives power cycles / OpenGear closed).
    pub savable: bool,
}

/// An effect the device runs by itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HardwareEffect {
    pub id: String,
    pub label: String,
    pub params: EffectParams,
    /// Zones it can be applied to individually; empty = whole device only.
    pub zones: Vec<String>,
}

/// A hardware effect request, as chosen by the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectRequest {
    pub effect: String,
    /// `None` = whole device.
    pub zone: Option<String>,
    #[serde(default)]
    pub colors: Vec<Rgb>,
    pub speed: Option<u8>,
    pub brightness: Option<u8>,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub save: bool,
}

impl HardwareEffect {
    /// Check a request against this effect and clamp its values. Drivers only receive validated
    /// requests.
    pub fn validate(&self, req: &EffectRequest) -> Result<EffectRequest> {
        if req.effect != self.id {
            bail!("Effet « {} » attendu, « {} » reçu", self.id, req.effect);
        }
        if let Some(zone) = &req.zone
            && !self.zones.iter().any(|z| z == zone)
        {
            bail!(
                "L'effet « {} » ne s'applique pas à la zone « {zone} »",
                self.label
            );
        }
        let (min, max) = self.params.colors;
        let n = req.colors.len();
        if n < min as usize || n > max as usize {
            bail!(
                "L'effet « {} » attend {min} à {max} couleur(s), {n} reçue(s)",
                self.label
            );
        }
        if req.save && !self.params.savable {
            bail!(
                "L'effet « {} » ne peut pas être enregistré dans l'appareil",
                self.label
            );
        }
        let clamp = |v: Option<u8>, range: Option<(u8, u8)>| match (v, range) {
            (Some(v), Some((lo, hi))) => Some(v.clamp(lo, hi)),
            (None, Some((lo, hi))) => Some(lo + (hi - lo) / 2),
            _ => None,
        };
        Ok(EffectRequest {
            effect: self.id.clone(),
            zone: req.zone.clone(),
            colors: req.colors.clone(),
            speed: clamp(req.speed, self.params.speed),
            brightness: clamp(req.brightness, self.params.brightness),
            reverse: req.reverse && self.params.direction,
            save: req.save,
        })
    }
}

/// A device's lighting, driven either by software frames or by its own hardware effects.
pub trait Lighting {
    /// Zones in a stable order; the LEDs of all zones, in order, form a frame.
    fn zones(&self) -> Vec<Zone>;

    /// Hardware effects the device can run on its own.
    fn hardware_effects(&self) -> Vec<HardwareEffect> {
        Vec::new()
    }

    /// Show one colour per LED (all zones, in [`Lighting::zones`] order). Called up to
    /// [`Lighting::max_fps`] times per second while a software effect runs.
    fn set_frame(&mut self, colors: &[Rgb]) -> Result<()>;

    /// Run a hardware effect (request already validated).
    fn apply_hardware_effect(&mut self, req: &EffectRequest) -> Result<()> {
        bail!("Effet matériel « {} » non pris en charge", req.effect)
    }

    /// Give control back to the device's own lighting (OpenGear stops streaming frames).
    fn release(&mut self) -> Result<()> {
        Ok(())
    }

    /// Highest useful frame rate (slow buses such as SMBus need fewer frames).
    fn max_fps(&self) -> u8 {
        30
    }

    /// Resize a zone whose `resizable` range is `Some((min, max))`.
    fn resize_zone(&mut self, zone_id: &str, led_count: u16) -> Result<()> {
        let _ = (zone_id, led_count);
        bail!("Cette zone ne peut pas être redimensionnée")
    }
}

/// Optional per-device or per-zone lighting override stored in a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum LightingOverride {
    /// Follow the profile's global software effect.
    Global,
    /// Run a custom software effect on this device or zone.
    Software { effect: RgbEffectConfig },
    /// Run one of the device's built-in hardware effects.
    Hardware { request: EffectRequest },
    /// Turn this device or zone off.
    Off,
}

fn default_true() -> bool {
    true
}

/// Per-device lighting configuration stored inside a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceLightingConfig {
    /// Whether OpenGear's lighting engine manages this device (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional override for the whole device (`None` = follow profile global effect).
    #[serde(default)]
    pub mode: Option<LightingOverride>,
    /// Optional overrides per zone ID.
    #[serde(default)]
    pub zones: std::collections::BTreeMap<String, LightingOverride>,
    /// Custom LED counts for resizable zones (`zone.id` -> LED count).
    #[serde(default)]
    pub zone_led_counts: std::collections::BTreeMap<String, u16>,
}

impl Default for DeviceLightingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: None,
            zones: std::collections::BTreeMap::new(),
            zone_led_counts: std::collections::BTreeMap::new(),
        }
    }
}

/// Where a device sits on the shared desk canvas (`0.0..=1.0` on both axes).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for Placement {
    /// The whole canvas: every device sees the full effect.
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

impl Placement {
    /// Canvas position of a LED of a device placed here.
    pub fn map(&self, led: &Led) -> (f32, f32) {
        (
            (self.x + led.x * self.width).clamp(0.0, 1.0),
            (self.y + led.y * self.height).clamp(0.0, 1.0),
        )
    }
}

/// Compute one software frame for a device: each LED takes the effect colour at its canvas
/// position. `led_index` counts LEDs across zones (used by per-LED effects).
pub fn render_frame(
    zones: &[Zone],
    placement: &Placement,
    effect: &RgbEffectConfig,
    t_sec: f32,
) -> Vec<Rgb> {
    zones
        .iter()
        .flat_map(|z| z.leds.iter())
        .enumerate()
        .map(|(i, led)| {
            let (x, y) = placement.map(led);
            effect.sample_led(i, led.key_code.as_deref(), x, y, t_sec)
        })
        .collect()
}

/// Number of LEDs in a frame for these zones.
pub fn frame_len(zones: &[Zone]) -> usize {
    zones.iter().map(|z| z.leds.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EffectKind;

    fn wave() -> HardwareEffect {
        HardwareEffect {
            id: "wave".into(),
            label: "Vague".into(),
            params: EffectParams {
                colors: (0, 2),
                speed: Some((1, 5)),
                brightness: None,
                direction: true,
                savable: true,
            },
            zones: vec!["logo".into()],
        }
    }

    fn req() -> EffectRequest {
        EffectRequest {
            effect: "wave".into(),
            zone: None,
            colors: vec![Rgb::new(255, 0, 0)],
            speed: Some(9),
            brightness: Some(50),
            reverse: true,
            save: true,
        }
    }

    #[test]
    fn hardware_requests_are_validated_and_clamped() {
        let ok = wave().validate(&req()).unwrap();
        assert_eq!(ok.speed, Some(5));
        assert_eq!(
            ok.brightness, None,
            "brightness not adjustable on this effect"
        );
        assert!(ok.reverse && ok.save);

        let mut r = req();
        r.colors = vec![Rgb::BLACK; 3];
        assert!(wave().validate(&r).is_err(), "too many colours");
        r = req();
        r.zone = Some("keys".into());
        assert!(wave().validate(&r).is_err(), "zone not supported");
        r = req();
        r.effect = "static".into();
        assert!(wave().validate(&r).is_err());

        let mut fixed = wave();
        fixed.params.savable = false;
        fixed.params.direction = false;
        assert!(fixed.validate(&req()).is_err(), "cannot save");
        let mut r = req();
        r.save = false;
        r.speed = None;
        let ok = fixed.validate(&r).unwrap();
        assert!(!ok.reverse);
        assert_eq!(ok.speed, Some(3), "default speed = middle of the range");
    }

    #[test]
    fn frames_follow_led_positions_on_the_canvas() {
        let zones = vec![
            Zone::linear("strip", "Bande", 5, 0.5),
            Zone::single("logo", "Logo", 0.5, 0.5),
        ];
        assert_eq!(frame_len(&zones), 6);
        assert_eq!(zones[0].leds[0].x, 0.0);
        assert_eq!(zones[0].leds[4].x, 1.0);

        let mut effect = RgbEffectConfig {
            kind: EffectKind::Static,
            primary: Rgb::new(10, 20, 30),
            ..Default::default()
        };
        let frame = render_frame(&zones, &Placement::default(), &effect, 0.0);
        assert_eq!(frame.len(), 6);
        assert!(frame.iter().all(|c| *c == frame[0]));

        // A device on the right half of the desk sees the right half of a horizontal effect.
        effect.kind = EffectKind::RainbowWave;
        let right = Placement {
            x: 0.5,
            width: 0.5,
            ..Default::default()
        };
        let led = Led::at("a", 0.0, 0.0);
        assert_eq!(right.map(&led), (0.5, 0.0));
        assert_eq!(right.map(&Led::at("b", 1.0, 1.0)), (1.0, 1.0));
    }

    #[test]
    fn per_key_frames_match_by_key_code() {
        let zones = vec![Zone {
            id: "keys".into(),
            name: "Keys".into(),
            kind: ZoneKind::Matrix,
            leds: vec![
                Led::at("Esc", 0.0, 0.0).key("Escape"),
                Led::at("Q", 0.2, 0.3).key("KeyQ"),
                Led::at("Logo", 0.5, 0.0),
            ],
            resizable: None,
        }];
        let mut effect = RgbEffectConfig {
            kind: EffectKind::PerKey,
            primary: Rgb::new(10, 10, 10),
            brightness: 1.0,
            ..Default::default()
        };
        effect.per_key.insert("Escape".into(), Rgb::RED);
        effect.per_key.insert("KeyQ".into(), Rgb::CYAN);
        let frame = render_frame(&zones, &Placement::default(), &effect, 0.0);
        assert_eq!(frame, vec![Rgb::RED, Rgb::CYAN, Rgb::new(10, 10, 10)]);
    }
}
