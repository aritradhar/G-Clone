//! Prints every input report from every openable HID collection of one device, to find
//! out how buttons the OS ignores (e.g. G602 G6–G11) are reported.
//!
//! cargo run -p gclone-core --example listen -- <pid hex> [seconds]

use gclone_core::LOGITECH_VID;
use hidapi::HidApi;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pid = u16::from_str_radix(args.first().map(String::as_str).unwrap_or("c537"), 16)?;
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let api = HidApi::new()?;
    let mut devs = Vec::new();
    for info in api.device_list().filter(|d| d.vendor_id() == LOGITECH_VID && d.product_id() == pid) {
        let tag = format!("{:04X}:{:04X}", info.usage_page(), info.usage());
        match info.open_device(&api) {
            Ok(d) => {
                println!("listening on {tag}");
                devs.push((tag, d));
            }
            Err(e) => println!("cannot open {tag}: {e}"),
        }
    }
    let end = Instant::now() + Duration::from_secs(secs);
    let mut buf = [0u8; 64];
    // Windows owns keyboard/mouse collections exclusively; reads from them fail, so drop them.
    let mut alive = vec![true; devs.len()];
    while Instant::now() < end {
        for (i, (tag, d)) in devs.iter().enumerate() {
            if !alive[i] {
                continue;
            }
            let n = match d.read_timeout(&mut buf, 2) {
                Ok(n) => n,
                Err(e) => {
                    println!("{tag}: not readable ({e})");
                    alive[i] = false;
                    continue;
                }
            };
            if n > 0 {
                let hex: Vec<String> = buf[..n].iter().map(|b| format!("{b:02x}")).collect();
                println!("{tag}  {}", hex.join(" "));
            }
        }
    }
    Ok(())
}
