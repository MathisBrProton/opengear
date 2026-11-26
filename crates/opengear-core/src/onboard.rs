//! Shared onboard-memory capability for mice and keyboards with hardware profile slots,
//! flash macro storage, and pre-write flash backups.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Current binding of one programmable hardware button stored in an onboard profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardButtonBinding {
    pub hex: String,
    pub label: String,
}

/// Global onboard-memory summary of a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardMemoryInfo {
    /// `"onboard"` or `"host"` when the device supports switching modes.
    pub mode: Option<String>,
    pub mode_label: Option<String>,
    pub profile_count: u8,
    pub button_count: u8,
    pub active_profile: u16,
    pub active_dpi_index: u8,
    pub writable: bool,
    pub macros_supported: bool,
}

/// Summary of one hardware profile slot stored inside a device's flash/ROM memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardProfileSummary {
    pub slot: u16,
    pub name: String,
    pub active: bool,
    pub dpi_stages: Vec<u16>,
    pub default_dpi_idx: Option<u8>,
    pub report_rate_hz: Option<u16>,
    pub editable: bool,
    pub sector_addr: Option<String>,
    pub from_rom: bool,
    pub crc_valid: Option<bool>,
    pub buttons: Vec<OnboardButtonBinding>,
}

/// Editable fields written to an onboard profile slot.
#[derive(Debug, Clone, Default)]
pub struct OnboardProfilePatch {
    pub report_rate_hz: Option<u16>,
    pub default_dpi_idx: Option<u8>,
    pub dpi_stages: Option<Vec<u16>>,
    pub button_hexes: Option<Vec<String>>,
}

/// Portable onboard hardware macro step (USB HID usages & 1-based mouse buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnboardMacroStep {
    KeyPress(u8),
    KeyRelease(u8),
    MousePress(u8),
    MouseRelease(u8),
    DelayMs(u16),
}

/// Metadata for a pre-write flash backup stored on disk by a driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardBackupEntry {
    pub file: String,
    pub tag: String,
    pub sector_hex: String,
    pub size: u64,
    pub created_ms: u64,
}

/// Device exposing onboard hardware profile slots and/or flash backups.
pub trait OnboardMemory {
    fn onboard_info(&self) -> Option<OnboardMemoryInfo> {
        None
    }

    fn onboard_profiles(&self) -> Vec<OnboardProfileSummary>;

    fn button_presets(&self) -> Vec<OnboardButtonBinding> {
        Vec::new()
    }

    fn set_active_onboard_profile(&mut self, slot: u16) -> Result<()>;

    fn reload_onboard_profiles(&mut self) -> Result<()> {
        Ok(())
    }

    fn write_onboard_profile(
        &mut self,
        slot_index: usize,
        patch: &OnboardProfilePatch,
    ) -> Result<()> {
        let _ = (slot_index, patch);
        anyhow::bail!("Écriture de profil embarqué non supportée sur cet appareil")
    }

    fn write_onboard_macro(
        &mut self,
        profile_index: usize,
        button_index: usize,
        steps: &[OnboardMacroStep],
    ) -> Result<()> {
        let _ = (profile_index, button_index, steps);
        anyhow::bail!("Écriture de macro embarquée non supportée sur cet appareil")
    }

    fn onboard_backups(&self) -> Vec<OnboardBackupEntry> {
        Vec::new()
    }

    fn restore_onboard_backup(&mut self, file: &str) -> Result<String> {
        let _ = file;
        anyhow::bail!("Restauration de sauvegarde flash non supportée sur cet appareil")
    }
}
