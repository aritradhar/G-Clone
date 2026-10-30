//! Vector drawings of the devices with clickable, live-highlighted parts.

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use gclone_core::config::{MousePart, WheelPart};

pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0xC8, 0xD7);
const BLUE: Color32 = Color32::from_rgb(0x1E, 0x90, 0xE6);
const BODY: Color32 = Color32::from_rgb(0x26, 0x28, 0x2D);
const BODY_EDGE: Color32 = Color32::from_rgb(0x44, 0x48, 0x50);
const PART: Color32 = Color32::from_rgb(0x34, 0x37, 0x3E);
const PART_HOVER: Color32 = Color32::from_rgb(0x48, 0x4C, 0x55);
const DIM: Color32 = Color32::from_rgb(0x3A, 0x3C, 0x42);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);
const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0x9C, 0xA2);

fn part_fill(pressed: bool, hovered: bool) -> Color32 {
    if pressed {
        ACCENT.linear_multiply(0.75)
    } else if hovered {
        PART_HOVER
    } else {
        PART
    }
}

// ------------------------------------------------------------------ mouse

pub struct Callout {
    pub part: MousePart,
    pub assignment: String,
    pub pressed: bool,
    pub selected: bool,
}

enum Geo {
    Quad([(f32, f32); 4]),
    Pill((f32, f32), (f32, f32)),
}

/// Shape of each G602 part in a 0..1 box (x right, y down), plus its callout side.
fn mouse_geo(part: MousePart) -> (Geo, bool) {
    use MousePart::*;
    match part {
        Left => (Geo::Quad([(0.30, 0.06), (0.47, 0.03), (0.47, 0.37), (0.27, 0.39)]), true),
        Right => (Geo::Quad([(0.53, 0.03), (0.70, 0.06), (0.79, 0.35), (0.53, 0.37)]), false),
        Middle => (Geo::Pill((0.50, 0.17), (0.035, 0.07)), false),
        G10 => (Geo::Pill((0.245, 0.13), (0.022, 0.04)), true),
        G11 => (Geo::Pill((0.225, 0.23), (0.022, 0.04)), true),
        G4 => (Geo::Pill((0.205, 0.43), (0.022, 0.035)), true),
        G5 => (Geo::Pill((0.185, 0.52), (0.022, 0.035)), true),
        G6 => (Geo::Pill((0.165, 0.61), (0.022, 0.035)), true),
        G7 => (Geo::Pill((0.255, 0.45), (0.022, 0.035)), true),
        G8 => (Geo::Pill((0.235, 0.54), (0.022, 0.035)), true),
        G9 => (Geo::Pill((0.215, 0.63), (0.022, 0.035)), true),
    }
}

const BODY_POLY: [(f32, f32); 12] = [
    (0.30, 0.04),
    (0.50, 0.01),
    (0.70, 0.04),
    (0.82, 0.30),
    (0.88, 0.62),
    (0.84, 0.86),
    (0.62, 0.98),
    (0.36, 0.98),
    (0.14, 0.88),
    (0.10, 0.68),
    (0.16, 0.40),
    (0.24, 0.10),
];

/// Draws the G602 with callouts; returns the part the user clicked.
pub fn mouse(ui: &mut Ui, parts: &[Callout]) -> Option<MousePart> {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    let painter = ui.painter_at(rect);
    let bh = (rect.height() * 0.86).min(rect.width() * 0.75);
    let bw = bh * 0.72;
    let origin = rect.center() - vec2(bw / 2.0, bh / 2.0);
    let map = |(u, v): (f32, f32)| origin + vec2(u * bw, v * bh);

    let body: Vec<Pos2> = BODY_POLY.iter().map(|p| map(*p)).collect();
    painter.add(Shape::convex_polygon(body, BODY, Stroke::new(1.5_f32, BODY_EDGE)));
    // Brushed-metal centre strip and logo.
    let strip = [(0.47, 0.37), (0.53, 0.37), (0.57, 0.90), (0.43, 0.90)].map(map).to_vec();
    painter.add(Shape::convex_polygon(strip, Color32::from_rgb(0x55, 0x59, 0x60), Stroke::NONE));
    painter.text(map((0.50, 0.80)), Align2::CENTER_CENTER, "G", FontId::proportional(bh * 0.05), TEXT_DIM);

    let mut clicked = None;
    let (left_parts, right_parts): (Vec<&Callout>, Vec<&Callout>) = parts.iter().partition(|c| mouse_geo(c.part).1);
    let label_w = ((rect.width() - bw) / 2.0 - 30.0).clamp(120.0, 260.0);

    for (side_left, list) in [(true, &left_parts), (false, &right_parts)] {
        let n = list.len().max(1) as f32;
        for (i, c) in list.iter().enumerate() {
            let (geo, _) = mouse_geo(c.part);
            let id = ui.id().with(("mouse", c.part));
            // Part shape.
            let (center, hit) = match &geo {
                Geo::Quad(q) => {
                    let pts: Vec<Pos2> = q.iter().map(|p| map(*p)).collect();
                    let r = Rect::from_points(&pts);
                    (r.center(), r)
                }
                Geo::Pill(c, s) => {
                    let r = Rect::from_center_size(map(*c), vec2(s.0 * 2.0 * bw, s.1 * 2.0 * bh));
                    (r.center(), r)
                }
            };
            let resp = ui.interact(hit, id, Sense::click());
            let fill = part_fill(c.pressed, resp.hovered());
            match &geo {
                Geo::Quad(q) => {
                    let pts: Vec<Pos2> = q.iter().map(|p| map(*p)).collect();
                    painter.add(Shape::convex_polygon(pts, fill, Stroke::new(1.0_f32, BODY_EDGE)));
                }
                Geo::Pill(..) => {
                    painter.rect(hit, hit.width().min(hit.height()) / 2.0, fill, Stroke::new(1.0_f32, BODY_EDGE));
                }
            }

            // Callout: label column on the side, line to a dot on the part.
            let y = rect.top() + rect.height() * (0.10 + 0.80 * (i as f32 + 0.5) / n);
            let (x0, x1) = if side_left {
                (rect.left() + 12.0, rect.left() + 12.0 + label_w)
            } else {
                (rect.right() - 12.0 - label_w, rect.right() - 12.0)
            };
            let elbow = if side_left { x1 } else { x0 };
            let line = if c.selected { ACCENT } else { DIM };
            painter.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.0_f32, line));
            painter.line_segment([pos2(elbow, y), center], Stroke::new(1.0_f32, line));
            let label_rect = Rect::from_min_max(pos2(x0, y - 38.0), pos2(x1, y + 4.0));
            let label_resp = ui.interact(label_rect, id.with("label"), Sense::click());
            let (anchor, align) = if side_left { (pos2(x0, y - 4.0), Align2::LEFT_BOTTOM) } else { (pos2(x1, y - 4.0), Align2::RIGHT_BOTTOM) };
            let title_col = if c.selected || label_resp.hovered() { ACCENT } else { TEXT_DIM };
            painter.text(anchor - vec2(0.0, 18.0), align, c.part.label(), FontId::proportional(12.0), title_col);
            painter.text(anchor, align, &c.assignment, FontId::proportional(16.0), TEXT);

            let dot_r = if c.selected { 7.0 } else { 6.0 };
            let dot_fill = if c.pressed { ACCENT } else { Color32::WHITE };
            painter.circle(center, dot_r, dot_fill, Stroke::new(2.0_f32, if c.selected { ACCENT } else { BODY }));

            if resp.clicked() || label_resp.clicked() {
                clicked = Some(c.part);
            }
            if resp.hovered() || label_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    }
    clicked
}

// ------------------------------------------------------------------ wheel

pub struct WheelView<'a> {
    /// Current steering angle in degrees (negative = left).
    pub angle: Option<f32>,
    pub range: f32,
    pub show_arc: bool,
    /// (part, pressed, selected)
    pub parts: &'a [(WheelPart, bool, bool)],
    /// D-pad direction 0..7 (0 = up, clockwise) or None.
    pub hat: Option<u8>,
}

fn wheel_part_pos(p: WheelPart) -> (f32, f32, f32) {
    use WheelPart::*;
    match p {
        Y => (0.56, -0.13, 0.045),
        X => (0.46, -0.03, 0.045),
        B => (0.66, -0.03, 0.045),
        A => (0.56, 0.07, 0.045),
        View => (-0.56, 0.15, 0.035),
        Menu => (0.56, 0.17, 0.035),
        LSB => (-0.42, 0.25, 0.035),
        RSB => (0.42, 0.25, 0.035),
        Plus => (-0.30, 0.33, 0.04),
        Minus => (-0.30, 0.44, 0.04),
        Enter => (0.30, 0.385, 0.045),
        DialLeft => (0.19, 0.385, 0.03),
        DialRight => (0.41, 0.385, 0.03),
        Xbox => (0.0, 0.57, 0.045),
        LB => (-0.80, -0.32, 0.055),
        RB => (0.80, -0.32, 0.055),
    }
}

fn part_glyph(p: WheelPart) -> (&'static str, Color32) {
    use WheelPart::*;
    match p {
        A => ("A", Color32::from_rgb(0x5B, 0xC2, 0x36)),
        B => ("B", Color32::from_rgb(0xE0, 0x3C, 0x31)),
        X => ("X", Color32::from_rgb(0x2E, 0x8B, 0xE6)),
        Y => ("Y", Color32::from_rgb(0xE8, 0xC2, 0x22)),
        View => ("⧉", TEXT_DIM),
        Menu => ("☰", TEXT_DIM),
        LSB => ("LSB", TEXT_DIM),
        RSB => ("RSB", TEXT_DIM),
        Plus => ("+", TEXT),
        Minus => ("−", TEXT),
        Enter => ("↵", TEXT),
        DialLeft => ("‹", TEXT_DIM),
        DialRight => ("›", TEXT_DIM),
        Xbox => ("⊗", TEXT),
        LB => ("LB", TEXT),
        RB => ("RB", TEXT),
    }
}

/// Draws the wheel (rotated to the live angle) with optional range arc; returns a clicked part.
pub fn wheel(ui: &mut Ui, view: &WheelView) -> Option<WheelPart> {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    let painter = ui.painter_at(rect);

    let top_space = if view.show_arc { 84.0 } else { 20.0 };
    let arc_k = if view.show_arc { 1.62 } else { 1.0 };
    let r = ((rect.height() - top_space - 10.0) / (arc_k + 1.05)).min(rect.width() / 3.4).max(40.0);
    let c = pos2(rect.center().x, rect.top() + top_space + r * arc_k);

    let angle = view.angle.unwrap_or(0.0);
    if view.show_arc {
        let text = match view.angle {
            Some(a) => format!("{:.0}", a),
            None => "–".into(),
        };
        painter.text(pos2(c.x, rect.top() + 8.0), Align2::CENTER_TOP, text, FontId::proportional(40.0), TEXT);
        // 900° of travel spread over ±70° of arc; the operating range lights up.
        let half_vis = 70f32.to_radians();
        let ticks = 57;
        for i in 0..ticks {
            let t = i as f32 / (ticks - 1) as f32 * 2.0 - 1.0;
            let real = t * 450.0;
            let a = t * half_vis;
            let dir = vec2(a.sin(), -a.cos());
            let col = if real.abs() <= view.range / 2.0 { BLUE } else { DIM };
            painter.line_segment([c + dir * r * 1.45, c + dir * r * 1.62], Stroke::new(2.5_f32, col));
        }
        // Marker for the live position.
        let ta = (angle / 450.0).clamp(-1.0, 1.0) * half_vis;
        let dir = vec2(ta.sin(), -ta.cos());
        let tip = c + dir * r * 1.66;
        let side = vec2(dir.y, -dir.x) * 7.0;
        painter.add(Shape::convex_polygon(vec![tip + dir * 12.0 + side, tip + dir * 12.0 - side, tip], TEXT, Stroke::NONE));
        painter.text(c - vec2(0.0, r * 1.30), Align2::CENTER_CENTER, format!("{:.0}", view.range), FontId::proportional(18.0), BLUE);
        painter.text(c - vec2(0.0, r * 1.17), Align2::CENTER_CENTER, "OPERATING RANGE (ANGLE)", FontId::proportional(13.0), BLUE);
    }

    let th = angle.to_radians();
    let (s, co) = th.sin_cos();
    let at = |x: f32, y: f32| c + vec2((x * co - y * s) * r, (x * s + y * co) * r);
    let poly = |pts: &[(f32, f32)], fill: Color32, stroke: Stroke| {
        painter.add(Shape::convex_polygon(pts.iter().map(|p| at(p.0, p.1)).collect(), fill, stroke));
    };

    // Rim, spokes and centre plate.
    painter.circle_stroke(c, r * 0.92, Stroke::new(r * 0.16, Color32::from_rgb(0x1C, 0x1D, 0x20)));
    painter.circle_stroke(c, r * 1.0, Stroke::new(1.0_f32, BODY_EDGE));
    painter.circle_stroke(c, r * 0.84, Stroke::new(1.0_f32, BODY_EDGE));
    poly(&[(-0.03, -1.0), (0.03, -1.0), (0.03, -0.84), (-0.03, -0.84)], BLUE, Stroke::NONE);
    poly(&[(-0.86, -0.10), (-0.30, -0.16), (-0.30, 0.24), (-0.86, 0.14)], BODY, Stroke::new(1.0_f32, BODY_EDGE));
    poly(&[(0.30, -0.16), (0.86, -0.10), (0.86, 0.14), (0.30, 0.24)], BODY, Stroke::new(1.0_f32, BODY_EDGE));
    poly(&[(-0.16, 0.30), (0.16, 0.30), (0.22, 0.86), (-0.22, 0.86)], BODY, Stroke::new(1.0_f32, BODY_EDGE));
    poly(
        &[(-0.66, -0.20), (0.66, -0.20), (0.70, 0.22), (0.34, 0.52), (-0.34, 0.52), (-0.70, 0.22)],
        Color32::from_rgb(0x1E, 0x20, 0x24),
        Stroke::new(1.0_f32, BODY_EDGE),
    );
    painter.circle(at(0.0, 0.02), r * 0.23, Color32::from_rgb(0x15, 0x16, 0x18), Stroke::new(2.0_f32, BODY_EDGE));
    painter.text(at(0.0, 0.02), Align2::CENTER_CENTER, "G", FontId::proportional(r * 0.2), TEXT);
    painter.circle_stroke(at(0.30, 0.385), r * 0.10, Stroke::new(r * 0.03, PART));

    // D-pad (hat): arms light up in the pressed direction(s).
    let hat_dirs: &[u8] = match view.hat {
        Some(0) => &[0],
        Some(1) => &[0, 2],
        Some(2) => &[2],
        Some(3) => &[2, 4],
        Some(4) => &[4],
        Some(5) => &[4, 6],
        Some(6) => &[6],
        Some(7) => &[6, 0],
        _ => &[],
    };
    let (dx, dy, w, l) = (-0.56, -0.03, 0.035, 0.10);
    for (dir, (ax, ay)) in [(0u8, (0.0, -1.0)), (2, (1.0, 0.0)), (4, (0.0, 1.0)), (6, (-1.0, 0.0))] {
        let (px, py) = (-ay, ax);
        let pts = [
            (dx + px * w, dy + py * w),
            (dx + px * w + ax * l, dy + py * w + ay * l),
            (dx - px * w + ax * l, dy - py * w + ay * l),
            (dx - px * w, dy - py * w),
        ];
        poly(&pts, part_fill(hat_dirs.contains(&dir), false), Stroke::new(1.0_f32, BODY_EDGE));
    }

    let mut clicked = None;
    for (part, pressed, selected) in view.parts {
        let (x, y, pr) = wheel_part_pos(*part);
        let center = at(x, y);
        let radius = pr * r;
        let hit = Rect::from_center_size(center, Vec2::splat(radius * 2.4));
        let resp = ui.interact(hit, ui.id().with(("wheel", *part)), Sense::click());
        let stroke = if *selected { Stroke::new(2.5_f32, ACCENT) } else { Stroke::new(1.0_f32, BODY_EDGE) };
        painter.circle(center, radius, part_fill(*pressed, resp.hovered()), stroke);
        let (glyph, col) = part_glyph(*part);
        let size = if glyph.len() > 2 { radius * 0.8 } else { radius * 1.2 };
        painter.text(center, Align2::CENTER_CENTER, glyph, FontId::proportional(size.max(9.0)), col);
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

// ------------------------------------------------------------------ pedals

/// Pedal ladders and plates. Values are 0..1 (fully pressed = 1), `None` if unknown.
pub fn pedals(ui: &mut Ui, pedals: [(&str, Option<f32>); 3]) {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());
    let painter = ui.painter_at(rect);
    let ladder_h = (rect.height() * 0.36).clamp(120.0, 320.0);
    let ticks = 24;
    let top = rect.top() + 30.0;
    let plate = vec2(rect.width() * 0.13, rect.height() * 0.22).min(vec2(110.0, 150.0));
    let base_top = top + ladder_h + 70.0 + plate.y * 0.65;
    let base = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.08, base_top),
        pos2(rect.right() - rect.width() * 0.08, (base_top + plate.y * 1.2).min(rect.bottom() - 10.0)),
    );
    painter.rect_filled(base, 24.0, Color32::from_rgb(0x18, 0x19, 0x1C));

    for (i, (name, value)) in pedals.iter().enumerate() {
        let x = rect.left() + rect.width() * (0.2 + 0.3 * i as f32);
        let v = value.unwrap_or(0.0).clamp(0.0, 1.0);
        let lit = (v * ticks as f32).round() as usize;
        for t in 0..ticks {
            let y = top + ladder_h - ladder_h * (t as f32 + 0.5) / ticks as f32;
            let col = if t < lit { BLUE } else { DIM };
            painter.line_segment([pos2(x - 28.0, y), pos2(x + 28.0, y)], Stroke::new(2.0_f32, col));
        }
        painter.text(pos2(x, top + ladder_h + 12.0), Align2::CENTER_TOP, *name, FontId::proportional(15.0), TEXT);
        let pct = match value {
            Some(_) => format!("{:.0}%", v * 100.0),
            None => "–".into(),
        };
        painter.text(pos2(x, top + ladder_h + 32.0), Align2::CENTER_TOP, pct, FontId::proportional(13.0), TEXT_DIM);

        // Plate dips and darkens as the pedal is pressed.
        let size = if i == 2 { vec2(plate.x * 0.8, plate.y * 1.1) } else { plate };
        let center = pos2(x, top + ladder_h + 70.0 + size.y / 2.0 + v * 14.0);
        let pr = Rect::from_center_size(center, size);
        let shade = (0xB8 as f32 * (1.0 - 0.35 * v)) as u8;
        painter.rect(pr, 12.0, Color32::from_gray(shade), Stroke::new(1.0_f32, Color32::from_gray(0x60)));
        for (hx, hy) in [(-0.3, -0.32), (0.3, -0.32), (-0.3, 0.0), (0.3, 0.0), (-0.3, 0.32), (0.3, 0.32)] {
            painter.circle_filled(center + vec2(hx * size.x, hy * size.y), size.x * 0.09, Color32::from_gray(0x22));
        }
    }
}
