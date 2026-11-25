use crate::device::{CorsairDevice, CorsairDeviceInfo};
use crate::proto::BragiError;
use opengear_core::{OperatingMode, PollingRate, RgbEffectConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingEndpoint {
    /// Resource 0x0022: 2-byte header `[0x12, 0x00]` + interleaved RGB triplets.
    Triplets0x22,
    /// Resource 0x0001: planar `RRR...GGG...BBB...` channels.
    Planar0x01,
}

pub type CorsairKeyboardInfo = CorsairDeviceInfo;

/// Thin backward-compatible wrapper delegating to [`CorsairDevice`].
pub struct CorsairKeyboard {
    inner: CorsairDevice,
}

impl CorsairKeyboard {
    pub fn connect(target_pid: Option<u16>) -> Result<Self, BragiError> {
        let devices = CorsairDevice::connect_all();
        let chosen = devices
            .into_iter()
            .find(|d| target_pid.is_none_or(|p| d.info.pid == p))
            .ok_or(BragiError::DeviceNotFound)?;
        Ok(Self { inner: chosen })
    }

    pub fn info(&self) -> &CorsairKeyboardInfo {
        &self.inner.info
    }

    pub fn set_restore_hw_on_drop(&mut self, restore: bool) {
        self.inner.set_restore_hw_on_drop(restore);
    }

    pub fn set_mode(&mut self, mode: OperatingMode) -> Result<(), BragiError> {
        self.inner.set_mode(mode)
    }

    pub fn set_pollrate(&mut self, rate: PollingRate) -> Result<(), BragiError> {
        self.inner.set_pollrate(rate)
    }

    pub fn set_hw_brightness_pct(&mut self, pct: u8) -> Result<(), BragiError> {
        self.inner.set_hw_brightness_pct(pct)
    }

    pub fn hw_brightness_pct(&self) -> u8 {
        self.inner.hw_brightness_pct()
    }

    pub fn render_effect(
        &mut self,
        effect: &RgbEffectConfig,
        t_sec: f32,
    ) -> Result<(), BragiError> {
        self.inner.render_effect(effect, t_sec)
    }
}
