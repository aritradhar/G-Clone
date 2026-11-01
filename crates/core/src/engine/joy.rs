//! Wheel input thread: publishes live axes/buttons for the UI, turns wheel button presses
//! into binding triggers, and computes live rev-light masks.

use super::{hook, Cmd, Shared};
use crate::config::{RevLights, Trigger};
use crate::devices::joystick::{AxisInfo, JoyState, Joystick};
use crate::devices::wheel::rev_mask;
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct JoyLive {
    pub axes: Vec<AxisInfo>,
    pub button_count: u16,
    pub state: JoyState,
}

pub(super) fn spawn(mut joy: Joystick, shared: Arc<Shared>, alive: Arc<AtomicBool>, mgr: Sender<Cmd>) {
    alive.store(true, Ordering::SeqCst);
    let flag = alive.clone();
    let started = std::thread::Builder::new().name("gclone-wheel".into()).spawn(move || {
        let initial = joy.snapshot().unwrap_or_default();
        *shared.joy.lock().unwrap() =
            Some(JoyLive { axes: joy.axes.clone(), button_count: joy.button_count, state: initial });
        // `None` until the first report, so buttons held at startup don't fire.
        let mut prev: Option<Vec<u16>> = None;
        let mut last_mask: Option<u8> = None;
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

            // Only send a rev-light update when the number of lit lights changes.
            match live_rev_mask(&shared, &joy.axes, &state) {
                Some(mask) if last_mask != Some(mask) => {
                    last_mask = Some(mask);
                    let _ = mgr.send(Cmd::RevLights(mask));
                }
                Some(_) => {}
                None => last_mask = None,
            }

            if let Some(live) = shared.joy.lock().unwrap().as_mut() {
                live.state = state;
            }
            if hook::is_live() {
                (shared.repaint)();
            }
        }
        log::info!("wheel input closed");
        *shared.joy.lock().unwrap() = None;
        flag.store(false, Ordering::SeqCst);
    });
    if started.is_err() {
        alive.store(false, Ordering::SeqCst);
    }
}

/// Rev-light mask for the axis the profile wants shown, or `None` when no live mode is on.
fn live_rev_mask(shared: &Shared, axes: &[AxisInfo], state: &JoyState) -> Option<u8> {
    let src = *shared.rev.lock().unwrap();
    if !matches!(src.mode, RevLights::Accelerator | RevLights::Steering) {
        return None;
    }
    let i = axes.iter().position(|a| a.page == 0x01 && a.usage == src.usage)?;
    let a = &axes[i];
    let v = *state.values.get(i)?;
    let frac = (v - a.min) as f32 / (a.max - a.min).max(1) as f32;
    let fill = match src.mode {
        // Pedals: 0 = released. Steering: 0 = centre, 1 = full lock either way.
        RevLights::Accelerator => if src.invert { 1.0 - frac } else { frac },
        _ => (frac - 0.5).abs() * 2.0,
    };
    Some(rev_mask(fill))
}
