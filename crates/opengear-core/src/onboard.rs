//! Shared onboard-memory capability for mice and keyboards with hardware profile slots.

use anyhow::Result;
use serde::Serialize;

/// Summary of one hardware profile slot stored inside a device's flash memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OnboardProfileSummary {
    pub slot: u16,
    pub name: String,
    pub active: bool,
    pub dpi_stages: Vec<u16>,
    pub report_rate_hz: Option<u16>,
    pub editable: bool,
}

/// Device exposing onboard hardware profile slots.
pub trait OnboardMemory {
    fn onboard_profiles(&self) -> Vec<OnboardProfileSummary>;

    fn set_active_onboard_profile(&mut self, slot: u16) -> Result<()>;

    fn reload_onboard_profiles(&mut self) -> Result<()> {
        Ok(())
    }
}
