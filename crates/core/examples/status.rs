//! Prints the current settings of every supported device. Read-only.
//!
//! cargo run -p gclone-core --example status

use gclone_core::devices::{discover, litra::Litra, mouse::Mouse, wheel::{Axis, Wheel}, Kind};
use hidapi::HidApi;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let api = HidApi::new()?;
    for (kind, pid, dev) in discover(&api, |_| true) {
        println!("\n== {kind:?} (046D:{pid:04X}) ==");
        match kind {
            Kind::Mouse => {
                let mut m = Mouse::new(dev);
                println!("  dpi          {:?}", m.dpi());
                println!("  dpi values   {:?}", m.dpi_values());
                println!("  report rate  {:?} ms", m.report_rate());
                println!("  rates        {:?} ms", m.report_rates());
                println!("  battery      {:?}", m.battery());
            }
            Kind::Light => {
                let mut l = Litra::new(dev);
                println!("  on           {:?}", l.is_on());
                println!("  brightness   {:?} lm", l.brightness());
                println!("  temperature  {:?} K", l.temperature());
            }
            Kind::Wheel => {
                let mut w = Wheel::new(dev);
                println!("  range        {:?} deg", w.range());
                println!("  strength     {:?} %", w.strength());
                println!("  effect slots {:?}", w.effect_slots());
                for axis in Axis::ALL {
                    println!("  sensitivity  {axis:?}: {:?}", w.sensitivity(axis));
                }
            }
        }
    }
    Ok(())
}
