use crate::color::RgbEffectConfig;
use crate::device::PollingRate;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroStep {
    /// Key name (e.g. "A", "LCTRL", "SPACE", "F1")
    pub key: String,
    /// true = key down, false = key up
    pub down: bool,
    /// Delay after this step in milliseconds
    pub delay_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KeyAction {
    /// Simple 1-to-1 key remap (target key name, e.g. "F13", "PLAY_PAUSE", "SPACE")
    Remap { target: String },
    /// Disable the key completely
    Disable,
    /// Multi-step macro sequence
    Macro {
        name: String,
        steps: Vec<MacroStep>,
    },
    /// Send a text string
    Text { text: String, delay_ms: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocdPair {
    pub key_a: String,
    pub key_b: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocdConfig {
    /// Enable Last-Input-Priority SOCD (Snap Tap / FlashTap)
    #[serde(default)]
    pub enabled: bool,
    /// Opposing key pairs (e.g. Q+D and Z+S on AZERTY, or A+D and W+S on QWERTY)
    #[serde(default = "default_socd_pairs")]
    pub pairs: Vec<SocdPair>,
    /// Executables where SOCD is automatically suspended (e.g. cs2.exe to prevent Valve anti-SOCD kick!)
    #[serde(default = "default_socd_blacklist")]
    pub blocked_apps: Vec<String>,
}

fn default_socd_pairs() -> Vec<SocdPair> {
    vec![
        SocdPair {
            key_a: "Q".into(),
            key_b: "D".into(),
        },
        SocdPair {
            key_a: "Z".into(),
            key_b: "S".into(),
        },
    ]
}

fn default_socd_blacklist() -> Vec<String> {
    vec!["cs2.exe".into()]
}

impl Default for SocdConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pairs: default_socd_pairs(),
            blocked_apps: default_socd_blacklist(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardProfile {
    pub name: String,
    pub polling_rate: PollingRate,
    /// Hardware brightness percentage (0..=100)
    pub hw_brightness_pct: u8,
    /// Whether to apply software RGB control on Corsair K70 TKL
    pub enable_rgb: bool,
    pub rgb: RgbEffectConfig,
    /// Disable Left/Right Windows key while input engine is active
    pub win_lock: bool,
    /// Key remaps/macros keyed by canonical key name (e.g. "CAPS", "F1", "SCROLL")
    pub bindings: BTreeMap<String, KeyAction>,
    /// Synchronize ASUS Aura motherboard + ARGB headers with OpenGear RGB effect
    #[serde(default)]
    pub sync_asus_aura: bool,
    /// Custom LED counts per ASUS ARGB Gen2 header (index -> LED count, string keys in TOML)
    #[serde(default, with = "crate::serde_util::usize_key_map")]
    pub asus_argb_led_counts: BTreeMap<usize, usize>,
    /// Which ASUS Aura channel index feeds the be quiet! Silent Loop 3 waterblock ARGB (None = auto/all)
    #[serde(default)]
    pub silent_loop3_argb_channel: Option<usize>,
    /// Drive Silent Loop 3 waterblock ARGB color dynamically from GPU/CPU thermal load
    #[serde(default)]
    pub silent_loop3_thermal_rgb: bool,
    /// SOCD / Snap Tap (Last-Input Priority) esports optimization
    #[serde(default)]
    pub socd: SocdConfig,
    /// Software key chatter / anti-bounce filter in ms (0 = disabled, safe range 5..40 ms)
    #[serde(default)]
    pub key_chatter_ms: u16,
    /// Mouse left/right button anti-double-click filter in ms (0 = disabled, safe range 10..50 ms)
    #[serde(default)]
    pub mouse_debounce_ms: u16,
    /// Executables that trigger automatic switching to this profile (e.g. ["VALORANT-Win64-Shipping.exe"])
    #[serde(default)]
    pub linked_apps: Vec<String>,
}

impl Default for KeyboardProfile {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            polling_rate: PollingRate::Hz1000,
            hw_brightness_pct: 100,
            enable_rgb: false,
            rgb: RgbEffectConfig::default(),
            win_lock: false,
            bindings: BTreeMap::new(),
            sync_asus_aura: false,
            asus_argb_led_counts: BTreeMap::new(),
            silent_loop3_argb_channel: None,
            silent_loop3_thermal_rgb: false,
            socd: SocdConfig::default(),
            key_chatter_ms: 0,
            mouse_debounce_ms: 0,
            linked_apps: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_omen_zones() -> [String; 4] {
    [
        "#00D4FF".to_string(),
        "#00D4FF".to_string(),
        "#00D4FF".to_string(),
        "#00D4FF".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub active_profile: String,
    pub use_azerty_labels: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default)]
    pub start_minimized: bool,
    #[serde(default = "default_true")]
    pub auto_profile_switch: bool,
    /// Interface mode: "simple" (risky / advanced settings hidden) or "expert".
    #[serde(default = "default_ui_mode")]
    pub ui_mode: String,
    /// First-run assistant completed (or skipped).
    #[serde(default)]
    pub onboarding_done: bool,
    /// Turn every OpenGear-controlled light off while the Windows session is locked.
    #[serde(default = "default_true")]
    pub lights_off_on_lock: bool,
    /// Turn the lights off after this many minutes without keyboard/mouse input (0 = never).
    #[serde(default)]
    pub idle_lights_off_min: u16,
    /// Windows notification when a wireless device battery drops to 15 % and 5 %.
    #[serde(default = "default_true")]
    pub battery_alerts: bool,
    /// Synchronize HP OMEN 4-zone keyboard RGB with OpenGear's active lighting effect.
    #[serde(default)]
    pub omen_sync_rgb: bool,
    /// Saved 4-zone RGB colors for HP OMEN keyboards.
    #[serde(default = "default_omen_zones")]
    pub omen_zone_colors: [String; 4],
    /// Automatically drop to 60 Hz, Cool thermal mode, and keyboard backlight off on battery.
    #[serde(default)]
    pub laptop_auto_battery_saver: bool,
    pub profiles: Vec<KeyboardProfile>,
}

fn default_ui_mode() -> String {
    "simple".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            active_profile: "Default".to_string(),
            use_azerty_labels: true,
            minimize_to_tray: true,
            start_minimized: false,
            auto_profile_switch: true,
            ui_mode: default_ui_mode(),
            onboarding_done: false,
            lights_off_on_lock: true,
            idle_lights_off_min: 0,
            battery_alerts: true,
            omen_sync_rgb: false,
            omen_zone_colors: default_omen_zones(),
            laptop_auto_battery_saver: false,
            profiles: vec![KeyboardProfile::default()],
        }
    }
}

const CONFIG_FILE_NAME: &str = "opengear_profiles.toml";

impl AppConfig {
    /// Per-user configuration file: `%APPDATA%\OpenGear\opengear_profiles.toml`.
    ///
    /// It used to live next to the executable, which made it disappear with `--uninstall`
    /// (the install folder is deleted) and unwritable from `Program Files`. A legacy file found
    /// next to the exe is copied over on first use (the original is left untouched).
    pub fn default_config_path() -> PathBuf {
        let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) else {
            return Self::legacy_config_path();
        };
        let path = PathBuf::from(appdata).join("OpenGear").join(CONFIG_FILE_NAME);
        if !path.exists() {
            let legacy = Self::legacy_config_path();
            if legacy.exists() {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Err(e) = fs::copy(&legacy, &path) {
                    tracing::warn!(
                        "Migration de {} vers {} impossible : {e}",
                        legacy.display(),
                        path.display()
                    );
                    return legacy;
                }
                tracing::info!("Profils migrés de {} vers {}", legacy.display(), path.display());
            }
        }
        path
    }

    /// Historical location: next to the executable.
    pub fn legacy_config_path() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join(CONFIG_FILE_NAME);
            }
        }
        PathBuf::from(CONFIG_FILE_NAME)
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Impossible de lire {}", path.display()))?;
        let cfg: Self = toml::from_str(&raw)
            .with_context(|| format!("Erreur de parsing TOML dans {}", path.display()))?;
        Ok(cfg)
    }

    /// Load the configuration; if the file exists but cannot be read/parsed, move it aside
    /// (`<name>.corrupt-<unix_ts>`) before falling back to defaults, so the next save can never
    /// silently overwrite the user's profiles. Returns a warning message when that happened.
    pub fn load_or_recover(path: &Path) -> (Self, Option<String>) {
        match Self::load(path) {
            Ok(cfg) => (cfg, None),
            Err(err) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let mut backup = path.as_os_str().to_owned();
                backup.push(format!(".corrupt-{ts}"));
                let backup = PathBuf::from(backup);
                let msg = match fs::rename(path, &backup) {
                    Ok(()) => format!(
                        "Configuration illisible ({err:#}) — fichier mis de côté dans {} et profils par défaut chargés",
                        backup.display()
                    ),
                    Err(e) => format!(
                        "Configuration illisible ({err:#}) et impossible à mettre de côté ({e}) — profils par défaut chargés"
                    ),
                };
                tracing::error!("{msg}");
                (Self::default(), Some(msg))
            }
        }
    }

    /// Atomically save the configuration: serialize first, write to a temporary file in the same
    /// directory, flush it to disk, then rename over the target. A crash or serialization error can
    /// no longer leave a truncated / corrupt profile file behind.
    pub fn save(&self, path: &Path) -> Result<()> {
        use std::io::Write;

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let content = toml::to_string_pretty(self).context("Sérialisation TOML des profils impossible")?;

        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("Impossible de créer {}", tmp.display()))?;
            f.write_all(content.as_bytes())
                .with_context(|| format!("Impossible d'écrire {}", tmp.display()))?;
            f.sync_all()
                .with_context(|| format!("Impossible de synchroniser {}", tmp.display()))?;
        }
        // std::fs::rename replaces an existing destination on Windows (MoveFileExW + REPLACE_EXISTING).
        fs::rename(&tmp, path).with_context(|| {
            let _ = fs::remove_file(&tmp);
            format!("Impossible de remplacer {}", path.display())
        })?;
        Ok(())
    }

    pub fn active_profile_mut(&mut self) -> &mut KeyboardProfile {
        let idx = self
            .profiles
            .iter()
            .position(|p| p.name == self.active_profile)
            .unwrap_or(0);
        if self.profiles.is_empty() {
            self.profiles.push(KeyboardProfile::default());
        }
        &mut self.profiles[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{EffectKind, Rgb};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "opengear-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_and_load_with_per_key_colors_and_led_counts() {
        let dir = temp_dir("roundtrip");
        let path = dir.join(CONFIG_FILE_NAME);
        let mut cfg = AppConfig::default();
        {
            let p = cfg.active_profile_mut();
            p.rgb.kind = EffectKind::PerKey;
            p.rgb.per_key.insert(42, Rgb::new(1, 2, 3));
            p.asus_argb_led_counts.insert(1, 60);
        }
        cfg.save(&path).expect("config with per-key colors must save");
        assert!(!dir.join(format!("{CONFIG_FILE_NAME}.tmp")).exists(), "temp file left behind");

        let back = AppConfig::load(&path).unwrap();
        let p = &back.profiles[0];
        assert_eq!(p.rgb.per_key.get(&42), Some(&Rgb::new(1, 2, 3)));
        assert_eq!(p.asus_argb_led_counts.get(&1), Some(&60));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_overwrites_existing_file_atomically() {
        let dir = temp_dir("overwrite");
        let path = dir.join(CONFIG_FILE_NAME);
        let mut cfg = AppConfig::default();
        cfg.save(&path).unwrap();
        cfg.use_azerty_labels = false;
        cfg.save(&path).unwrap();
        assert!(!AppConfig::load(&path).unwrap().use_azerty_labels);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_moved_aside_not_overwritten() {
        let dir = temp_dir("corrupt");
        let path = dir.join(CONFIG_FILE_NAME);
        fs::write(&path, "this is = not [valid toml").unwrap();
        let (cfg, warning) = AppConfig::load_or_recover(&path);
        assert!(warning.is_some());
        assert_eq!(cfg.active_profile, "Default");
        assert!(!path.exists(), "corrupt file must be moved away");
        let backups: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read_to_string(backups[0].path()).unwrap(),
            "this is = not [valid toml"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_file_loads_defaults_without_warning() {
        let dir = temp_dir("missing");
        let (cfg, warning) = AppConfig::load_or_recover(&dir.join(CONFIG_FILE_NAME));
        assert!(warning.is_none());
        assert_eq!(cfg.profiles.len(), 1);
        let _ = fs::remove_dir_all(dir);
    }
}
