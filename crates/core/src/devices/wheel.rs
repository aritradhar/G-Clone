//! G920 / G923 racing wheel via the ForceFeedback feature (0x8123).
//! Function numbers follow the Linux hid-logitech-hidpp driver.

use crate::hidpp::{features as f, Device, Result};

pub const RANGE_MIN: u16 = 40;
pub const RANGE_MAX: u16 = 900;

/// Number of rev lights and the mask lighting all of them.
pub const REV_COUNT: u8 = 5;
pub const REV_ALL: u8 = 0x1F;

/// Mask lighting the first `fraction` (0..1) of the rev lights, like a rev counter.
pub fn rev_mask(fraction: f32) -> u8 {
    let lit = (fraction.clamp(0.0, 1.0) * REV_COUNT as f32).round() as u8;
    ((1u16 << lit) - 1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rev_mask_fills_like_a_rev_counter() {
        assert_eq!(rev_mask(0.0), 0);
        assert_eq!(rev_mask(0.2), 0b00001);
        assert_eq!(rev_mask(0.6), 0b00111);
        assert_eq!(rev_mask(1.0), REV_ALL);
        assert_eq!(rev_mask(-1.0), 0);
        assert_eq!(rev_mask(7.0), REV_ALL);
    }
}

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

/// Axes with an adjustable sensitivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Steering,
    Throttle,
    Brake,
    Clutch,
}

impl Axis {
    pub const ALL: [Axis; 4] = [Axis::Steering, Axis::Throttle, Axis::Brake, Axis::Clutch];

    /// HID Generic Desktop usage the wheel reports for this axis.
    fn usage(self) -> u16 {
        match self {
            Axis::Steering => 0x30,
            Axis::Throttle => 0x31,
            Axis::Brake => 0x32,
            Axis::Clutch => 0x35,
        }
    }
}

pub struct Wheel {
    pub dev: Device,
    /// Slot of our centering-spring effect, if one is loaded.
    spring_slot: Option<u8>,
    /// AxisSensitivity (0x80A3) entry index per [`Axis::ALL`], if the wheel has one.
    axis_index: [Option<u8>; 4],
}

impl Wheel {
    pub fn new(mut dev: Device) -> Self {
        let axis_index = Self::find_axes(&mut dev);
        Self { dev, spring_slot: None, axis_index }
    }

    /// 0x80A3 fn0 → entry count; fn1(i) → `00 01 <usage hi> <usage lo> <bits> ..`.
    fn find_axes(dev: &mut Device) -> [Option<u8>; 4] {
        let mut out = [None; 4];
        let Ok(count) = dev.call(f::AXIS_SENSITIVITY, 0x0, &[]).map(|r| r.params()[0]) else { return out };
        for i in 0..count {
            let Ok(r) = dev.call(f::AXIS_SENSITIVITY, 0x1, &[i]) else { continue };
            let usage = u16::from_be_bytes([r.params()[2], r.params()[3]]);
            if let Some(slot) = Axis::ALL.iter().position(|a| a.usage() == usage) {
                out[slot] = Some(i);
            }
        }
        out
    }

    fn axis(&self, axis: Axis) -> Result<u8> {
        let slot = Axis::ALL.iter().position(|a| *a == axis).unwrap();
        self.axis_index[slot].ok_or(crate::hidpp::Error::Unsupported(f::AXIS_SENSITIVITY))
    }

    /// Hands the rev lights to the host (`true`) or back to the wheel / games (`false`).
    /// 0x807A fn3: mode 2 = host control, 0 = wheel. Lights only accept masks in mode 2.
    pub fn set_rev_control(&mut self, host: bool) -> Result<()> {
        self.dev.call(f::REV_LIGHTS, 0x3, &[if host { 2 } else { 0 }])?;
        Ok(())
    }

    /// Turns the 5 rev lights on/off, one bit per light (0x1F = all). Needs host control.
    pub fn set_rev_lights(&mut self, mask: u8) -> Result<()> {
        self.dev.call(f::REV_LIGHTS, 0x6, &[0, mask & REV_ALL])?;
        Ok(())
    }

    /// Sensitivity 0–100 as G HUB shows it. The wheel stores pedals inverted (G HUB's
    /// "18" reads back as 82); steering is stored as-is.
    pub fn sensitivity(&mut self, axis: Axis) -> Result<u8> {
        let i = self.axis(axis)?;
        let raw = self.dev.call(f::AXIS_SENSITIVITY, 0x2, &[i])?.params()[1].min(100);
        Ok(if axis == Axis::Steering { raw } else { 100 - raw })
    }

    pub fn set_sensitivity(&mut self, axis: Axis, value: u8) -> Result<()> {
        let i = self.axis(axis)?;
        let v = value.min(100);
        let raw = if axis == Axis::Steering { v } else { 100 - v };
        // The first byte is always 0x64, as G HUB sends it; it is part of the stored pair.
        self.dev.call(f::AXIS_SENSITIVITY, 0x3, &[i, 0x64, raw])?;
        Ok(())
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
