//! Device figures drawn from G HUB's photos (see `photos.rs`), with callouts and hotspots at
//! the button positions from G HUB's metadata. Used instead of the vector drawings in
//! `figures.rs` whenever the photos are available.

use crate::figures::{canvas, Callout, ACCENT, LABEL_W, MOUSE_CANVAS, PEDAL_CANVAS};
use crate::photos::{Marker, Photo};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use gclone_core::config::{MousePart, WheelPart};

const DIM: Color32 = Color32::from_rgb(0x3A, 0x3C, 0x42);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);
const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0x9C, 0xA2);
const GREEN: Color32 = Color32::from_rgb(0x4C, 0xD9, 0x64);
const BLUE: Color32 = Color32::from_rgb(0x1E, 0x90, 0xE6);
const FULL_UV: Rect = Rect { min: Pos2 { x: 0.0, y: 0.0 }, max: Pos2 { x: 1.0, y: 1.0 } };

/// G HUB slot number of each G602 button (`g602_g<n>_m1`).
fn mouse_slot(part: MousePart) -> u16 {
    use MousePart::*;
    match part {
        Left => 1,
        Right => 2,
        Middle => 3,
        G4 => 4,
        G5 => 5,
        G6 => 6,
        G7 => 7,
        G8 => 8,
        G9 => 9,
        G10 => 10,
        G11 => 11,
    }
}

/// G HUB slot number (= HID button number) of each G923 face button (`g923-xbox_g<n>_m1`).
pub fn wheel_slot(part: WheelPart) -> u16 {
    use WheelPart::*;
    match part {
        A => 1,
        B => 2,
        X => 3,
        Y => 4,
        RB => 5,
        LB => 6,
        Menu => 7,
        View => 8,
        RSB => 9,
        LSB => 10,
        Xbox => 11,
        Plus => 19,
        Minus => 20,
        DialRight => 21,
        DialLeft => 22,
        Enter => 23,
    }
}

/// D-pad markers are slots 24 (up) … 31 (up-left), clockwise like the HID hat value.
const HAT_FIRST_SLOT: u16 = 24;

/// Draws a photo scaled to `height`, centred in `area`; returns its screen rect and scale.
fn place(painter: &egui::Painter, photo: &Photo, center: Pos2, height: f32) -> (Rect, f32) {
    let s = height / photo.origin.y;
    let rect = Rect::from_center_size(center, photo.origin * s);
    painter.image(photo.texture.id(), rect, FULL_UV, Color32::WHITE);
    (rect, s)
}

/// G602 photo (top or side) with G HUB-style callouts. Returns the clicked button.
pub fn mouse(ui: &mut Ui, photo: &Photo, parts: &[Callout], mode: Option<bool>, show_mode: bool) -> Option<MousePart> {
    let (painter, o) = canvas(ui, MOUSE_CANVAS);
    let (img, s) = place(&painter, photo, o + MOUSE_CANVAS / 2.0, MOUSE_CANVAS.y - 40.0);

    let mut items: Vec<(&Callout, &Marker)> =
        parts.iter().filter_map(|c| photo.marker(mouse_slot(c.part)).map(|m| (c, m))).collect();
    items.sort_by(|a, b| a.1.label_y.total_cmp(&b.1.label_y));
    let (left, right): (Vec<_>, Vec<_>) = items.into_iter().partition(|(_, m)| m.label_left);

    let mut clicked = None;
    for (side_left, list) in [(true, left), (false, right)] {
        let n = list.len().max(1) as f32;
        for (i, (c, m)) in list.into_iter().enumerate() {
            let y = o.y + MOUSE_CANVAS.y * (0.10 + 0.80 * (i as f32 + 0.5) / n);
            let (x0, x1) = if side_left { (o.x, o.x + LABEL_W) } else { (o.x + MOUSE_CANVAS.x - LABEL_W, o.x + MOUSE_CANVAS.x) };
            let dot = img.min + m.pos.to_vec2() * s;
            let id = ui.id().with(("photo_mouse", c.part));
            let line = if c.selected { ACCENT } else { DIM };
            let joint = if side_left { pos2(x1, y) } else { pos2(x0, y) };
            painter.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.0_f32, line));
            painter.line_segment([joint, dot], Stroke::new(1.0_f32, line));

            let hit = Rect::from_center_size(dot, Vec2::splat(30.0));
            let label = Rect::from_min_max(pos2(x0, y - 40.0), pos2(x1, y + 4.0));
            let resp = ui.interact(hit, id, Sense::click()) | ui.interact(label, id.with("label"), Sense::click());
            let (anchor, align) = if side_left { (pos2(x0, y - 4.0), Align2::LEFT_BOTTOM) } else { (pos2(x1, y - 4.0), Align2::RIGHT_BOTTOM) };
            let title_col = if c.selected || resp.hovered() { ACCENT } else { TEXT_DIM };
            painter.text(anchor - vec2(0.0, 19.0), align, c.part.label(), FontId::proportional(12.0), title_col);
            painter.text(anchor, align, truncate(&c.assignment, 20), FontId::proportional(15.0), TEXT);

            marker_dot(&painter, dot, c.pressed, c.selected, resp.hovered());
            if resp.clicked() {
                clicked = Some(c.part);
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    }

    if show_mode {
        let (text, col) = match mode {
            Some(true) => ("Endurance (125 Hz)", GREEN),
            Some(false) => ("Performance", TEXT),
            None => ("Unknown", TEXT_DIM),
        };
        let at = o + vec2(MOUSE_CANVAS.x, MOUSE_CANVAS.y - 4.0);
        painter.text(at - vec2(0.0, 19.0), Align2::RIGHT_BOTTOM, "Mode switch", FontId::proportional(12.0), TEXT_DIM);
        painter.text(at, Align2::RIGHT_BOTTOM, text, FontId::proportional(15.0), col);
    }
    clicked
}

/// White marker dot like G HUB's; teal when pressed or selected.
fn marker_dot(painter: &egui::Painter, at: Pos2, pressed: bool, selected: bool, hovered: bool) {
    if pressed {
        painter.circle_filled(at, 13.0, ACCENT.linear_multiply(0.45));
    }
    let ring = if selected || hovered { ACCENT } else { Color32::from_black_alpha(160) };
    let fill = if pressed { ACCENT } else { Color32::WHITE };
    painter.circle(at, if selected { 7.5 } else { 6.5 }, fill, Stroke::new(2.0_f32, ring));
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

/// Draws the G923 photo (base behind, wheel front rotated to `angle`) centred at `center`,
/// with button hotspots. Returns the clicked part.
pub fn wheel(
    ui: &mut Ui,
    painter: &egui::Painter,
    front: &Photo,
    base: Option<&Photo>,
    center: Pos2,
    size: f32,
    angle_deg: f32,
    parts: &[(WheelPart, bool, bool)],
    hat: Option<u8>,
) -> Option<WheelPart> {
    let s = size / front.origin.x;
    if let Some(base) = base {
        painter.image(base.texture.id(), Rect::from_center_size(center, base.origin * s), FULL_UV, Color32::WHITE);
    }
    let rect = Rect::from_center_size(center, front.origin * s);
    let angle = angle_deg.to_radians();
    egui::Image::new((front.texture.id(), rect.size())).rotate(angle, Vec2::splat(0.5)).paint_at(ui, rect);

    // Marker positions rotate with the wheel.
    let (sin, cos) = angle.sin_cos();
    let at = |m: &Marker| {
        let d = (m.pos.to_vec2() - front.origin / 2.0) * s;
        center + vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    };

    if let Some(m) = hat.and_then(|h| front.marker(HAT_FIRST_SLOT + h as u16)) {
        painter.circle_filled(at(m), 11.0, ACCENT.linear_multiply(0.7));
    }

    let mut clicked = None;
    for (part, pressed, selected) in parts {
        let Some(m) = front.marker(wheel_slot(*part)) else { continue };
        let p = at(m);
        let resp = ui.interact(Rect::from_center_size(p, Vec2::splat(26.0)), ui.id().with(("photo_wheel", *part)), Sense::click());
        if *pressed {
            painter.circle_filled(p, 12.0, ACCENT.linear_multiply(0.6));
        }
        if *selected || resp.hovered() {
            painter.circle_stroke(p, 12.0, Stroke::new(2.5_f32, ACCENT));
        }
        if resp.clicked() {
            clicked = Some(*part);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            resp.on_hover_text(part.label());
        }
    }
    clicked
}

/// Pedals photo with a live travel ladder above each pedal; pressed pedals light up.
/// `pedals` is (label, travel 0..1) for clutch, brake and accelerator.
pub fn pedals(ui: &mut Ui, photo: &Photo, pedals: [(&str, Option<f32>); 3]) {
    let (painter, o) = canvas(ui, PEDAL_CANVAS);
    let (ladder_top, ladder_h, ticks) = (10.0, 140.0, 20);
    let img_h = PEDAL_CANVAS.y - ladder_h - 80.0;
    let (img, _) = place(&painter, photo, o + vec2(PEDAL_CANVAS.x / 2.0, PEDAL_CANVAS.y - img_h / 2.0), img_h);
    // Pedal plates in the photo, as fractions of its size (from G HUB's outline paths).
    let plates = [(-0.03, 0.22, 0.35), (0.34, 0.59, 0.35), (0.78, 1.0, 0.39)];

    for (i, (name, value)) in pedals.iter().enumerate() {
        let (l, r, b) = plates[i];
        let x = img.left() + img.width() * (l + r) / 2.0;
        let v = value.unwrap_or(0.0).clamp(0.0, 1.0);
        let lit = (v * ticks as f32).round() as usize;
        for t in 0..ticks {
            let y = o.y + ladder_top + ladder_h - ladder_h * (t as f32 + 0.5) / ticks as f32;
            let col = if t < lit { BLUE } else { DIM };
            painter.line_segment([pos2(x - 26.0, y), pos2(x + 26.0, y)], Stroke::new(2.0_f32, col));
        }
        let below = o.y + ladder_top + ladder_h;
        let pct = value.map_or("–".to_string(), |_| format!("{:.0}%", v * 100.0));
        painter.text(pos2(x, below + 8.0), Align2::CENTER_TOP, *name, FontId::proportional(15.0), TEXT);
        painter.text(pos2(x, below + 28.0), Align2::CENTER_TOP, pct, FontId::proportional(13.0), TEXT_DIM);

        // Tint the pedal plate by how far it's pressed.
        if v > 0.02 {
            let plate = Rect::from_min_max(
                pos2(img.left() + img.width() * l.max(0.0), img.top()),
                pos2(img.left() + img.width() * r.min(1.0), img.top() + img.height() * b),
            );
            painter.rect_filled(plate, 8.0, ACCENT.linear_multiply(0.45 * v));
        }
    }
}
