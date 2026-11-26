//! Shared audio / equalizer capability for headsets and DACs.

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EqPresetInfo {
    pub id: String,
    pub name: String,
    pub gains_db: Vec<i8>,
}

/// Snapshot of an audio device's hardware equalizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EqInfo {
    pub band_count: usize,
    pub min_db: i8,
    pub max_db: i8,
    pub frequencies_hz: Vec<u16>,
    pub gains_db: Vec<i8>,
    pub presets: Vec<EqPresetInfo>,
}

/// Audio device exposing a hardware parametric/graphic equalizer.
pub trait Equalizer {
    fn eq_info(&self) -> Option<EqInfo>;

    fn set_eq_gains(&mut self, gains: &[i8]) -> Result<()>;
}
