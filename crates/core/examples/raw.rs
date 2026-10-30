//! Sends one raw HID++ 2.0 call and prints the reply. For protocol exploration.
//!
//! cargo run -p gclone-core --example raw -- <pid hex> <feature hex> <function> [param bytes hex...]
//! e.g. raw c537 2201 0

use gclone_core::devices::{discover, Kind};
use hidapi::HidApi;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: raw <pid> <feature> <function> [params...]");
        std::process::exit(2);
    }
    let pid = u16::from_str_radix(&args[0], 16)?;
    let feature = u16::from_str_radix(&args[1], 16)?;
    let function: u8 = args[2].parse()?;
    let params: Vec<u8> = args[3..].iter().map(|p| u8::from_str_radix(p, 16)).collect::<Result<_, _>>()?;

    let api = HidApi::new()?;
    let (_, _, mut dev) = discover(&api, |_| true)
        .into_iter()
        .find(|(k, p, _)| *p == pid && Kind::for_pid(pid) == Some(*k))
        .ok_or("device not found")?;
    match dev.call(feature, function, &params) {
        Ok(r) => println!("{r:?}"),
        Err(e) => println!("error: {e}"),
    }
    Ok(())
}
