//! Live game-controller input (wheel angle, pedals, buttons) from the wheel's joystick
//! HID collection, decoded with the Windows HID parser (`HidP_*`) so we don't hard-code
//! report layouts. This is read-only and shares the device with games.

use crate::LOGITECH_VID;
use hidapi::HidApi;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::ptr::null_mut;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
pub const PAGE_GENERIC_DESKTOP: u16 = 0x01;
pub const PAGE_BUTTON: u16 = 0x09;
pub const USAGE_HAT: u16 = 0x39;

/// One absolute input value (axis or hat) the device reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisInfo {
    pub page: u16,
    pub usage: u16,
    pub min: i32,
    pub max: i32,
}

impl AxisInfo {
    pub fn name(&self) -> String {
        match (self.page, self.usage) {
            (0x01, 0x30) => "X".into(),
            (0x01, 0x31) => "Y".into(),
            (0x01, 0x32) => "Z".into(),
            (0x01, 0x33) => "Rx".into(),
            (0x01, 0x34) => "Ry".into(),
            (0x01, 0x35) => "Rz".into(),
            (0x01, 0x36) => "Slider".into(),
            (0x01, 0x37) => "Dial".into(),
            (0x01, 0x39) => "Hat".into(),
            (p, u) => format!("{p:02X}:{u:02X}"),
        }
    }
}

/// Snapshot of the controller state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JoyState {
    /// Raw values, same order as [`Joystick::axes`].
    pub values: Vec<i32>,
    /// Pressed buttons, 1-based HID button numbers.
    pub buttons: Vec<u16>,
}

pub struct Joystick {
    handle: HANDLE,
    preparsed: PHIDP_PREPARSED_DATA,
    buf: Vec<u8>,
    /// Previous raw report; identical reports are not decoded again.
    last: Vec<u8>,
    usages: Vec<u16>,
    pub axes: Vec<AxisInfo>,
    pub button_count: u16,
}

// The handle and preparsed data are plain kernel/heap objects usable from any thread.
unsafe impl Send for Joystick {}

impl Drop for Joystick {
    fn drop(&mut self) {
        unsafe {
            HidD_FreePreparsedData(self.preparsed);
            CloseHandle(self.handle);
        }
    }
}

impl Joystick {
    /// Opens the joystick / gamepad collection of the given Logitech product.
    pub fn open(api: &HidApi, pid: u16) -> Option<Self> {
        let info = api.device_list().find(|d| {
            d.vendor_id() == LOGITECH_VID
                && d.product_id() == pid
                && d.usage_page() == PAGE_GENERIC_DESKTOP
                && matches!(d.usage(), 0x04 | 0x05)
        })?;
        let path: Vec<u16> = OsString::from(info.path().to_string_lossy().into_owned())
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe { Self::from_path(&path) }
    }

    unsafe fn from_path(path: &[u16]) -> Option<Self> {
        let handle = CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            log::warn!("joystick open failed: {}", OsString::from_wide(&path[..path.len() - 1]).to_string_lossy());
            return None;
        }
        let mut preparsed: PHIDP_PREPARSED_DATA = 0;
        if HidD_GetPreparsedData(handle, &mut preparsed) == 0 {
            CloseHandle(handle);
            return None;
        }
        let mut caps: HIDP_CAPS = std::mem::zeroed();
        if HidP_GetCaps(preparsed, &mut caps) != HIDP_STATUS_SUCCESS {
            HidD_FreePreparsedData(preparsed);
            CloseHandle(handle);
            return None;
        }

        let mut n = caps.NumberInputValueCaps;
        let mut vcaps: Vec<HIDP_VALUE_CAPS> = vec![std::mem::zeroed(); n as usize];
        HidP_GetValueCaps(HidP_Input, vcaps.as_mut_ptr(), &mut n, preparsed);
        let mut axes = Vec::new();
        for c in &vcaps[..n as usize] {
            let (lo, hi) = if c.IsRange != 0 {
                (c.Anonymous.Range.UsageMin, c.Anonymous.Range.UsageMax)
            } else {
                (c.Anonymous.NotRange.Usage, c.Anonymous.NotRange.Usage)
            };
            for usage in lo..=hi {
                axes.push(AxisInfo { page: c.UsagePage, usage, min: c.LogicalMin, max: c.LogicalMax });
            }
        }

        let mut nb = caps.NumberInputButtonCaps;
        let mut bcaps: Vec<HIDP_BUTTON_CAPS> = vec![std::mem::zeroed(); nb as usize];
        HidP_GetButtonCaps(HidP_Input, bcaps.as_mut_ptr(), &mut nb, preparsed);
        let button_count = bcaps[..nb as usize]
            .iter()
            .filter(|c| c.UsagePage == PAGE_BUTTON)
            .map(|c| if c.IsRange != 0 { c.Anonymous.Range.UsageMax } else { c.Anonymous.NotRange.Usage })
            .max()
            .unwrap_or(0);

        let len = caps.InputReportByteLength as usize;
        Some(Self {
            handle,
            preparsed,
            buf: vec![0; len],
            last: Vec::with_capacity(len),
            usages: vec![0; button_count as usize + 1],
            axes,
            button_count,
        })
    }

    /// Blocks until the next input report. `None` when the device is gone,
    /// `Some(None)` when the report is identical to the previous one.
    pub fn read(&mut self) -> Option<Option<JoyState>> {
        let mut got = 0u32;
        let ok = unsafe { ReadFile(self.handle, self.buf.as_mut_ptr(), self.buf.len() as u32, &mut got, null_mut()) };
        if ok == 0 || got == 0 {
            return None;
        }
        if self.buf == self.last {
            return Some(None);
        }
        self.last.clone_from(&self.buf);
        Some(Some(self.decode()))
    }

    fn decode(&mut self) -> JoyState {
        let report = &mut self.buf;
        let mut values = Vec::with_capacity(self.axes.len());
        for a in &self.axes {
            let mut v: u32 = 0;
            let st = unsafe {
                HidP_GetUsageValue(
                    HidP_Input,
                    a.page,
                    0,
                    a.usage,
                    &mut v,
                    self.preparsed,
                    report.as_ptr(),
                    report.len() as u32,
                )
            };
            values.push(if st == HIDP_STATUS_SUCCESS { v as i32 } else { a.min });
        }

        let usages = &mut self.usages;
        let mut len = usages.len() as u32;
        let st = unsafe {
            HidP_GetUsages(
                HidP_Input,
                PAGE_BUTTON,
                0,
                usages.as_mut_ptr(),
                &mut len,
                self.preparsed,
                report.as_mut_ptr(),
                report.len() as u32,
            )
        };
        let mut buttons = if st == HIDP_STATUS_SUCCESS { usages[..len as usize].to_vec() } else { Vec::new() };
        buttons.sort_unstable();
        JoyState { values, buttons }
    }
}
