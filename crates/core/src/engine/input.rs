//! Synthetic keyboard / mouse input via `SendInput`.

use crate::config::{Mods, MouseButton};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

/// Stamped into `dwExtraInfo` of everything we inject.
pub const INJECT_TAG: usize = 0x4743_4C4E;

const XBUTTON1: u32 = 1;
const XBUTTON2: u32 = 2;

fn key_input(vk: u16, down: bool) -> INPUT {
    // Games often read scan codes rather than virtual keys, so send both.
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: INJECT_TAG },
        },
    }
}

fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0x6F | 0x90 | 0xA3 | 0xA5)
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn key(vk: u16, down: bool) {
    send(&[key_input(vk, down)]);
}

/// Presses modifiers then the key (down), or releases in reverse order (up).
pub fn combo(vk: u16, mods: Mods, down: bool) {
    let mut held: Vec<u16> = Vec::with_capacity(4);
    if mods.ctrl {
        held.push(0xA2);
    }
    if mods.shift {
        held.push(0xA0);
    }
    if mods.alt {
        held.push(0xA4);
    }
    if mods.win {
        held.push(0x5B);
    }
    let mut seq: Vec<INPUT> = Vec::with_capacity(held.len() + 1);
    if down {
        seq.extend(held.iter().map(|m| key_input(*m, true)));
        seq.push(key_input(vk, true));
    } else {
        seq.push(key_input(vk, false));
        seq.extend(held.iter().rev().map(|m| key_input(*m, false)));
    }
    send(&seq);
}

pub fn mouse(button: MouseButton, down: bool) {
    let (flags, data) = match (button, down) {
        (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
        (MouseButton::Back, false) => (MOUSEEVENTF_XUP, XBUTTON1),
        (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
        (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, XBUTTON2),
    };
    send(&[INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: INJECT_TAG },
        },
    }]);
}

/// Types arbitrary text as Unicode key events, independent of keyboard layout.
pub fn text(s: &str) {
    let seq: Vec<INPUT> = s
        .encode_utf16()
        .flat_map(|u| {
            [false, true].map(|up| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: u,
                        dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                        time: 0,
                        dwExtraInfo: INJECT_TAG,
                    },
                },
            })
        })
        .collect();
    send(&seq);
}
