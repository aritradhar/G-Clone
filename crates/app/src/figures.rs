//! Vector drawings of the devices with clickable, live-highlighted parts.
//!
//! Every figure is laid out on a fixed-size canvas centred in the available space, so it
//! keeps its size and proportions when the window is resized.

use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use crate::photos::Photo;
use gclone_core::config::{MousePart, WheelPart};

pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0xC8, 0xD7);
const BLUE: Color32 = Color32::from_rgb(0x1E, 0x90, 0xE6);
const ENDURANCE_GREEN: Color32 = Color32::from_rgb(0x4C, 0xD9, 0x64);
const BODY: Color32 = Color32::from_rgb(0x26, 0x28, 0x2D);
const BODY_EDGE: Color32 = Color32::from_rgb(0x44, 0x48, 0x50);
const PART: Color32 = Color32::from_rgb(0x34, 0x37, 0x3E);
const PART_HOVER: Color32 = Color32::from_rgb(0x48, 0x4C, 0x55);
const DIM: Color32 = Color32::from_rgb(0x3A, 0x3C, 0x42);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);
const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0x9C, 0xA2);

pub const MOUSE_CANVAS: Vec2 = vec2(660.0, 520.0);
pub const WHEEL_CANVAS: Vec2 = vec2(600.0, 560.0);
pub const PEDAL_CANVAS: Vec2 = vec2(600.0, 580.0);
pub const LIGHT_CANVAS: Vec2 = vec2(420.0, 420.0);

fn edge(width: f32) -> Stroke {
    Stroke::new(width, BODY_EDGE)
}

fn part_fill(pressed: bool, hovered: bool) -> Color32 {
    if pressed {
        ACCENT.linear_multiply(0.75)
    } else if hovered {
        PART_HOVER
    } else {
        PART
    }
}

/// Takes the remaining space and returns a painter for it plus the top-left of a centred
/// canvas of `size`. When space is short the canvas sticks to the top-left instead of
/// being clipped on both sides.
pub fn canvas(ui: &mut Ui, size: Vec2) -> (Painter, Pos2) {
    let avail = ui.available_rect_before_wrap();
    ui.allocate_rect(avail, Sense::hover());
    let c = Rect::from_center_size(avail.center(), size);
    (ui.painter_at(avail), pos2(c.min.x.max(avail.min.x), c.min.y.max(avail.min.y)))
}

// ------------------------------------------------------------------ mouse

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseView {
    Top,
    Side,
}

pub struct Callout {
    pub part: MousePart,
    pub assignment: String,
    pub pressed: bool,
    pub selected: bool,
}

enum Geo {
    Quad([(f32, f32); 4]),
    /// Centre and half-size.
    Pill((f32, f32), (f32, f32)),
}

#[derive(Clone, Copy)]
enum LabelAlign {
    Left,
    Right,
    Center,
}

/// Label column: underline from x0 to x0 + LABEL_W at height y (canvas pixels).
struct Label {
    x0: f32,
    y: f32,
    align: LabelAlign,
}

pub const LABEL_W: f32 = 150.0;

/// Top view: body box at (171, 40) sized 317 x 440 on the canvas.
const TOP_BOX: (f32, f32, f32, f32) = (171.0, 40.0, 317.0, 440.0);
/// Side view (left side, front of the mouse to the left): body box 560 x 240.
const SIDE_BOX: (f32, f32, f32, f32) = (50.0, 150.0, 560.0, 240.0);

fn mouse_layout(view: MouseView, part: MousePart) -> Option<(Geo, Label)> {
    use MousePart::*;
    let left = |y| Label { x0: 0.0, y, align: LabelAlign::Left };
    let right = |y| Label { x0: MOUSE_CANVAS.x - LABEL_W, y, align: LabelAlign::Right };
    match view {
        MouseView::Top => Some(match part {
            G10 => (Geo::Pill((0.245, 0.13), (0.022, 0.04)), left(80.0)),
            G11 => (Geo::Pill((0.225, 0.23), (0.022, 0.04)), left(160.0)),
            Left => (Geo::Quad([(0.30, 0.06), (0.47, 0.03), (0.47, 0.37), (0.27, 0.39)]), left(250.0)),
            Right => (Geo::Quad([(0.53, 0.03), (0.70, 0.06), (0.79, 0.35), (0.53, 0.37)]), right(110.0)),
            Middle => (Geo::Pill((0.50, 0.17), (0.035, 0.07)), right(200.0)),
            _ => return None,
        }),
        MouseView::Side => {
            let top = |i: f32| Label { x0: i * 127.0, y: 105.0, align: LabelAlign::Center };
            let bottom = |i: f32| Label { x0: 125.0 + i * 140.0, y: 455.0, align: LabelAlign::Center };
            let (w, h) = (0.045, 0.06);
            Some(match part {
                G10 => (Geo::Pill((0.13, 0.22), (0.02, 0.045)), top(0.0)),
                G11 => (Geo::Pill((0.21, 0.13), (0.02, 0.045)), top(1.0)),
                // Upper thumb row is G7–G9, lower row G4–G6.
                G7 => (Geo::Pill((0.32, 0.40), (w, h)), top(2.0)),
                G8 => (Geo::Pill((0.44, 0.40), (w, h)), top(3.0)),
                G9 => (Geo::Pill((0.56, 0.40), (w, h)), top(4.0)),
                G4 => (Geo::Pill((0.33, 0.60), (w, h)), bottom(0.0)),
                G5 => (Geo::Pill((0.45, 0.60), (w, h)), bottom(1.0)),
                G6 => (Geo::Pill((0.57, 0.60), (w, h)), bottom(2.0)),
                _ => return None,
            })
        }
    }
}

const TOP_BODY: [(f32, f32); 12] = [
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

const SIDE_BODY: [(f32, f32); 11] = [
    (0.00, 0.62),
    (0.04, 0.30),
    (0.16, 0.10),
    (0.40, 0.00),
    (0.68, 0.04),
    (0.88, 0.20),
    (1.00, 0.50),
    (0.97, 0.85),
    (0.90, 1.00),
    (0.06, 1.00),
    (0.01, 0.85),
];

/// Draws the G602 in the chosen view with callouts; returns the part the user clicked.
/// `mode` is the hardware switch: `Some(true)` = Endurance.
pub fn mouse(ui: &mut Ui, view: MouseView, parts: &[Callout], mode: Option<bool>) -> Option<MousePart> {
    let (painter, o) = canvas(ui, MOUSE_CANVAS);
    let (bx, by, bw, bh) = if view == MouseView::Top { TOP_BOX } else { SIDE_BOX };
    let map = |(u, v): (f32, f32)| o + vec2(bx + u * bw, by + v * bh);

    match view {
        MouseView::Top => {
            painter.add(Shape::convex_polygon(TOP_BODY.map(map).to_vec(), BODY, edge(1.5)));
            // Brushed-metal centre strip and logo.
            let strip = [(0.47, 0.37), (0.53, 0.37), (0.57, 0.90), (0.43, 0.90)].map(map).to_vec();
            painter.add(Shape::convex_polygon(strip, Color32::from_rgb(0x55, 0x59, 0x60), Stroke::NONE));
            painter.text(map((0.50, 0.80)), Align2::CENTER_CENTER, "G", FontId::proportional(22.0), TEXT_DIM);

            // DPI indicator lights (3 x 4 grid) and the Performance / Endurance slider.
            // Like the real mouse, the lights glow green in Endurance mode.
            let led = if mode == Some(true) { ENDURANCE_GREEN } else { BLUE };
            for row in 0..3 {
                for col in 0..4 {
                    let p = map((0.4775 + col as f32 * 0.015, 0.40 + row as f32 * 0.016));
                    painter.circle_filled(p, 2.2, led.linear_multiply(0.85));
                }
            }
            let switch = Rect::from_center_size(map((0.50, 0.50)), vec2(18.0, 26.0));
            painter.rect(switch, 4.0, Color32::from_rgb(0x2A, 0x2C, 0x31), edge(1.0));
            let knob_y = if mode == Some(true) { 0.25 } else { -0.25 };
            painter.rect_filled(
                Rect::from_center_size(switch.center() + vec2(0.0, knob_y * switch.height()), vec2(12.0, 10.0)),
                2.0,
                TEXT_DIM,
            );
            let (x0, y) = (o.x + MOUSE_CANVAS.x - LABEL_W, o.y + 300.0);
            painter.line_segment([pos2(x0, y), pos2(x0 + LABEL_W, y)], Stroke::new(1.0_f32, DIM));
            painter.line_segment([pos2(x0, y), switch.right_center()], Stroke::new(1.0_f32, DIM));
            let mode_text = match mode {
                Some(true) => "Endurance (125 Hz)",
                Some(false) => "Performance",
                None => "Unknown",
            };
            painter.text(pos2(x0 + LABEL_W, y - 23.0), Align2::RIGHT_BOTTOM, "Mode switch", FontId::proportional(12.0), TEXT_DIM);
            let mode_col = if mode == Some(true) { ENDURANCE_GREEN } else { TEXT };
            painter.text(pos2(x0 + LABEL_W, y - 4.0), Align2::RIGHT_BOTTOM, mode_text, FontId::proportional(15.0), mode_col);
        }
        MouseView::Side => {
            painter.add(Shape::convex_polygon(SIDE_BODY.map(map).to_vec(), BODY, edge(1.5)));
            // Thumb panel, top-shell seam and the scroll wheel peeking over the top.
            let panel = [(0.24, 0.28), (0.66, 0.28), (0.68, 0.72), (0.22, 0.72)].map(map).to_vec();
            painter.add(Shape::convex_polygon(panel, Color32::from_rgb(0x1C, 0x1D, 0x21), edge(1.0)));
            painter.line_segment([map((0.05, 0.28)), map((0.86, 0.22))], edge(1.0));
            painter.circle(map((0.30, 0.02)), 14.0, PART, edge(1.0));
            painter.text(map((0.82, 0.62)), Align2::CENTER_CENTER, "G602", FontId::proportional(14.0), TEXT_DIM);
        }
    }

    let mut clicked = None;
    for c in parts {
        let Some((geo, label)) = mouse_layout(view, c.part) else { continue };
        let id = ui.id().with(("mouse", c.part));
        let hit = match &geo {
            Geo::Quad(q) => Rect::from_points(&q.map(map)),
            Geo::Pill(c, s) => Rect::from_center_size(map(*c), vec2(s.0 * 2.0 * bw, s.1 * 2.0 * bh)),
        };
        let resp = ui.interact(hit, id, Sense::click());
        let fill = part_fill(c.pressed, resp.hovered());
        match &geo {
            Geo::Quad(q) => {
                painter.add(Shape::convex_polygon(q.map(map).to_vec(), fill, edge(1.0)));
            }
            Geo::Pill(..) => {
                painter.rect(hit, hit.width().min(hit.height()) / 2.0, fill, edge(1.0));
            }
        }
        let dot = hit.center();

        // Callout: title + assignment above an underline, then a line to the part's dot.
        let (x0, x1, y) = (o.x + label.x0, o.x + label.x0 + LABEL_W, o.y + label.y);
        let line_col = if c.selected { ACCENT } else { DIM };
        let joint = match label.align {
            LabelAlign::Left => pos2(x1, y),
            LabelAlign::Right => pos2(x0, y),
            LabelAlign::Center => pos2((x0 + x1) / 2.0, y),
        };
        painter.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.0_f32, line_col));
        painter.line_segment([joint, dot], Stroke::new(1.0_f32, line_col));
        let label_rect = Rect::from_min_max(pos2(x0, y - 40.0), pos2(x1, y + 4.0));
        let label_resp = ui.interact(label_rect, id.with("label"), Sense::click());
        let (anchor, align) = match label.align {
            LabelAlign::Left => (pos2(x0, y - 4.0), Align2::LEFT_BOTTOM),
            LabelAlign::Right => (pos2(x1, y - 4.0), Align2::RIGHT_BOTTOM),
            LabelAlign::Center => (pos2((x0 + x1) / 2.0, y - 4.0), Align2::CENTER_BOTTOM),
        };
        let title_col = if c.selected || label_resp.hovered() { ACCENT } else { TEXT_DIM };
        painter.text(anchor - vec2(0.0, 19.0), align, c.part.label(), FontId::proportional(12.0), title_col);
        painter.text(anchor, align, truncate(&c.assignment, 20), FontId::proportional(15.0), TEXT);

        let dot_fill = if c.pressed { ACCENT } else { Color32::WHITE };
        let ring = if c.selected { ACCENT } else { BODY };
        painter.circle(dot, if c.selected { 7.0 } else { 6.0 }, dot_fill, Stroke::new(2.0_f32, ring));

        if resp.clicked() || label_resp.clicked() {
            clicked = Some(c.part);
        }
        if resp.hovered() || label_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    if view == MouseView::Top {
        painter.text(
            o + vec2(MOUSE_CANVAS.x / 2.0, MOUSE_CANVAS.y - 4.0),
            Align2::CENTER_BOTTOM,
            "Thumb buttons G4–G9 are in the side view",
            FontId::proportional(12.0),
            TEXT_DIM,
        );
    }
    clicked
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
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

/// Wheel radius in pixels; fixed so the figure never rescales with the window.
const WHEEL_R: f32 = 160.0;

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
        // Drawn separately (two overlapping squares); no font has a reliable glyph.
        View => ("", TEXT_DIM),
        Menu => ("☰", TEXT_DIM),
        LSB => ("LSB", TEXT_DIM),
        RSB => ("RSB", TEXT_DIM),
        Plus => ("+", TEXT),
        Minus => ("−", TEXT),
        Enter => ("OK", TEXT),
        DialLeft => ("‹", TEXT_DIM),
        DialRight => ("›", TEXT_DIM),
        Xbox => ("⊗", TEXT),
        LB => ("LB", TEXT),
        RB => ("RB", TEXT),
    }
}

/// Draws the wheel (rotated to the live angle) with optional range arc; returns a clicked part.
/// With `photo` (front, base) the G HUB photo replaces the vector wheel.
pub fn wheel(ui: &mut Ui, view: &WheelView, photo: Option<(&Photo, Option<&Photo>)>) -> Option<WheelPart> {
    let (painter, o) = canvas(ui, WHEEL_CANVAS);
    let r = WHEEL_R;
    let c = o + vec2(WHEEL_CANVAS.x / 2.0, 84.0 + r * 1.62);

    let angle = view.angle.unwrap_or(0.0);
    if view.show_arc {
        let text = match view.angle {
            Some(a) => format!("{a:.0}"),
            None => "–".into(),
        };
        painter.text(o + vec2(WHEEL_CANVAS.x / 2.0, 6.0), Align2::CENTER_TOP, text, FontId::proportional(40.0), TEXT);
        // 900° of travel spread over ±70° of arc; the operating range lights up.
        let half_vis = 70f32.to_radians();
        let ticks = 57;
        for i in 0..ticks {
            let t = i as f32 / (ticks - 1) as f32 * 2.0 - 1.0;
            let a = t * half_vis;
            let dir = vec2(a.sin(), -a.cos());
            let col = if (t * 450.0).abs() <= view.range / 2.0 { BLUE } else { DIM };
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

    if let Some((front, base)) = photo {
        // Sized so the rim stays below the "OPERATING RANGE" label.
        return crate::photo_figures::wheel(ui, &painter, front, base, c, r * 2.3, angle, view.parts, view.hat);
    }

    let (s, co) = angle.to_radians().sin_cos();
    let at = |x: f32, y: f32| c + vec2((x * co - y * s) * r, (x * s + y * co) * r);
    let poly = |pts: &[(f32, f32)], fill: Color32, stroke: Stroke| {
        painter.add(Shape::convex_polygon(pts.iter().map(|p| at(p.0, p.1)).collect(), fill, stroke));
    };

    // Rim, top marker, spokes and centre plate.
    painter.circle_stroke(c, r * 0.92, Stroke::new(r * 0.16, Color32::from_rgb(0x1C, 0x1D, 0x20)));
    painter.circle_stroke(c, r, edge(1.0));
    painter.circle_stroke(c, r * 0.84, edge(1.0));
    poly(&[(-0.03, -1.0), (0.03, -1.0), (0.03, -0.84), (-0.03, -0.84)], BLUE, Stroke::NONE);
    poly(&[(-0.86, -0.10), (-0.30, -0.16), (-0.30, 0.24), (-0.86, 0.14)], BODY, edge(1.0));
    poly(&[(0.30, -0.16), (0.86, -0.10), (0.86, 0.14), (0.30, 0.24)], BODY, edge(1.0));
    poly(&[(-0.16, 0.30), (0.16, 0.30), (0.22, 0.86), (-0.22, 0.86)], BODY, edge(1.0));
    poly(
        &[(-0.66, -0.20), (0.66, -0.20), (0.70, 0.22), (0.34, 0.52), (-0.34, 0.52), (-0.70, 0.22)],
        Color32::from_rgb(0x1E, 0x20, 0x24),
        edge(1.0),
    );
    painter.circle(at(0.0, 0.02), r * 0.23, Color32::from_rgb(0x15, 0x16, 0x18), edge(2.0));
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
        poly(&pts, part_fill(hat_dirs.contains(&dir), false), edge(1.0));
    }

    let mut clicked = None;
    for (part, pressed, selected) in view.parts {
        let (x, y, pr) = wheel_part_pos(*part);
        let center = at(x, y);
        let radius = pr * r;
        let hit = Rect::from_center_size(center, Vec2::splat(radius * 2.4));
        let resp = ui.interact(hit, ui.id().with(("wheel", *part)), Sense::click());
        let stroke = if *selected { Stroke::new(2.5_f32, ACCENT) } else { edge(1.0) };
        painter.circle(center, radius, part_fill(*pressed, resp.hovered()), stroke);
        let (glyph, col) = part_glyph(*part);
        if *part == WheelPart::View {
            let s = radius * 0.38;
            for off in [vec2(-s * 0.35, -s * 0.35), vec2(s * 0.35, s * 0.35)] {
                painter.rect_stroke(Rect::from_center_size(center + off, Vec2::splat(s * 1.4)), 1.0, Stroke::new(1.2_f32, col));
            }
        } else {
            let size = if glyph.chars().count() > 1 { radius * 0.75 } else { radius * 1.2 };
            painter.text(center, Align2::CENTER_CENTER, glyph, FontId::proportional(size.max(9.0)), col);
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

// ------------------------------------------------------------------ pedals

/// Pedal ladders and plates. Values are 0..1 (fully pressed = 1), `None` if unknown.
pub fn pedals(ui: &mut Ui, pedals: [(&str, Option<f32>); 3]) {
    let (painter, o) = canvas(ui, PEDAL_CANVAS);
    let (ladder_top, ladder_h, ticks) = (20.0, 200.0, 24);
    let plate = vec2(92.0, 130.0);
    let plate_top = ladder_top + ladder_h + 76.0;
    let base = Rect::from_min_size(o + vec2(30.0, plate_top + plate.y * 0.6), vec2(PEDAL_CANVAS.x - 60.0, 150.0));
    painter.rect_filled(base, 24.0, Color32::from_rgb(0x18, 0x19, 0x1C));

    for (i, (name, value)) in pedals.iter().enumerate() {
        let x = o.x + 100.0 + 200.0 * i as f32;
        let v = value.unwrap_or(0.0).clamp(0.0, 1.0);
        let lit = (v * ticks as f32).round() as usize;
        for t in 0..ticks {
            let y = o.y + ladder_top + ladder_h - ladder_h * (t as f32 + 0.5) / ticks as f32;
            let col = if t < lit { BLUE } else { DIM };
            painter.line_segment([pos2(x - 28.0, y), pos2(x + 28.0, y)], Stroke::new(2.0_f32, col));
        }
        let below = o.y + ladder_top + ladder_h;
        painter.text(pos2(x, below + 12.0), Align2::CENTER_TOP, *name, FontId::proportional(15.0), TEXT);
        let pct = match value {
            Some(_) => format!("{:.0}%", v * 100.0),
            None => "–".into(),
        };
        painter.text(pos2(x, below + 32.0), Align2::CENTER_TOP, pct, FontId::proportional(13.0), TEXT_DIM);

        // Plate dips and darkens as the pedal is pressed.
        let size = if i == 2 { vec2(plate.x * 0.8, plate.y * 1.1) } else { plate };
        let center = pos2(x, o.y + plate_top + size.y / 2.0 + v * 14.0);
        let pr = Rect::from_center_size(center, size);
        let shade = (0xB8 as f32 * (1.0 - 0.35 * v)) as u8;
        painter.rect(pr, 12.0, Color32::from_gray(shade), Stroke::new(1.0_f32, Color32::from_gray(0x60)));
        for (hx, hy) in [(-0.3, -0.32), (0.3, -0.32), (-0.3, 0.0), (0.3, 0.0), (-0.3, 0.32), (0.3, 0.32)] {
            painter.circle_filled(center + vec2(hx * size.x, hy * size.y), size.x * 0.09, Color32::from_gray(0x22));
        }
    }
}

// ------------------------------------------------------------------ light

/// Glowing panel whose colour follows the light's temperature and brightness.
pub fn light(ui: &mut Ui, on: bool, lumens: u16, kelvin: u16) {
    let (painter, o) = canvas(ui, LIGHT_CANVAS);
    let panel = Rect::from_center_size(o + vec2(LIGHT_CANVAS.x / 2.0, 180.0), vec2(240.0, 240.0));
    // Rough blackbody tint between 2700 K (amber) and 6500 K (cool white).
    let t = ((kelvin as f32 - 2700.0) / 3800.0).clamp(0.0, 1.0);
    let tint = Color32::from_rgb(255, (180.0 + 70.0 * t) as u8, (110.0 + 140.0 * t) as u8);
    if on {
        let k = 0.35 + 0.65 * (lumens as f32 / 250.0);
        for i in (1..=6).rev() {
            let glow = panel.expand(i as f32 * 14.0);
            painter.rect_filled(glow, 30.0 + i as f32 * 8.0, tint.linear_multiply(0.04 * k));
        }
        painter.rect_filled(panel, 24.0, tint.linear_multiply(k));
    } else {
        painter.rect(panel, 24.0, Color32::from_rgb(0x1A, 0x1B, 0x1E), Stroke::new(1.0_f32, DIM));
    }
    let label = if on { format!("{lumens} lm · {kelvin} K") } else { "Off".into() };
    painter.text(panel.center_bottom() + vec2(0.0, 30.0), Align2::CENTER_TOP, label, FontId::proportional(18.0), TEXT);
}
