//! Dumps every Logitech HID collection and the HID++ feature table of each device.
//! Read-only: only Root, FeatureSet and DeviceName queries are sent.
//!
//! cargo run -p gclone-core --example probe

use gclone_core::hidpp::{features, Channel, Device, DEVICE_DIRECT};
use gclone_core::LOGITECH_VID;
use hidapi::HidApi;
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let api = HidApi::new()?;

    let mut by_pid: BTreeMap<u16, Vec<&hidapi::DeviceInfo>> = BTreeMap::new();
    for info in api.device_list().filter(|d| d.vendor_id() == LOGITECH_VID) {
        by_pid.entry(info.product_id()).or_default().push(info);
    }

    for (pid, infos) in &by_pid {
        println!("\n=== 046D:{pid:04X}  {} ===", infos[0].product_string().unwrap_or("?"));
        for i in infos {
            println!(
                "  iface {:>2}  usage {:04X}:{:04X}  {}",
                i.interface_number(),
                i.usage_page(),
                i.usage(),
                i.path().to_string_lossy()
            );
        }

        let chan = match Channel::open(&api, infos) {
            Ok(c) => c,
            Err(e) => {
                println!("  (no HID++ channel: {e})");
                continue;
            }
        };
        let mut dev = Device::new(chan, DEVICE_DIRECT);
        for index in [DEVICE_DIRECT, 1, 2, 3, 4, 5, 6] {
            dev.index = index;
            match dev.protocol_version() {
                Ok((maj, min)) => {
                    println!("  device index {index:#04x}: HID++ {maj}.{min}");
                    if maj >= 2 {
                        dump_features(&mut dev);
                    }
                }
                Err(e) => log::debug!("index {index:#04x}: {e}"),
            }
        }
    }
    Ok(())
}

fn dump_features(dev: &mut Device) {
    match dev.name() {
        Ok(n) => println!("    name: {n}"),
        Err(e) => println!("    name: <{e}>"),
    }
    match dev.list_features() {
        Ok(list) => {
            for f in list {
                println!(
                    "    [{:>2}] {:#06x} v{} flags {:#04x}  {}",
                    f.index,
                    f.id,
                    f.version,
                    f.flags,
                    features::name(f.id)
                );
            }
        }
        Err(e) => println!("    feature table: <{e}>"),
    }
}
