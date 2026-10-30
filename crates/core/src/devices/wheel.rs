//! G920 / G923 racing wheel via the ForceFeedback feature (0x8123).
//! Function numbers follow the Linux hid-logitech-hidpp driver.

use crate::hidpp::{features as f, Device, Result};

pub const RANGE_MIN: u16 = 40;
pub const RANGE_MAX: u16 = 900;

const FN_GET_INFO: u8 = 0x0;
const FN_RESET_ALL: u8 = 0x1;
const FN_DOWNLOAD_EFFECT: u8 = 0x2;
const FN_DESTROY_EFFECT: u8 = 0x4;
const FN_GET_APERTURE: u8 = 0x5;
const FN_SET_APERTURE: u8 = 0x6;
const FN_GET_GAINS: u8 = 0x7;
const FN_SET_GAINS: u8 = 0x8;

const EFFECT_SPRING: u8 = 0x06;
const EFFECT_AUTOSTART: u8 = 0x80;

pub struct Wheel {
    pub dev: Device,
    /// Slot of our centering-spring effect, if one is loaded.
    spring_slot: Option<u8>,
}

impl Wheel {
    pub fn new(dev: Device) -> Self {
        Self { dev, spring_slot: None }
    }

    /// Number of effect slots the wheel offers.
    pub fn effect_slots(&mut self) -> Result<u8> {
        Ok(self.dev.call(f::FORCE_FEEDBACK, FN_GET_INFO, &[])?.params()[0])
    }

    /// Operating range (lock to lock) in degrees.
    pub fn range(&mut self) -> Result<u16> {
        let r = self.dev.call(f::FORCE_FEEDBACK, FN_GET_APERTURE, &[])?;
        Ok(u16::from_be_bytes([r.params()[0], r.params()[1]]))
    }

    pub fn set_range(&mut self, degrees: u16) -> Result<()> {
        let v = degrees.clamp(RANGE_MIN, RANGE_MAX).to_be_bytes();
        self.dev.call(f::FORCE_FEEDBACK, FN_SET_APERTURE, &v)?;
        Ok(())
    }

    /// Global force-feedback strength, 0–100 %.
    pub fn strength(&mut self) -> Result<u8> {
        let r = self.dev.call(f::FORCE_FEEDBACK, FN_GET_GAINS, &[])?;
        let gain = u16::from_be_bytes([r.params()[0], r.params()[1]]) as u32;
        Ok(((gain * 100 + 0x7FFF) / 0xFFFF) as u8)
    }

    pub fn set_strength(&mut self, percent: u8) -> Result<()> {
        let gain = (percent.min(100) as u32 * 0xFFFF / 100) as u16;
        let [hi, lo] = gain.to_be_bytes();
        // Second pair is the boost gain; G Hub leaves it at 0.
        self.dev.call(f::FORCE_FEEDBACK, FN_SET_GAINS, &[hi, lo, 0, 0])?;
        Ok(())
    }

    /// Centering spring strength, 0 = off. Only felt while no game drives force feedback.
    pub fn set_centering(&mut self, percent: u8) -> Result<()> {
        if let Some(slot) = self.spring_slot.take() {
            self.dev.call(f::FORCE_FEEDBACK, FN_DESTROY_EFFECT, &[slot])?;
        }
        if percent == 0 {
            return Ok(());
        }
        let m = (percent.min(100) as u32 * 0xFFFF / 100) as u16;
        let mut p = [0u8; 18];
        p[1] = EFFECT_SPRING | EFFECT_AUTOSTART;
        // Coefficients and saturation for both directions; zero deadband and centre.
        p[6] = (m >> 9) as u8;
        p[7] = ((m >> 1) & 0xFF) as u8;
        p[8] = (m >> 11) as u8;
        p[9] = ((m >> 3) & 0xFF) as u8;
        p[14] = p[8];
        p[15] = p[9];
        p[16] = p[6];
        p[17] = p[7];
        let r = self.dev.call(f::FORCE_FEEDBACK, FN_DOWNLOAD_EFFECT, &p)?;
        self.spring_slot = Some(r.params()[0]);
        Ok(())
    }

    /// Stops and removes every effect on the wheel.
    pub fn reset_effects(&mut self) -> Result<()> {
        self.spring_slot = None;
        self.dev.call(f::FORCE_FEEDBACK, FN_RESET_ALL, &[])?;
        Ok(())
    }
}
