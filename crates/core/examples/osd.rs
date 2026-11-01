//! Shows the on-screen DPI popup once, for checking how it looks.
//!
//! cargo run -p gclone-core --example osd

fn main() {
    gclone_core::engine::osd::show("1500 DPI", "Stage 3 of 4");
    std::thread::sleep(std::time::Duration::from_millis(1800));
}
