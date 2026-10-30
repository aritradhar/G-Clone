//! Drivers for the supported devices and discovery of connected ones.

pub mod joystick;
pub mod litra;
pub mod mouse;
pub mod wheel;

use crate::hidpp::{Channel, Device, Result, DEVICE_DIRECT};
use crate::LOGITECH_VID;
use hidapi::{DeviceInfo, HidApi};

pub const PID_G602_RECEIVER: u16 = 0xC537;
pub const PID_LITRA_GLOW: u16 = 0xC900;
pub const PID_G923_XBOX: u16 = 0xC26E;
pub const PID_G923_PS: u16 = 0xC266;
pub const PID_G920: u16 = 0xC262;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Mouse,
    Light,
    Wheel,
}

impl Kind {
    pub fn for_pid(pid: u16) -> Option<Self> {
        match pid {
            PID_G602_RECEIVER => Some(Kind::Mouse),
            PID_LITRA_GLOW => Some(Kind::Light),
            PID_G923_XBOX | PID_G923_PS | PID_G920 => Some(Kind::Wheel),
            _ => None,
        }
    }

    /// HID++ device index used to reach the device on its channel.
    fn device_index(self) -> u8 {
        match self {
            Kind::Mouse => 1,
            _ => DEVICE_DIRECT,
        }
    }
}

/// Opens the HID++ device for every supported, plugged-in product whose kind passes `want`.
pub fn discover(api: &HidApi, want: impl Fn(Kind) -> bool) -> Vec<(Kind, u16, Device)> {
    let mut pids: Vec<u16> = api
        .device_list()
        .filter(|d| d.vendor_id() == LOGITECH_VID)
        .map(DeviceInfo::product_id)
        .filter(|pid| Kind::for_pid(*pid).is_some_and(&want))
        .collect();
    pids.sort_unstable();
    pids.dedup();

    let mut out = Vec::new();
    for pid in pids {
        let kind = Kind::for_pid(pid).unwrap();
        match open(api, pid, kind) {
            Ok(dev) => out.push((kind, pid, dev)),
            Err(e) => log::warn!("cannot open 046D:{pid:04X}: {e}"),
        }
    }
    out
}

fn open(api: &HidApi, pid: u16, kind: Kind) -> Result<Device> {
    let infos: Vec<&DeviceInfo> = api
        .device_list()
        .filter(|d| d.vendor_id() == LOGITECH_VID && d.product_id() == pid)
        .collect();
    let mut dev = Device::new(Channel::open(api, &infos)?, kind.device_index());
    dev.protocol_version()?;
    Ok(dev)
}
