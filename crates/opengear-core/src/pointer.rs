//! Shared pointer (mouse) capability for DPI, stages, LOD and polling rate.

use anyhow::Result;
use serde::Serialize;

/// Snapshot of a pointer device's DPI, LOD and report-rate capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PointerDpiInfo {
    pub dpi_x: u16,
    pub dpi_y: u16,
    pub min_dpi: u16,
    pub max_dpi: u16,
    pub step_dpi: u16,
    pub independent_xy: bool,
    pub supports_lod: bool,
    pub lod: Option<String>,
    pub stages: Vec<u16>,
    pub active_stage: Option<usize>,
    pub report_rate_hz: Option<u16>,
    pub supported_rates_hz: Vec<u16>,
}

/// Mice and pointing devices exposing DPI, lift-off distance and/or stage controls.
pub trait PointerDevice {
    fn pointer_info(&self) -> Option<PointerDpiInfo>;

    fn set_dpi(&mut self, x: u16, y: Option<u16>) -> Result<()>;

    fn set_dpi_with_lod(&mut self, x: u16, y: Option<u16>, lod: Option<&str>) -> Result<()> {
        let _ = lod;
        self.set_dpi(x, y)
    }

    fn set_dpi_stage(&mut self, stage: usize, dpi: u16) -> Result<()> {
        let _ = (stage, dpi);
        anyhow::bail!("Paliers DPI non modifiables sur cet appareil")
    }

    fn set_active_dpi_stage(&mut self, stage: usize) -> Result<()> {
        let _ = stage;
        anyhow::bail!("Changement de palier DPI non pris en charge")
    }
}
