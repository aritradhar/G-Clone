//! Macro player thread.

use super::input;
use crate::config::{Macro, MacroMode, MouseButton, Step};
use crossbeam_channel::{unbounded, Sender};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub enum MacroCmd {
    Set(Vec<Macro>),
    Press(String),
    Release(String),
}

pub fn spawn() -> Sender<MacroCmd> {
    let (tx, rx) = unbounded::<MacroCmd>();
    thread::Builder::new()
        .name("gclone-macros".into())
        .spawn(move || {
            let mut macros: HashMap<String, Arc<Macro>> = HashMap::new();
            // `true` while a macro's playback thread should keep going.
            let mut running: HashMap<String, Arc<AtomicBool>> = HashMap::new();
            for cmd in rx {
                match cmd {
                    MacroCmd::Set(list) => {
                        macros = list.into_iter().map(|m| (m.name.clone(), Arc::new(m))).collect();
                    }
                    MacroCmd::Press(name) => {
                        let Some(m) = macros.get(&name).cloned() else { continue };
                        let flag = running.entry(name).or_insert_with(|| Arc::new(AtomicBool::new(false))).clone();
                        match m.mode {
                            MacroMode::Once => {
                                if !flag.swap(true, Ordering::SeqCst) {
                                    thread::spawn(move || {
                                        play(&m, &flag, false);
                                        flag.store(false, Ordering::SeqCst);
                                    });
                                }
                            }
                            MacroMode::WhileHeld => {
                                if !flag.swap(true, Ordering::SeqCst) {
                                    thread::spawn(move || play(&m, &flag, true));
                                }
                            }
                            MacroMode::Toggle => {
                                if !flag.swap(false, Ordering::SeqCst) {
                                    flag.store(true, Ordering::SeqCst);
                                    thread::spawn(move || play(&m, &flag, true));
                                }
                            }
                        }
                    }
                    MacroCmd::Release(name) => {
                        let held = macros.get(&name).is_some_and(|m| m.mode == MacroMode::WhileHeld);
                        if let (true, Some(flag)) = (held, running.get(&name)) {
                            flag.store(false, Ordering::SeqCst);
                        }
                    }
                }
            }
        })
        .expect("spawn macro thread");
    tx
}

/// Plays `m` once, or repeatedly while `flag` stays set when `repeat` is true.
fn play(m: &Macro, flag: &AtomicBool, repeat: bool) {
    let mut keys: Vec<u16> = Vec::new();
    let mut buttons: Vec<MouseButton> = Vec::new();
    let has_delay = m.steps.iter().any(|s| matches!(s, Step::Delay(_)));
    'outer: loop {
        for step in &m.steps {
            if repeat && !flag.load(Ordering::SeqCst) {
                break 'outer;
            }
            match step {
                Step::KeyDown(vk) => {
                    input::key(*vk, true);
                    keys.push(*vk);
                }
                Step::KeyUp(vk) => {
                    input::key(*vk, false);
                    keys.retain(|k| k != vk);
                }
                Step::KeyTap(vk) => {
                    input::key(*vk, true);
                    input::key(*vk, false);
                }
                Step::MouseDown(b) => {
                    input::mouse(*b, true);
                    buttons.push(*b);
                }
                Step::MouseUp(b) => {
                    input::mouse(*b, false);
                    buttons.retain(|x| x != b);
                }
                Step::Click(b) => {
                    input::mouse(*b, true);
                    input::mouse(*b, false);
                }
                Step::Delay(ms) => sleep(*ms, flag, repeat),
                Step::Text(s) => input::text(s),
            }
        }
        if !repeat || !flag.load(Ordering::SeqCst) {
            break;
        }
        if !has_delay {
            // A repeating macro without delays would flood the input queue.
            thread::sleep(Duration::from_millis(10));
        }
    }
    // Never leave keys stuck down when a loop is cut short.
    for vk in keys {
        input::key(vk, false);
    }
    for b in buttons {
        input::mouse(b, false);
    }
}

fn sleep(ms: u32, flag: &AtomicBool, interruptible: bool) {
    let end = Instant::now() + Duration::from_millis(ms as u64);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if interruptible && !flag.load(Ordering::SeqCst) {
            return;
        }
        thread::sleep(left.min(Duration::from_millis(10)));
    }
}
