//! Prints live wheel/pedal axes and pressed buttons of the G923 for a few seconds.
//!
//! cargo run -p gclone-core --example axes -- [seconds]

use gclone_core::devices::{joystick::Joystick, PID_G923_XBOX};
use hidapi::HidApi;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let api = HidApi::new()?;
    let mut joy = Joystick::open(&api, PID_G923_XBOX).ok_or("G923 joystick collection not found")?;
    println!("buttons: {}", joy.button_count);
    for a in &joy.axes {
        println!("axis {:>6}  page {:02X} usage {:02X}  range {}..{}", a.name(), a.page, a.usage, a.min, a.max);
    }
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last = None;
    while Instant::now() < end {
        let Some(read) = joy.read() else { break };
        let Some(s) = read else { continue };
        if last.as_ref() != Some(&s) {
            let vals: Vec<String> = joy.axes.iter().zip(&s.values).map(|(a, v)| format!("{}={v}", a.name())).collect();
            println!("{}  buttons {:?}", vals.join(" "), s.buttons);
            last = Some(s);
        }
    }
    Ok(())
}
