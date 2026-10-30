//! Windows virtual-key names for the UI.

use std::sync::OnceLock;

const NAMED: &[(u16, &str)] = &[
    (0x08, "Backspace"),
    (0x09, "Tab"),
    (0x0D, "Enter"),
    (0x10, "Shift"),
    (0x11, "Ctrl"),
    (0x12, "Alt"),
    (0x13, "Pause"),
    (0x14, "Caps Lock"),
    (0x1B, "Esc"),
    (0x20, "Space"),
    (0x21, "Page Up"),
    (0x22, "Page Down"),
    (0x23, "End"),
    (0x24, "Home"),
    (0x25, "Left"),
    (0x26, "Up"),
    (0x27, "Right"),
    (0x28, "Down"),
    (0x2C, "Print Screen"),
    (0x2D, "Insert"),
    (0x2E, "Delete"),
    (0x5B, "Win"),
    (0x5D, "Menu"),
    (0x6A, "Num *"),
    (0x6B, "Num +"),
    (0x6D, "Num -"),
    (0x6E, "Num ."),
    (0x6F, "Num /"),
    (0x90, "Num Lock"),
    (0x91, "Scroll Lock"),
    (0xA0, "Left Shift"),
    (0xA1, "Right Shift"),
    (0xA2, "Left Ctrl"),
    (0xA3, "Right Ctrl"),
    (0xA4, "Left Alt"),
    (0xA5, "Right Alt"),
    (0xAD, "Mute"),
    (0xAE, "Volume Down"),
    (0xAF, "Volume Up"),
    (0xB0, "Next Track"),
    (0xB1, "Previous Track"),
    (0xB2, "Stop Media"),
    (0xB3, "Play / Pause"),
    (0xBA, ";"),
    (0xBB, "="),
    (0xBC, ","),
    (0xBD, "-"),
    (0xBE, "."),
    (0xBF, "/"),
    (0xC0, "`"),
    (0xDB, "["),
    (0xDC, "\\"),
    (0xDD, "]"),
    (0xDE, "'"),
];

pub fn name(vk: u16) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => NAMED
            .iter()
            .find(|(v, _)| *v == vk)
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| format!("Key {vk:#04x}")),
    }
}

/// Every key offered in the key picker, in display order.
pub fn all() -> &'static [(u16, String)] {
    static ALL: OnceLock<Vec<(u16, String)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<u16> = (0x41..=0x5A).chain(0x30..=0x39).chain(0x70..=0x87).collect();
        v.extend(NAMED.iter().map(|(k, _)| *k));
        v.extend(0x60..=0x69);
        v.into_iter().map(|k| (k, name(k))).collect()
    })
}
