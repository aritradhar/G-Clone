//! Device control core for the G Hub clone: HID++ protocol, device drivers,
//! profiles and the input engine. The UI crate only talks to this crate.

pub mod config;
pub mod devices;
pub mod engine;
pub mod hidpp;
pub mod keys;

pub const LOGITECH_VID: u16 = 0x046D;
