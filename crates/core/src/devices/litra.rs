//! Litra Glow via the Illumination feature (0x1990).

use crate::hidpp::{features as f, Device, Result};

pub const BRIGHTNESS_MIN: u16 = 20;
pub const BRIGHTNESS_MAX: u16 = 250;
pub const TEMP_MIN: u16 = 2700;
pub const TEMP_MAX: u16 = 6500;
pub const TEMP_STEP: u16 = 100;

/// Fallback when the feature table lookup fails; matches every Litra model seen so far.
const ILLUMINATION_INDEX: u8 = 0x04;

pub struct Litra {
    pub dev: Device,
    index: u8,
}

impl Litra {
    pub fn new(mut dev: Device) -> Self {
        let index = dev.feature_index(f::ILLUMINATION).unwrap_or(ILLUMINATION_INDEX);
        Self { dev, index }
    }

    pub fn is_on(&mut self) -> Result<bool> {
        Ok(self.dev.call_index(self.index, 0x0, &[])?.params()[0] == 1)
    }

    pub fn set_on(&mut self, on: bool) -> Result<()> {
        self.dev.call_index(self.index, 0x1, &[on as u8])?;
        Ok(())
    }

    /// Brightness in lumens.
    pub fn brightness(&mut self) -> Result<u16> {
        let r = self.dev.call_index(self.index, 0x3, &[])?;
        Ok(u16::from_be_bytes([r.params()[0], r.params()[1]]))
    }

    pub fn set_brightness(&mut self, lumens: u16) -> Result<()> {
        let v = lumens.clamp(BRIGHTNESS_MIN, BRIGHTNESS_MAX).to_be_bytes();
        self.dev.call_index(self.index, 0x4, &v)?;
        Ok(())
    }

    /// Colour temperature in kelvin.
    pub fn temperature(&mut self) -> Result<u16> {
        let r = self.dev.call_index(self.index, 0x8, &[])?;
        Ok(u16::from_be_bytes([r.params()[0], r.params()[1]]))
    }

    pub fn set_temperature(&mut self, kelvin: u16) -> Result<()> {
        let k = kelvin.clamp(TEMP_MIN, TEMP_MAX) / TEMP_STEP * TEMP_STEP;
        self.dev.call_index(self.index, 0x9, &k.to_be_bytes())?;
        Ok(())
    }
}
