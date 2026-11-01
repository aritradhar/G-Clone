//! Real device photos, taken from a local G HUB install at runtime (never bundled: they are
//! Logitech's images). G HUB's `metadata.json` next to them gives each button's marker
//! position, so callouts land exactly on the buttons. Photos are downscaled once and cached
//! in the data folder, keeping load time and memory low.

use eframe::egui::{self, pos2, vec2, Pos2, TextureHandle, Vec2};
use gclone_core::config::data_dir;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Longest side of a cached photo, in pixels: sharp at 150 % scaling, still small in memory.
const MAX_SIDE: u32 = 1100;

#[derive(Deserialize)]
struct Metadata {
    images: Vec<MetaImage>,
}

#[derive(Deserialize)]
struct MetaImage {
    key: String,
    origin: Size,
    #[serde(default)]
    assignments: Vec<MetaAssignment>,
}

#[derive(Deserialize)]
struct Size {
    width: f32,
    height: f32,
}

#[derive(Deserialize)]
struct MetaAssignment {
    #[serde(rename = "slotId")]
    slot_id: String,
    marker: Point,
    label: Point,
}

#[derive(Deserialize)]
struct Point {
    x: f32,
    y: f32,
}

/// A button position on a photo.
#[derive(Clone, Debug)]
pub struct Marker {
    /// Button number from the slot id (`g602_g7_m1` → 7, `g923-xbox_g19_m1` → 19).
    pub button: u16,
    /// Position in the photo's original pixel space (see [`Photo::origin`]).
    pub pos: Pos2,
    /// G HUB puts this button's label on the left.
    pub label_left: bool,
    /// G HUB's label height, used to order labels top to bottom.
    pub label_y: f32,
}

pub struct Photo {
    pub texture: TextureHandle,
    /// Size of the original image; marker coordinates are relative to it.
    pub origin: Vec2,
    pub markers: Vec<Marker>,
}

impl Photo {
    pub fn marker(&self, button: u16) -> Option<&Marker> {
        self.markers.iter().find(|m| m.button == button)
    }
}

/// Photos are loaded per device the first time its page is shown.
#[derive(Default)]
pub struct Photos {
    pub mouse_top: Option<Photo>,
    pub mouse_side: Option<Photo>,
    pub wheel_front: Option<Photo>,
    pub wheel_base: Option<Photo>,
    pub pedals: Option<Photo>,
    mouse_tried: bool,
    wheel_tried: bool,
}

/// Folder holding G HUB's per-device assets (`…\LGHUB\depots\<n>\`), if installed.
fn depot_dir(device: &str) -> Option<PathBuf> {
    let root = Path::new(r"C:\ProgramData\LGHUB\depots");
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path().join(device))
        .find(|p| p.join("metadata.json").exists())
}

fn button_of(slot: &str) -> Option<u16> {
    // "<device>_g<n>_m1"
    slot.rsplit('_').nth(1)?.strip_prefix('g')?.parse().ok()
}

impl Photos {
    /// G602 top and side photos, if G HUB has them on this PC.
    pub fn ensure_mouse(&mut self, ctx: &egui::Context) {
        if std::mem::replace(&mut self.mouse_tried, true) {
            return;
        }
        if let Some(dir) = depot_dir("g602") {
            let meta = read_meta(&dir);
            self.mouse_top = load_photo(ctx, &dir.join("g602_top.png"), meta.as_ref(), "device_image", "g602_top");
            self.mouse_side = load_photo(ctx, &dir.join("g602_side.png"), meta.as_ref(), "device_side", "g602_side");
        }
    }

    /// G923 wheel (front + base) and pedal photos, if G HUB has them on this PC.
    pub fn ensure_wheel(&mut self, ctx: &egui::Context) {
        if std::mem::replace(&mut self.wheel_tried, true) {
            return;
        }
        if let Some(dir) = depot_dir("g923_xbox") {
            let meta = read_meta(&dir);
            let img = dir.join("images");
            self.wheel_front =
                load_photo(ctx, &img.join("wheel_front.webp"), meta.as_ref(), "wheel_image_front", "g923_front");
            self.wheel_base =
                load_photo(ctx, &img.join("wheel_base.webp"), meta.as_ref(), "wheel_image_base", "g923_base");
            self.pedals = load_photo(ctx, &img.join("pedals.webp"), meta.as_ref(), "pedals_image", "g923_pedals");
        }
    }
}

fn read_meta(dir: &Path) -> Option<Metadata> {
    let text = std::fs::read_to_string(dir.join("metadata.json")).ok()?;
    serde_json::from_str(&text).map_err(|e| log::warn!("G HUB metadata: {e}")).ok()
}

fn load_photo(ctx: &egui::Context, src: &Path, meta: Option<&Metadata>, key: &str, cache_name: &str) -> Option<Photo> {
    let entry = meta.and_then(|m| m.images.iter().find(|i| i.key == key));
    let rgba = cached_rgba(src, cache_name)?;
    let origin = match entry {
        Some(e) => vec2(e.origin.width, e.origin.height),
        None => vec2(rgba.width() as f32, rgba.height() as f32),
    };
    let markers = entry
        .map(|e| {
            e.assignments
                .iter()
                .filter_map(|a| {
                    Some(Marker {
                        button: button_of(&a.slot_id)?,
                        pos: pos2(a.marker.x, a.marker.y),
                        label_left: a.label.x < e.origin.width / 2.0,
                        label_y: a.label.y,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let image = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    let texture = ctx.load_texture(cache_name, image, egui::TextureOptions::LINEAR);
    Some(Photo { texture, origin, markers })
}

/// Decoded, downscaled photo, read from the cache when it's newer than the source.
fn cached_rgba(src: &Path, cache_name: &str) -> Option<image::RgbaImage> {
    let cache = data_dir().join("cache").join(format!("{cache_name}.png"));
    let fresh = |c: &Path| {
        let (Ok(c), Ok(s)) = (std::fs::metadata(c), std::fs::metadata(src)) else { return false };
        matches!((c.modified(), s.modified()), (Ok(c), Ok(s)) if c >= s)
    };
    if fresh(&cache) {
        if let Ok(img) = image::open(&cache) {
            return Some(img.to_rgba8());
        }
    }
    let img = image::open(src).map_err(|e| log::warn!("photo {}: {e}", src.display())).ok()?;
    let img = if img.width().max(img.height()) > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    if std::fs::create_dir_all(cache.parent().unwrap()).is_ok() {
        let _ = rgba.save(&cache);
    }
    Some(rgba)
}

#[cfg(test)]
mod tests {
    #[test]
    fn slot_ids_give_button_numbers() {
        assert_eq!(super::button_of("g602_g7_m1"), Some(7));
        assert_eq!(super::button_of("g923-xbox_g19_m1"), Some(19));
        assert_eq!(super::button_of("weird"), None);
    }
}
