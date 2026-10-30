//! Wheel input thread: publishes live axes/buttons for the UI and turns wheel button
//! presses into binding triggers.

use super::{hook, Shared};
use crate::config::Trigger;
use crate::devices::joystick::{AxisInfo, JoyState, Joystick};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct JoyLive {
    pub axes: Vec<AxisInfo>,
    pub button_count: u16,
    pub state: JoyState,
}

pub(super) fn spawn(mut joy: Joystick, shared: Arc<Shared>, alive: Arc<AtomicBool>) {
    alive.store(true, Ordering::SeqCst);
    let flag = alive.clone();
    let started = std::thread::Builder::new().name("gclone-wheel".into()).spawn(move || {
        let initial = joy.snapshot().unwrap_or_default();
        *shared.joy.lock().unwrap() =
            Some(JoyLive { axes: joy.axes.clone(), button_count: joy.button_count, state: initial });
        // `None` until the first report, so buttons held at startup don't fire.
        let mut prev: Option<Vec<u16>> = None;
        while let Some(read) = joy.read() {
            let Some(state) = read else { continue };
            if let Some(p) = &prev {
                for b in state.buttons.iter().filter(|b| !p.contains(b)) {
                    hook::handle(Trigger::Joy(*b), true);
                }
                for b in p.iter().filter(|b| !state.buttons.contains(b)) {
                    hook::handle(Trigger::Joy(*b), false);
                }
            }
            prev = Some(state.buttons.clone());
            if let Some(live) = shared.joy.lock().unwrap().as_mut() {
                live.state = state;
            }
            if hook::is_live() {
                (shared.repaint)();
            }
        }
        log::info!("wheel input closed");
        *shared.joy.lock().unwrap() = None;
        alive.store(false, Ordering::SeqCst);
    });
    if started.is_err() {
        flag.store(false, Ordering::SeqCst);
    }
}
