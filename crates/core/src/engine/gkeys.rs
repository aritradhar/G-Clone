//! Extra mouse buttons that bypass Windows input.
//!
//! The G602 reports G6–G11 only on its receiver's vendor collection (usage FF80:0080) as
//! `80 <mask lo> <mask hi> ..`; the OS mouse driver never sees them. This thread reads
//! that collection in shared mode and feeds bit changes into the binding engine.

use super::hook;
use crate::config::Trigger;
use crate::LOGITECH_VID;
use hidapi::HidApi;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const USAGE_PAGE: u16 = 0xFF80;
const USAGE: u16 = 0x0080;
const REPORT_ID: u8 = 0x80;

pub(super) fn spawn(api: &HidApi, pid: u16, alive: Arc<AtomicBool>) {
    let Some(info) = api.device_list().find(|d| {
        d.vendor_id() == LOGITECH_VID && d.product_id() == pid && d.usage_page() == USAGE_PAGE && d.usage() == USAGE
    }) else {
        return;
    };
    let dev = match info.open_device(api) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("G-key channel: {e}");
            return;
        }
    };
    alive.store(true, Ordering::SeqCst);
    let flag = alive.clone();
    let started = std::thread::Builder::new().name("gclone-gkeys".into()).spawn(move || {
        let mut prev: u16 = 0;
        let mut buf = [0u8; 16];
        loop {
            // Blocks until the next report; the thread sleeps while no G-key is pressed.
            match dev.read(&mut buf) {
                Ok(n) if n >= 3 && buf[0] == REPORT_ID => {
                    let mask = u16::from_le_bytes([buf[1], buf[2]]);
                    let changed = mask ^ prev;
                    for bit in (0..16u8).filter(|b| changed & (1 << b) != 0) {
                        hook::handle(Trigger::GKey(bit), mask & (1 << bit) != 0);
                    }
                    prev = mask;
                }
                Ok(_) => {}
                Err(e) => {
                    log::info!("G-key channel closed: {e}");
                    break;
                }
            }
        }
        // Release anything still held so no action stays stuck.
        for bit in (0..16u8).filter(|b| prev & (1 << b) != 0) {
            hook::handle(Trigger::GKey(bit), false);
        }
        flag.store(false, Ordering::SeqCst);
    });
    if started.is_err() {
        alive.store(false, Ordering::SeqCst);
    }
}
