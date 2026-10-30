//! Gaming mouse over HID++ 2.0 (tested: G602 behind receiver C537).

use crate::hidpp::{features as f, Device, Error, Result};

const ERR_INVALID_ARGUMENT: u8 = 0x02;

pub struct Mouse {
    pub dev: Device,
    /// Sensor number for AdjustableDpi; 0 on current mice, 1 on v0 devices like the G602.
    sensor: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charging {
    Discharging,
    Recharging,
    AlmostFull,
    Full,
    Other(u8),
}

#[derive(Debug, Clone, Copy)]
pub struct Battery {
    pub percent: u8,
    pub charging: Charging,
}

impl Mouse {
    pub fn new(mut dev: Device) -> Self {
        let sensor = match dev.call(f::ADJUSTABLE_DPI, 0x2, &[0]) {
            Err(Error::Hidpp20(ERR_INVALID_ARGUMENT)) => 1,
            _ => 0,
        };
        Self { dev, sensor }
    }

    /// All selectable DPI values, with step ranges expanded.
    pub fn dpi_values(&mut self) -> Result<Vec<u16>> {
        let r = self.dev.call(f::ADJUSTABLE_DPI, 0x1, &[self.sensor])?;
        // Current firmware echoes the sensor number first; the v0 (sensor 1) reply does not.
        let skip = if self.sensor == 0 { 1 } else { 0 };
        let raw: Vec<u16> = r.params()[skip..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .take_while(|v| *v != 0)
            .collect();

        // v0 devices (G602) answer `min, step, max` with no step marker: 250, 250, 2500.
        let has_marker = raw.iter().any(|v| v >> 13 == 0b111);
        if !has_marker && raw.len() == 3 && raw[1] <= raw[0] && raw[2] > raw[0] && raw[1] > 0 {
            return Ok((raw[0]..=raw[2]).step_by(raw[1] as usize).collect());
        }

        let mut out: Vec<u16> = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let v = raw[i];
            if v >> 13 == 0b111 {
                // "step" marker: previous value .. next value in increments of `step`.
                let step = v & 0x1FFF;
                if let (Some(&from), Some(&to)) = (out.last(), raw.get(i + 1)) {
                    let mut d = from + step;
                    while step > 0 && d <= to {
                        out.push(d);
                        d += step;
                    }
                    i += 2;
                    continue;
                }
            } else {
                out.push(v);
            }
            i += 1;
        }
        out.dedup();
        Ok(out)
    }

    pub fn dpi(&mut self) -> Result<u16> {
        let r = self.dev.call(f::ADJUSTABLE_DPI, 0x2, &[self.sensor])?;
        let p = r.params();
        Ok(u16::from_be_bytes([p[1], p[2]]))
    }

    pub fn set_dpi(&mut self, dpi: u16) -> Result<()> {
        let [hi, lo] = dpi.to_be_bytes();
        self.dev.call(f::ADJUSTABLE_DPI, 0x3, &[self.sensor, hi, lo])?;
        Ok(())
    }

    /// Supported report intervals in milliseconds (1 ms = 1000 Hz).
    pub fn report_rates(&mut self) -> Result<Vec<u8>> {
        let mask = self.dev.call(f::REPORT_RATE, 0x0, &[])?.params()[0];
        Ok((0..8).filter(|b| mask & (1 << b) != 0).map(|b| b + 1).collect())
    }

    pub fn report_rate(&mut self) -> Result<u8> {
        Ok(self.dev.call(f::REPORT_RATE, 0x1, &[])?.params()[0])
    }

    pub fn set_report_rate(&mut self, ms: u8) -> Result<()> {
        self.dev.call(f::REPORT_RATE, 0x2, &[ms])?;
        Ok(())
    }

    pub fn battery(&mut self) -> Result<Battery> {
        let r = self.dev.call(f::BATTERY_STATUS, 0x0, &[])?;
        let p = r.params();
        let charging = match p[2] {
            0 => Charging::Discharging,
            1 => Charging::Recharging,
            2 => Charging::AlmostFull,
            3 => Charging::Full,
            s => Charging::Other(s),
        };
        Ok(Battery { percent: p[0], charging })
    }
}
