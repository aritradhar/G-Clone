//! egui front end in a G HUB-like layout: device list on the left, a settings card, and a
//! live device figure. The UI edits a copy of [`Config`]; changes go to the engine at once
//! and are saved to disk shortly after.

use crate::figures::{self, Callout, MouseView, WheelView, ACCENT};
use crate::{tray, AppShared};
use eframe::egui::{self, Color32, Id, RichText, Ui};
use gclone_core::config::*;
use gclone_core::devices::{joystick::USAGE_HAT, litra, wheel};
use gclone_core::engine::{JoyLive, Status, UiEvent};
use gclone_core::keys;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
const SIDEBAR: Color32 = Color32::from_rgb(0x0E, 0x0F, 0x11);
const CARD: Color32 = Color32::from_rgb(0x20, 0x22, 0x26);
const OK: Color32 = Color32::from_rgb(0x4C, 0xD9, 0x64);
const OFF: Color32 = Color32::from_rgb(0x55, 0x57, 0x5C);
const WARN: Color32 = Color32::from_rgb(0xE0, 0x80, 0x60);
const SAVE_DELAY: Duration = Duration::from_millis(600);
/// Minimum time between frames (~60 fps). With vsync off, bursts of pointer events would
/// otherwise repaint at the input rate.
const FRAME_BUDGET: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Mouse,
    Light,
    Wheel,
    Profiles,
    Macros,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MouseTab {
    Assignments,
    Sensitivity,
    ReportRate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WheelTab {
    Steering,
    Pedals,
    Buttons,
}

/// What the next captured button press is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Capture {
    Row(usize),
    Mouse(MousePart),
    Wheel(WheelPart),
    /// Key combination for the keystroke assignment of this trigger.
    Combo(Trigger),
}

pub struct App {
    shared: Arc<AppShared>,
    cfg: Config,
    status: Status,
    joy: Option<JoyLive>,
    pressed: HashSet<Trigger>,
    page: Page,
    mouse_tab: MouseTab,
    wheel_tab: WheelTab,
    /// Profile shown in the editor (not necessarily the active one).
    edit: usize,
    mouse_part: MousePart,
    wheel_part: WheelPart,
    /// A wheel button picked from the full list (overrides `wheel_part`).
    joy_button: Option<u16>,
    selected_macro: usize,
    capture: Option<Capture>,
    capture_note: Option<String>,
    recording: Option<usize>,
    new_app: String,
    dirty_since: Option<Instant>,
    autostart: bool,
    autostart_error: Option<String>,
    logo: egui::TextureHandle,
    last_frame: Instant,
    mouse_view: figures::MouseView,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, shared: Arc<AppShared>) -> Self {
        style(&cc.egui_ctx);
        if let Ok(RawWindowHandle::Win32(h)) = cc.window_handle().map(|h| h.as_raw()) {
            tray::set_window(h.hwnd.get());
        }
        stop_raw_input();
        *shared.ctx.lock().unwrap() = Some(cc.egui_ctx.clone());
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::icon::rgba(64)),
            egui::TextureOptions::LINEAR,
        );
        let cfg = shared.cfg.lock().unwrap().clone();
        Self {
            shared,
            cfg,
            status: Status::default(),
            joy: None,
            pressed: HashSet::new(),
            page: Page::Mouse,
            mouse_tab: MouseTab::Assignments,
            wheel_tab: WheelTab::Steering,
            edit: 0,
            mouse_part: MousePart::G4,
            wheel_part: WheelPart::A,
            joy_button: None,
            selected_macro: 0,
            capture: None,
            capture_note: None,
            recording: None,
            new_app: String::new(),
            dirty_since: None,
            autostart: crate::autostart::is_enabled(),
            autostart_error: None,
            logo,
            last_frame: Instant::now(),
            mouse_view: figures::MouseView::Top,
        }
    }

    fn handle_engine_events(&mut self) {
        for ev in self.shared.engine.poll_events() {
            match ev {
                UiEvent::Captured(trigger) => match self.capture.take() {
                    Some(Capture::Row(i)) => {
                        if let Some(b) = self.cfg.profiles[self.edit].bindings.as_mut().and_then(|v| v.get_mut(i)) {
                            b.trigger = trigger;
                        }
                    }
                    Some(Capture::Mouse(part)) => {
                        if matches!(trigger, Trigger::Joy(_)) {
                            self.capture_note = Some("That was a wheel button, not the mouse.".into());
                        } else {
                            self.cfg.hardware.set_trigger(part, trigger);
                            self.capture_note = None;
                        }
                    }
                    Some(Capture::Wheel(part)) => match trigger {
                        Trigger::Joy(n) => {
                            self.cfg.hardware.set_button(part, n);
                            self.capture_note = None;
                        }
                        _ => self.capture_note = Some("Press the button on the wheel.".into()),
                    },
                    Some(Capture::Combo(_)) | None => {}
                },
                UiEvent::CapturedCombo { vk, mods } => {
                    if let Some(Capture::Combo(trigger)) = self.capture.take() {
                        let list = self.cfg.profiles[self.edit].bindings.get_or_insert_with(Vec::new);
                        match list.iter_mut().find(|b| b.trigger == trigger) {
                            Some(b) => b.action = Action::Key { vk, mods },
                            None => list.push(Binding { trigger, action: Action::Key { vk, mods } }),
                        }
                    }
                }
                UiEvent::Recorded(step) => {
                    if let Some(m) = self.recording.and_then(|i| self.cfg.macros.get_mut(i)) {
                        m.steps.push(step);
                    }
                }
            }
        }
    }

    fn start_capture(&mut self, c: Capture) {
        self.capture = Some(c);
        self.capture_note = None;
        self.shared.engine.capture_trigger();
    }

    fn cancel_capture(&mut self) {
        self.capture = None;
        self.shared.engine.stop_capture();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let since = self.last_frame.elapsed();
        if since < FRAME_BUDGET {
            std::thread::sleep(FRAME_BUDGET - since);
        }
        self.last_frame = Instant::now();
        if std::env::var_os("GCLONE_TRACE_REPAINT").is_some() {
            let causes: Vec<String> = ctx.repaint_causes().iter().map(|c| c.to_string().replace('\n', " ")).collect();
            let events: Vec<String> = ctx.input(|i| {
                i.raw.events.iter().map(|e| format!("{e:?}").chars().take(28).collect()).collect()
            });
            log::info!("frame {} causes {} events {:?}", ctx.cumulative_pass_nr(), causes.join(" | "), events);
        }
        let engine = &self.shared.engine;
        engine.set_live(matches!(self.page, Page::Mouse | Page::Wheel));
        self.status = engine.status();
        self.joy = engine.joystick();
        self.pressed = engine.pressed();
        // Snapshot first: captured keystrokes / Detect results edit the config too, and must
        // count as changes so they reach the engine and get saved.
        let before = self.cfg.clone();
        self.handle_engine_events();
        self.edit = self.edit.min(self.cfg.profiles.len() - 1);

        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(210.0)
            .frame(egui::Frame::none().fill(SIDEBAR).inner_margin(14.0))
            .show(ctx, |ui| self.nav(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(20.0, 14.0)))
            .show(ctx, |ui| {
                self.header(ui);
                ui.add_space(10.0);
                match self.page {
                    Page::Mouse => self.mouse_page(ui),
                    Page::Light => self.light_page(ui),
                    Page::Wheel => self.wheel_page(ui),
                    Page::Profiles => card_page(ui, |ui| self.profiles_page(ui)),
                    Page::Macros => card_page(ui, |ui| self.macros_page(ui)),
                    Page::Settings => card_page(ui, |ui| self.settings_page(ui)),
                }
            });

        if self.cfg != before {
            self.shared.engine.set_config(self.cfg.clone());
            *self.shared.cfg.lock().unwrap() = self.cfg.clone();
            self.dirty_since = Some(Instant::now());
        }
        if let Some(t) = self.dirty_since {
            if t.elapsed() >= SAVE_DELAY {
                self.shared.save();
                self.dirty_since = None;
            } else {
                ctx.request_repaint_after(SAVE_DELAY - t.elapsed());
            }
        }
    }
}

// ---------------------------------------------------------------- chrome

impl App {
    fn nav(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(egui::load::SizedTexture::new(self.logo.id(), [40.0, 40.0])));
            ui.label(RichText::new("G-CLONE").size(20.0).strong().color(Color32::WHITE));
        });
        ui.add_space(24.0);
        let s = &self.status;
        let items = [
            (Page::Mouse, "G602 Mouse", Some(s.mouse.is_some())),
            (Page::Wheel, "G923 Wheel", Some(s.wheel.is_some())),
            (Page::Light, "Litra Glow", Some(s.light.is_some())),
            (Page::Profiles, "Profiles", None),
            (Page::Macros, "Macros", None),
            (Page::Settings, "Settings", None),
        ];
        for (i, (page, label, connected)) in items.into_iter().enumerate() {
            if i == 3 {
                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                // Drawn rather than a "●" glyph, which egui's bundled fonts don't have.
                let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                if let Some(c) = connected {
                    ui.painter().circle_filled(dot.center(), 4.5, if c { OK } else { OFF });
                }
                if ui.selectable_label(self.page == page, RichText::new(label).size(15.0)).clicked() {
                    self.page = page;
                }
            });
            ui.add_space(4.0);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            if let Some(e) = &self.status.last_error {
                ui.label(RichText::new(e).small().color(WARN));
            }
            if let Some((pct, charging)) = self.status.mouse.as_ref().and_then(|m| m.battery) {
                ui.label(RichText::new(format!("Mouse battery {pct}%{}", if charging { " (charging)" } else { "" })).weak());
            }
        });
    }

    fn header(&mut self, ui: &mut Ui) {
        let title = match self.page {
            Page::Mouse => "G602",
            Page::Wheel => "G923",
            Page::Light => "LITRA GLOW",
            Page::Profiles => "PROFILES",
            Page::Macros => "MACROS",
            Page::Settings => "SETTINGS",
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(34.0).strong().color(Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("edit_profile")
                    .width(220.0)
                    .selected_text(RichText::new(format!("Profile: {}", self.cfg.profiles[self.edit].name)).size(15.0))
                    .show_ui(ui, |ui| {
                        for (i, p) in self.cfg.profiles.iter().enumerate() {
                            ui.selectable_value(&mut self.edit, i, &p.name);
                        }
                    });
                let active = self.status.active_profile.min(self.cfg.profiles.len() - 1);
                let mut note = format!("Active now: {}", self.cfg.profiles[active].name);
                if !self.status.foreground.is_empty() {
                    note += &format!(" ({})", self.status.foreground);
                }
                ui.label(RichText::new(note).weak());
            });
        });
    }

    /// Settings card on the left, live figure on the right.
    fn device_layout(&mut self, ui: &mut Ui, card_fn: impl FnOnce(&mut Self, &mut Ui), figure_fn: impl FnOnce(&mut Self, &mut Ui)) {
        egui::SidePanel::left(ui.id().with("card"))
            .resizable(false)
            .exact_width(390.0)
            .frame(egui::Frame::none().fill(CARD).rounding(14.0).inner_margin(20.0))
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| card_fn(self, ui));
            });
        egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(10.0)).show_inside(ui, |ui| figure_fn(self, ui));
    }
}

fn card_page(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    egui::Frame::none().fill(CARD).rounding(14.0).inner_margin(22.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, body);
    });
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).size(17.0).color(Color32::WHITE));
    ui.add_space(4.0);
}

fn tabs<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, &str)]) {
    ui.horizontal_wrapped(|ui| {
        for (value, label) in items {
            let selected = *current == *value;
            let text = RichText::new(label.to_uppercase()).size(15.0);
            let text = if selected { text.color(Color32::WHITE).underline() } else { text.color(OFF) };
            if ui.add(egui::Label::new(text).sense(egui::Sense::click())).clicked() {
                *current = *value;
            }
            ui.add_space(10.0);
        }
    });
    ui.add_space(8.0);
}

/// For non-default profiles a section can inherit from Default. Returns the editable
/// settings, or `None` while inheriting.
fn override_section<'a, T: Clone>(ui: &mut Ui, profile: usize, section: &'a mut Option<T>, default: &T) -> Option<&'a mut T> {
    if profile != 0 {
        ui.horizontal_wrapped(|ui| {
            if section.is_some() {
                ui.label(RichText::new("Custom for this profile").weak());
                if ui.small_button("Use Default profile").clicked() {
                    *section = None;
                }
            } else {
                ui.label(RichText::new("Uses the Default profile's settings").weak());
                if ui.small_button("Customize").clicked() {
                    *section = Some(default.clone());
                }
            }
        });
    }
    section.as_mut()
}

fn not_connected(ui: &mut Ui, what: &str) {
    ui.label(RichText::new(format!("{what} not connected — settings apply when it's plugged in.")).color(WARN));
}

fn slider_row(ui: &mut Ui, slider: egui::Slider) {
    ui.add_sized([ui.available_width(), 22.0], slider);
}

// ---------------------------------------------------------------- mouse

impl App {
    fn mouse_page(&mut self, ui: &mut Ui) {
        self.device_layout(ui, Self::mouse_card, Self::mouse_figure);
    }

    fn mouse_figure(&mut self, ui: &mut Ui) {
        let bindings = self.cfg.resolved(self.edit).bindings;
        let callouts: Vec<Callout> = MousePart::ALL
            .iter()
            .map(|&part| {
                let trigger = self.cfg.hardware.trigger_of(part);
                let assignment = match trigger {
                    None => "Not detected yet".into(),
                    Some(t) => bindings
                        .iter()
                        .find(|b| b.trigger == t)
                        .map(|b| action_summary(&b.action))
                        .unwrap_or_else(|| default_summary(t)),
                };
                Callout {
                    part,
                    assignment,
                    pressed: trigger.is_some_and(|t| self.pressed.contains(&t)),
                    selected: self.mouse_part == part && self.mouse_tab == MouseTab::Assignments,
                }
            })
            .collect();
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() / 2.0 - 90.0).max(0.0));
            for (view, label) in [(MouseView::Top, "TOP VIEW"), (MouseView::Side, "SIDE VIEW")] {
                let selected = self.mouse_view == view;
                let text = RichText::new(label).size(13.0).color(if selected { Color32::BLACK } else { Color32::WHITE });
                let mut button = egui::Button::new(text).rounding(14.0).min_size(egui::vec2(84.0, 26.0));
                if selected {
                    button = button.fill(Color32::WHITE);
                }
                if ui.add(button).clicked() {
                    self.mouse_view = view;
                }
            }
        });
        let mode = self.status.mouse.as_ref().and_then(|m| m.endurance);
        if let Some(part) = figures::mouse(ui, self.mouse_view, &callouts, mode) {
            self.mouse_part = part;
            self.mouse_tab = MouseTab::Assignments;
        }
    }

    fn mouse_card(&mut self, ui: &mut Ui) {
        tabs(
            ui,
            &mut self.mouse_tab,
            &[(MouseTab::Assignments, "Assignments"), (MouseTab::Sensitivity, "Sensitivity"), (MouseTab::ReportRate, "Report rate")],
        );
        if self.status.mouse.is_none() {
            not_connected(ui, "Mouse");
        }
        match self.mouse_tab {
            MouseTab::Assignments => self.mouse_assignments(ui),
            MouseTab::Sensitivity => self.mouse_dpi(ui),
            MouseTab::ReportRate => self.mouse_rate(ui),
        }
    }

    fn mouse_assignments(&mut self, ui: &mut Ui) {
        let part = self.mouse_part;
        ui.label(RichText::new(part.label()).size(26.0).color(Color32::WHITE));
        ui.label(RichText::new("Click a button on the mouse, or press it, to select it.").weak());
        ui.add_space(8.0);

        if part == MousePart::Left {
            ui.label("Primary click can't be reassigned, so the mouse always stays usable.");
        } else {
            let trigger = self.cfg.hardware.trigger_of(part);
            ui.horizontal_wrapped(|ui| {
                ui.label("Sends:");
                ui.label(RichText::new(trigger.map_or("unknown".into(), trigger_label)).strong());
                if self.capture == Some(Capture::Mouse(part)) {
                    ui.label(RichText::new(format!("Press {} on the mouse…", part.label())).color(ACCENT));
                    if ui.small_button("Cancel").clicked() {
                        self.cancel_capture();
                    }
                } else if part.default_trigger().is_none()
                    && ui.small_button("Detect").on_hover_text("Press the physical button to learn what it sends").clicked()
                {
                    self.start_capture(Capture::Mouse(part));
                }
            });
            if let Some(n) = &self.capture_note {
                ui.label(RichText::new(n).color(WARN));
            }
            ui.add_space(6.0);
            match trigger {
                Some(t) => {
                    section(ui, "Assignment");
                    self.assignment_ui(ui, t, Id::new(("mouse_assign", part)));
                }
                None => {
                    ui.label(RichText::new("Click Detect, then press this button once so G-Clone learns what it sends.").weak());
                }
            }
        }

        ui.add_space(14.0);
        egui::CollapsingHeader::new("All assignments (keys and buttons)").show(ui, |ui| self.binding_list(ui));
    }

    fn mouse_dpi(&mut self, ui: &mut Ui) {
        let live = self.status.mouse.clone();
        if let Some(m) = &live {
            ui.label(format!("Now {} DPI · stage {}", m.dpi, self.status.dpi_stage + 1));
        }
        let values: Vec<u16> = live
            .as_ref()
            .map(|m| m.dpi_values.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| (250..=2500).step_by(250).collect());
        let default = self.cfg.profiles[0].mouse.clone().unwrap_or_default();
        let edit = self.edit;
        let Some(m) = override_section(ui, edit, &mut self.cfg.profiles[edit].mouse, &default) else { return };

        section(ui, "DPI stages");
        ui.label(RichText::new("The selected stage is used when this profile activates. Bind DPI Up / Down / Cycle to a button to switch.").weak());
        let mut remove = None;
        for i in 0..m.dpi_stages.len() {
            ui.horizontal(|ui| {
                ui.radio_value(&mut m.default_stage, i, "");
                let mut idx = values.iter().position(|v| *v >= m.dpi_stages[i]).unwrap_or(values.len() - 1);
                let label = format!("{}", values[idx]);
                ui.add_sized([230.0, 22.0], egui::Slider::new(&mut idx, 0..=values.len() - 1).show_value(false));
                m.dpi_stages[i] = values[idx];
                ui.label(RichText::new(label).size(16.0).strong());
                if m.dpi_stages.len() > 1 && ui.small_button("✕").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            m.dpi_stages.remove(i);
            m.default_stage = m.default_stage.min(m.dpi_stages.len() - 1);
        }
        if m.dpi_stages.len() < 5 && ui.button("+ Add stage").clicked() {
            let last = *m.dpi_stages.last().unwrap_or(&values[0]);
            m.dpi_stages.push(last);
        }
    }

    fn mouse_rate(&mut self, ui: &mut Ui) {
        let rates: Vec<u8> = self
            .status
            .mouse
            .as_ref()
            .map(|m| m.report_rates.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec![2, 4, 8]);
        let default = self.cfg.profiles[0].mouse.clone().unwrap_or_default();
        let edit = self.edit;
        let Some(m) = override_section(ui, edit, &mut self.cfg.profiles[edit].mouse, &default) else { return };
        section(ui, "Report rate");
        ui.label(RichText::new("How often the mouse reports its position. Higher is smoother, lower saves battery.").weak());
        let endurance = self.status.mouse.as_ref().and_then(|s| s.endurance) == Some(true);
        for ms in &rates {
            ui.radio_value(&mut m.report_rate_ms, *ms, format!("{} Hz  ({} ms)", 1000 / *ms as u32, ms));
        }
        if endurance {
            ui.add_space(6.0);
            ui.label(
                RichText::new("The mouse's mode switch is on Endurance, which holds it at 125 Hz. Slide it to Performance to use this setting.")
                    .color(WARN),
            );
        }
    }

    /// Editor for what `trigger` does in the profile being edited.
    fn assignment_ui(&mut self, ui: &mut Ui, trigger: Trigger, id: Id) {
        let macros: Vec<String> = self.cfg.macros.iter().map(|m| m.name.clone()).collect();
        let default_list = self.cfg.profiles[0].bindings.clone().unwrap_or_default();
        let edit = self.edit;
        let p = &mut self.cfg.profiles[edit];
        if edit != 0 && p.bindings.is_none() {
            ui.label(RichText::new("This profile uses the Default profile's assignments.").weak());
            if ui.button("Customize for this profile").clicked() {
                p.bindings = Some(default_list);
            }
            return;
        }
        let recording = self.capture == Some(Capture::Combo(trigger));
        let list = p.bindings.get_or_insert_with(Vec::new);
        let mut record = false;
        match list.iter().position(|b| b.trigger == trigger) {
            Some(i) => {
                ui.horizontal_wrapped(|ui| record = action_editor(ui, id, &mut list[i].action, &macros, recording));
                if matches!(list[i].action, Action::Key { .. }) && !recording {
                    ui.label(RichText::new("Click the keystroke, then press the keys you want.").weak().small());
                }
                manual_key_editor(ui, id, &mut list[i].action);
                if ui.button("Restore default").clicked() {
                    list.remove(i);
                }
            }
            None => {
                ui.label(format!("Default: {}", default_summary(trigger)));
                let mut kind = 0;
                egui::ComboBox::from_id_salt(id.with("new"))
                    .width(200.0)
                    .selected_text("Assign…")
                    .show_ui(ui, |ui| {
                        for (k, label) in ACTION_KINDS.iter().enumerate() {
                            ui.selectable_value(&mut kind, k, *label);
                        }
                    });
                if kind != 0 {
                    list.push(Binding { trigger, action: action_for_kind(kind, &macros) });
                    // Picking "Keystroke" goes straight to pressing the combination.
                    record = kind == KIND_KEYSTROKE;
                }
            }
        }
        if record {
            self.start_combo(trigger);
        }
    }

    fn start_combo(&mut self, trigger: Trigger) {
        self.capture = Some(Capture::Combo(trigger));
        self.shared.engine.capture_combo();
    }

    fn binding_list(&mut self, ui: &mut Ui) {
        let macro_names: Vec<String> = self.cfg.macros.iter().map(|m| m.name.clone()).collect();
        let default = self.cfg.profiles[0].bindings.clone().unwrap_or_default();
        let edit = self.edit;
        let capture = self.capture;
        let mut start = None;
        let mut combo = None;
        let names = self.cfg.hardware.clone();
        let Some(list) = override_section(ui, edit, &mut self.cfg.profiles[edit].bindings, &default) else { return };
        let mut remove = None;
        for (i, b) in list.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                if capture == Some(Capture::Row(i)) {
                    ui.label(RichText::new("Press a button or key…").color(ACCENT));
                } else {
                    let label = match b.trigger {
                        Trigger::Joy(n) => format!("Wheel {}", names.wheel_button_name(n)),
                        t => trigger_label(t),
                    };
                    if ui.button(label).on_hover_text("Click, then press the button or key to change").clicked() {
                        start = Some(i);
                    }
                }
                ui.label("→");
                let recording = capture == Some(Capture::Combo(b.trigger));
                if action_editor(ui, Id::new(("row", i)), &mut b.action, &macro_names, recording) {
                    combo = Some(b.trigger);
                }
                if ui.small_button("🗑").clicked() {
                    remove = Some(i);
                }
            });
            manual_key_editor(ui, Id::new(("row", i)), &mut b.action);
            ui.separator();
        }
        if let Some(i) = remove {
            list.remove(i);
        }
        if ui.button("+ Add assignment").clicked() {
            list.push(Binding { trigger: Trigger::Mouse(MouseButton::Middle), action: Action::Disabled });
            start = Some(list.len() - 1);
        }
        if let Some(i) = start {
            self.start_capture(Capture::Row(i));
        }
        if let Some(t) = combo {
            self.start_combo(t);
        }
    }
}

// ---------------------------------------------------------------- wheel

impl App {
    fn wheel_page(&mut self, ui: &mut Ui) {
        self.device_layout(ui, Self::wheel_card, Self::wheel_figure);
    }

    fn axis_value(&self, usage: u16) -> Option<(f32, i32)> {
        let j = self.joy.as_ref()?;
        let i = j.axes.iter().position(|a| a.page == 0x01 && a.usage == usage)?;
        let a = &j.axes[i];
        let v = *j.state.values.get(i)?;
        let span = (a.max - a.min).max(1) as f32;
        Some((((v - a.min) as f32 / span).clamp(0.0, 1.0), v))
    }

    /// Pedal travel 0..1; a connected wheel that hasn't reported yet is at rest.
    fn pedal(&self, usage: u16) -> Option<f32> {
        match self.axis_value(usage) {
            Some((f, _)) => Some(if self.cfg.hardware.axes.invert_pedals { 1.0 - f } else { f }),
            None => self.joy.as_ref().map(|_| 0.0),
        }
    }

    fn wheel_range(&self) -> u16 {
        self.status.wheel.as_ref().map(|w| w.range).unwrap_or_else(|| self.cfg.resolved(self.edit).wheel.range)
    }

    fn wheel_figure(&mut self, ui: &mut Ui) {
        let axes = self.cfg.hardware.axes.clone();
        if self.wheel_tab == WheelTab::Pedals {
            figures::pedals(
                ui,
                [("Clutch", self.pedal(axes.clutch)), ("Brake", self.pedal(axes.brake)), ("Gas", self.pedal(axes.throttle))],
            );
            return;
        }
        let range = self.wheel_range() as f32;
        let angle = match self.axis_value(axes.steering) {
            Some((f, _)) => Some((f - 0.5) * range),
            None => self.joy.as_ref().map(|_| 0.0),
        };
        let hat = self.joy.as_ref().and_then(|j| {
            let i = j.axes.iter().position(|a| a.usage == USAGE_HAT)?;
            let v = *j.state.values.get(i)?;
            (0..=7).contains(&v).then_some(v as u8)
        });
        let buttons = self.joy.as_ref().map(|j| j.state.buttons.clone()).unwrap_or_default();
        let selecting = self.wheel_tab == WheelTab::Buttons;
        let parts: Vec<(WheelPart, bool, bool)> = WheelPart::ALL
            .iter()
            .map(|&p| {
                let pressed = self.cfg.hardware.button_of(p).is_some_and(|n| buttons.contains(&n));
                (p, pressed, selecting && self.joy_button.is_none() && self.wheel_part == p)
            })
            .collect();
        let view = WheelView { angle, range, show_arc: self.wheel_tab == WheelTab::Steering, parts: &parts, hat };
        if let Some(p) = figures::wheel(ui, &view) {
            self.wheel_part = p;
            self.joy_button = None;
            self.wheel_tab = WheelTab::Buttons;
        }
    }

    fn wheel_card(&mut self, ui: &mut Ui) {
        tabs(
            ui,
            &mut self.wheel_tab,
            &[(WheelTab::Steering, "Steering wheel"), (WheelTab::Pedals, "Pedals"), (WheelTab::Buttons, "Buttons")],
        );
        if self.status.wheel.is_none() {
            not_connected(ui, "Wheel");
        }
        match self.wheel_tab {
            WheelTab::Steering => self.wheel_steering(ui),
            WheelTab::Pedals => self.wheel_pedals(ui),
            WheelTab::Buttons => self.wheel_buttons(ui),
        }
    }

    fn wheel_steering(&mut self, ui: &mut Ui) {
        let default = self.cfg.profiles[0].wheel.clone().unwrap_or_default();
        let edit = self.edit;
        let Some(w) = override_section(ui, edit, &mut self.cfg.profiles[edit].wheel, &default) else { return };
        section(ui, "Operating range (angle)");
        slider_row(ui, egui::Slider::new(&mut w.range, wheel::RANGE_MIN..=wheel::RANGE_MAX).step_by(10.0).suffix("°"));
        ui.horizontal_wrapped(|ui| {
            for deg in [180, 270, 360, 540, 900] {
                ui.selectable_value(&mut w.range, deg, format!("{deg}°"));
            }
        });
        section(ui, "Force feedback strength");
        slider_row(ui, egui::Slider::new(&mut w.strength, 0..=100).suffix(" %"));
        section(ui, "Centering spring strength");
        slider_row(ui, egui::Slider::new(&mut w.centering, 0..=100).suffix(" %"));
        ui.label(RichText::new("Pulls the wheel to center while no game drives force feedback. 0 = off.").weak());
    }

    fn wheel_pedals(&mut self, ui: &mut Ui) {
        section(ui, "Pedal axes");
        ui.label(RichText::new("Press each pedal and check the right bar moves. Change the axis if it doesn't.").weak());
        ui.add_space(6.0);
        let options: Vec<(u16, String, f32)> = self
            .joy
            .as_ref()
            .map(|j| {
                j.axes
                    .iter()
                    .filter(|a| a.page == 0x01 && a.usage != USAGE_HAT)
                    .map(|a| (a.usage, a.name(), self.axis_value(a.usage).map_or(0.0, |v| v.0)))
                    .collect()
            })
            .unwrap_or_default();
        let invert = self.cfg.hardware.axes.invert_pedals;
        let axes = &mut self.cfg.hardware.axes;
        for (label, slot) in [("Clutch", &mut axes.clutch), ("Brake", &mut axes.brake), ("Gas", &mut axes.throttle), ("Steering", &mut axes.steering)] {
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 20.0], egui::Label::new(label));
                let current = options.iter().find(|o| o.0 == *slot).map_or(format!("{:#04x}", slot), |o| o.1.clone());
                egui::ComboBox::from_id_salt(("axis", label)).width(90.0).selected_text(current).show_ui(ui, |ui| {
                    for (usage, name, _) in &options {
                        ui.selectable_value(slot, *usage, name);
                    }
                });
                if let Some((_, _, v)) = options.iter().find(|o| o.0 == *slot) {
                    let shown = if invert && label != "Steering" { 1.0 - v } else { *v };
                    ui.add(egui::ProgressBar::new(shown).desired_width(150.0));
                }
            });
        }
        ui.checkbox(&mut axes.invert_pedals, "Pedals read high when released (Logitech default)");
        if options.is_empty() {
            ui.label(RichText::new("No live wheel input yet.").color(WARN));
        }
    }

    fn wheel_buttons(&mut self, ui: &mut Ui) {
        let (trigger_n, title) = match self.joy_button {
            Some(n) => (Some(n), self.cfg.hardware.wheel_button_name(n)),
            None => (self.cfg.hardware.button_of(self.wheel_part), self.wheel_part.label().to_string()),
        };
        ui.label(RichText::new(title).size(26.0).color(Color32::WHITE));
        ui.label(RichText::new("Click a button on the wheel or in the list below.").weak());
        ui.add_space(6.0);

        if self.joy_button.is_none() {
            let part = self.wheel_part;
            ui.horizontal_wrapped(|ui| {
                ui.label("Sends:");
                ui.label(RichText::new(trigger_n.map_or("unknown".into(), |n| format!("Button {n}"))).strong());
                if self.capture == Some(Capture::Wheel(part)) {
                    ui.label(RichText::new(format!("Press {} on the wheel…", part.label())).color(ACCENT));
                    if ui.small_button("Cancel").clicked() {
                        self.cancel_capture();
                    }
                } else if ui.small_button("Detect").clicked() {
                    self.start_capture(Capture::Wheel(part));
                }
            });
            if let Some(n) = &self.capture_note {
                ui.label(RichText::new(n).color(WARN));
            }
        }
        if let Some(n) = trigger_n {
            section(ui, "Assignment");
            ui.label(RichText::new("Wheel buttons still reach games; an assignment adds an action on top.").weak());
            self.assignment_ui(ui, Trigger::Joy(n), Id::new(("joy_assign", n)));
        }

        section(ui, "All wheel buttons");
        let (count, pressed) = match &self.joy {
            Some(j) => (j.button_count, j.state.buttons.clone()),
            None => (0, Vec::new()),
        };
        ui.horizontal_wrapped(|ui| {
            for n in 1..=count {
                let lit = pressed.contains(&n);
                let text = RichText::new(format!("{n}")).color(if lit { Color32::BLACK } else { Color32::WHITE });
                let mut b = egui::Button::new(text).min_size(egui::vec2(34.0, 26.0));
                if lit {
                    b = b.fill(ACCENT);
                }
                if ui.add(b).on_hover_text(self.cfg.hardware.wheel_button_name(n)).clicked() {
                    self.joy_button = Some(n);
                }
            }
        });
        if let Some(n) = self.joy_button {
            ui.horizontal(|ui| {
                ui.label("Label");
                let hw = &mut self.cfg.hardware;
                let mut name = hw.wheel_button_names.iter().find(|(b, _)| *b == n).map(|(_, s)| s.clone()).unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut name).hint_text(format!("Button {n}")).desired_width(160.0)).changed() {
                    hw.wheel_button_names.retain(|(b, _)| *b != n);
                    if !name.trim().is_empty() {
                        hw.wheel_button_names.push((n, name));
                    }
                }
            });
        }
    }
}

// ---------------------------------------------------------------- light

impl App {
    fn light_page(&mut self, ui: &mut Ui) {
        self.device_layout(ui, Self::light_card, |app, ui| {
            let (on, b, t) = match &app.status.light {
                Some(l) => (l.on, l.brightness, l.temperature),
                None => (false, 0, 4500),
            };
            figures::light(ui, on, b, t);
        });
    }

    fn light_card(&mut self, ui: &mut Ui) {
        if self.status.light.is_none() {
            not_connected(ui, "Light");
        }
        let default = self.cfg.profiles[0].light.clone().unwrap_or_default();
        let edit = self.edit;
        let Some(l) = override_section(ui, edit, &mut self.cfg.profiles[edit].light, &default) else { return };
        section(ui, "Power");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut l.on, true, RichText::new("  On  ").size(15.0));
            ui.selectable_value(&mut l.on, false, RichText::new("  Off  ").size(15.0));
        });
        section(ui, "Brightness");
        slider_row(ui, egui::Slider::new(&mut l.brightness, litra::BRIGHTNESS_MIN..=litra::BRIGHTNESS_MAX).suffix(" lm"));
        section(ui, "Color temperature");
        slider_row(ui, egui::Slider::new(&mut l.temperature, litra::TEMP_MIN..=litra::TEMP_MAX).step_by(litra::TEMP_STEP as f64).suffix(" K"));
        ui.horizontal_wrapped(|ui| {
            for (label, k) in [("Warm", 3000), ("Neutral", 4500), ("Daylight", 5600), ("Cool", 6500)] {
                if ui.button(label).clicked() {
                    l.temperature = k;
                }
            }
        });
    }
}


// ---------------------------------------------------------------- profiles, macros, settings

impl App {
    fn profiles_page(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("A profile activates automatically when one of its apps is focused. Default is used otherwise.").weak());
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(220.0);
                for (i, p) in self.cfg.profiles.iter().enumerate() {
                    let mut text = RichText::new(&p.name).size(15.0);
                    if i == self.status.active_profile {
                        text = text.color(ACCENT);
                    }
                    if ui.selectable_label(self.edit == i, text).clicked() {
                        self.edit = i;
                    }
                }
                ui.add_space(8.0);
                if ui.button("+ New profile").clicked() {
                    let n = self.cfg.profiles.len();
                    self.cfg.profiles.push(Profile::new(format!("Profile {n}")));
                    self.edit = n;
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let edit = self.edit;
                let recent = self.status.recent_apps.clone();
                let pinned = &mut self.cfg.pinned_profile;
                let p = &mut self.cfg.profiles[edit];
                section(ui, "Name");
                ui.add_enabled(edit != 0, egui::TextEdit::singleline(&mut p.name));
                let mut is_pinned = pinned.as_deref() == Some(p.name.as_str());
                if ui.checkbox(&mut is_pinned, "Always use this profile (ignore focused app)").changed() {
                    *pinned = is_pinned.then(|| p.name.clone());
                }
                if edit == 0 {
                    return;
                }
                section(ui, "Activate for these apps");
                let mut remove = None;
                for (i, app) in p.apps.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(app);
                        if ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    p.apps.remove(i);
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_app).hint_text("game.exe"));
                    let name = self.new_app.trim().to_lowercase();
                    if ui.button("Add").clicked() && !name.is_empty() && !p.apps.contains(&name) {
                        p.apps.push(name);
                        self.new_app.clear();
                    }
                });
                if !recent.is_empty() {
                    ui.label(RichText::new("Recently focused:").weak());
                    ui.horizontal_wrapped(|ui| {
                        for app in &recent {
                            if !p.apps.contains(app) && ui.small_button(format!("+ {app}")).clicked() {
                                p.apps.push(app.clone());
                            }
                        }
                    });
                }
                ui.add_space(12.0);
                if ui.button(RichText::new("Delete profile").color(WARN)).clicked() {
                    if pinned.as_deref() == Some(p.name.as_str()) {
                        *pinned = None;
                    }
                    self.cfg.profiles.remove(edit);
                    self.edit = 0;
                }
            });
        });
    }

    fn macros_page(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Assign a macro to a mouse or wheel button on its device page.").weak());
        ui.add_space(10.0);
        self.selected_macro = self.selected_macro.min(self.cfg.macros.len().saturating_sub(1));
        let mut record = None;
        let mut delete = false;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(220.0);
                for (i, m) in self.cfg.macros.iter().enumerate() {
                    if ui.selectable_label(self.selected_macro == i, RichText::new(&m.name).size(15.0)).clicked() {
                        self.selected_macro = i;
                    }
                }
                ui.add_space(8.0);
                if ui.button("+ New macro").clicked() {
                    let n = self.cfg.macros.len() + 1;
                    self.cfg.macros.push(Macro { name: format!("Macro {n}"), mode: MacroMode::Once, steps: Vec::new() });
                    self.selected_macro = self.cfg.macros.len() - 1;
                }
            });
            ui.separator();
            let idx = self.selected_macro;
            let recording = self.recording;
            let Some(m) = self.cfg.macros.get_mut(idx) else {
                ui.label("No macros yet.");
                return;
            };
            ui.vertical(|ui| {
                section(ui, "Name");
                ui.text_edit_singleline(&mut m.name);
                section(ui, "Playback");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut m.mode, MacroMode::Once, "Once per press");
                    ui.selectable_value(&mut m.mode, MacroMode::WhileHeld, "Repeat while held");
                    ui.selectable_value(&mut m.mode, MacroMode::Toggle, "Toggle repeat");
                });
                section(ui, "Steps");
                ui.horizontal(|ui| {
                    if recording == Some(idx) {
                        if ui.button(RichText::new("Stop recording").color(Color32::RED)).clicked() {
                            record = Some(false);
                        }
                        ui.label(RichText::new("Recording keys and mouse buttons…").color(ACCENT));
                    } else if ui.button("Record").clicked() {
                        record = Some(true);
                    }
                    if ui.button("Clear").clicked() {
                        m.steps.clear();
                    }
                });
                let mut op: Option<(usize, i32)> = None;
                for (i, step) in m.steps.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{:>3}", i + 1)).monospace().weak());
                        step_editor(ui, i, step);
                        if ui.small_button("⬆").clicked() {
                            op = Some((i, -1));
                        }
                        if ui.small_button("⬇").clicked() {
                            op = Some((i, 1));
                        }
                        if ui.small_button("🗑").clicked() {
                            op = Some((i, 0));
                        }
                    });
                }
                match op {
                    Some((i, 0)) => {
                        m.steps.remove(i);
                    }
                    Some((i, d)) => {
                        let j = i as i32 + d;
                        if j >= 0 && (j as usize) < m.steps.len() {
                            m.steps.swap(i, j as usize);
                        }
                    }
                    None => {}
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("+ Key press").clicked() {
                        m.steps.push(Step::KeyTap(0x41));
                    }
                    if ui.button("+ Delay").clicked() {
                        m.steps.push(Step::Delay(50));
                    }
                    if ui.button("+ Text").clicked() {
                        m.steps.push(Step::Text(String::new()));
                    }
                    if ui.button("+ Click").clicked() {
                        m.steps.push(Step::Click(MouseButton::Left));
                    }
                });
                ui.add_space(12.0);
                if ui.button(RichText::new("Delete macro").color(WARN)).clicked() {
                    record = Some(false);
                    delete = true;
                }
            });
        });
        if delete {
            self.cfg.macros.remove(self.selected_macro);
        }
        match record {
            Some(true) => {
                self.recording = Some(self.selected_macro);
                self.shared.engine.record_macro();
            }
            Some(false) => {
                self.recording = None;
                self.shared.engine.stop_capture();
            }
            None => {}
        }
    }

    fn settings_page(&mut self, ui: &mut Ui) {
        section(ui, "Startup");
        if ui.checkbox(&mut self.autostart, "Start G-Clone when Windows starts (in the tray)").changed() {
            match crate::autostart::set_enabled(self.autostart) {
                Ok(()) => self.autostart_error = None,
                Err(e) => {
                    self.autostart = !self.autostart;
                    self.autostart_error = Some(e);
                }
            }
        }
        if let Some(e) = &self.autostart_error {
            ui.label(RichText::new(format!("Could not change autostart: {e}")).color(WARN));
        }
        ui.checkbox(&mut self.cfg.start_hidden, "Always start in the tray without opening this window");
        ui.label(
            RichText::new("Closing this window keeps G-Clone running in the tray with almost no memory use. Quit from the tray menu.")
                .weak(),
        );
        section(ui, "Storage");
        ui.label(format!("Config file: {}", Config::path().display()));
        section(ui, "G HUB");
        ui.label("Quit Logitech G HUB while G-Clone runs, otherwise both apps fight over device settings.");
    }
}

// ---------------------------------------------------------------- editors

fn trigger_label(t: Trigger) -> String {
    match t {
        Trigger::Mouse(b) => b.label().to_string(),
        Trigger::Key(vk) => format!("Key {}", keys::name(vk)),
        Trigger::Joy(n) => format!("Wheel button {n}"),
        Trigger::GKey(bit) => format!("G-key #{bit}"),
    }
}

/// What a trigger does when nothing is assigned to it.
fn default_summary(t: Trigger) -> String {
    match t {
        Trigger::Mouse(MouseButton::Left) => "Primary click".into(),
        Trigger::Mouse(MouseButton::Right) => "Secondary click".into(),
        Trigger::Mouse(MouseButton::Middle) => "Middle click".into(),
        Trigger::Mouse(MouseButton::Back) => "Back".into(),
        Trigger::Mouse(MouseButton::Forward) => "Forward".into(),
        Trigger::Key(vk) => keys::name(vk),
        Trigger::Joy(n) => format!("Game button {n}"),
        // Windows never sees these buttons, so without an assignment they do nothing.
        Trigger::GKey(_) => "Unassigned".into(),
    }
}

fn action_summary(a: &Action) -> String {
    match a {
        Action::Disabled => "Disabled".into(),
        Action::Key { vk, mods } => {
            let mut parts = Vec::new();
            for (on, name) in [(mods.ctrl, "Ctrl"), (mods.shift, "Shift"), (mods.alt, "Alt"), (mods.win, "Win")] {
                if on {
                    parts.push(name.to_string());
                }
            }
            parts.push(keys::name(*vk));
            parts.join(" + ")
        }
        Action::Mouse(b) => b.label().into(),
        Action::Media(vk) => MEDIA_KEYS.iter().find(|(k, _)| k == vk).map_or("Media key", |(_, l)| *l).into(),
        Action::Macro(n) => format!("Macro: {n}"),
        Action::DpiUp => "DPI Up".into(),
        Action::DpiDown => "DPI Down".into(),
        Action::DpiCycle => "DPI Cycle".into(),
        Action::DpiShift(d) => format!("DPI Shift ({d})"),
        Action::LightToggle => "Toggle Litra".into(),
        Action::NextProfile => "Next Profile".into(),
        Action::Launch(t) => format!("Launch {}", t.rsplit(['\\', '/']).next().unwrap_or(t)),
    }
}

const ACTION_KINDS: [&str; 12] = [
    "Do nothing",
    "Keystroke",
    "Mouse button",
    "Macro",
    "DPI up",
    "DPI down",
    "DPI cycle",
    "DPI shift (hold)",
    "Toggle Litra light",
    "Next profile",
    "Launch app / URL",
    "Media / volume",
];

fn action_kind(a: &Action) -> usize {
    match a {
        Action::Disabled => 0,
        Action::Key { .. } => 1,
        Action::Mouse(_) => 2,
        Action::Macro(_) => 3,
        Action::DpiUp => 4,
        Action::DpiDown => 5,
        Action::DpiCycle => 6,
        Action::DpiShift(_) => 7,
        Action::LightToggle => 8,
        Action::NextProfile => 9,
        Action::Launch(_) => 10,
        Action::Media(_) => 11,
    }
}

fn action_for_kind(kind: usize, macros: &[String]) -> Action {
    match kind {
        1 => Action::Key { vk: 0x41, mods: Mods::default() },
        2 => Action::Mouse(MouseButton::Middle),
        3 => Action::Macro(macros.first().cloned().unwrap_or_default()),
        4 => Action::DpiUp,
        5 => Action::DpiDown,
        6 => Action::DpiCycle,
        7 => Action::DpiShift(400),
        8 => Action::LightToggle,
        9 => Action::NextProfile,
        10 => Action::Launch(String::new()),
        11 => Action::Media(MEDIA_KEYS[0].0),
        _ => Action::Disabled,
    }
}

const KIND_KEYSTROKE: usize = 1;

/// Edits one action. Returns true when the user asks to record a key combination;
/// `recording` shows that one is being captured for this action right now.
fn action_editor(ui: &mut Ui, id: Id, a: &mut Action, macros: &[String], recording: bool) -> bool {
    let mut record = false;
    let current = action_kind(a);
    let mut kind = current;
    egui::ComboBox::from_id_salt(id.with("kind"))
        .width(170.0)
        .selected_text(ACTION_KINDS[current])
        .show_ui(ui, |ui| {
            for (i, label) in ACTION_KINDS.iter().enumerate() {
                ui.selectable_value(&mut kind, i, *label);
            }
        });
    if kind != current {
        *a = action_for_kind(kind, macros);
        record = kind == KIND_KEYSTROKE;
    }
    match a {
        Action::Key { vk, mods } => {
            if recording {
                ui.label(RichText::new("Press the key combination…").color(ACCENT));
            } else {
                let combo = action_summary(&Action::Key { vk: *vk, mods: *mods });
                let button = egui::Button::new(RichText::new(combo).strong()).min_size(egui::vec2(120.0, 24.0));
                if ui.add(button).on_hover_text("Click, then press the keys you want (e.g. Ctrl + Shift + S)").clicked() {
                    record = true;
                }
            }
        }
        Action::Media(vk) => {
            let label = MEDIA_KEYS.iter().find(|(k, _)| k == vk).map_or("?", |(_, l)| *l);
            egui::ComboBox::from_id_salt(id.with("media")).selected_text(label).show_ui(ui, |ui| {
                for (k, l) in MEDIA_KEYS {
                    ui.selectable_value(vk, k, l);
                }
            });
        }
        Action::Mouse(b) => button_picker(ui, id.with("btn"), b),
        Action::Macro(name) => {
            egui::ComboBox::from_id_salt(id.with("macro"))
                .selected_text(if name.is_empty() { "(none)" } else { name.as_str() })
                .show_ui(ui, |ui| {
                    for m in macros {
                        ui.selectable_value(name, m.clone(), m);
                    }
                });
        }
        Action::DpiShift(dpi) => {
            ui.add(egui::DragValue::new(dpi).range(50..=25600).speed(50).suffix(" DPI"));
        }
        Action::Launch(target) => {
            ui.add(egui::TextEdit::singleline(target).hint_text("C:\\path\\app.exe or https://…").desired_width(220.0));
        }
        _ => {}
    }
    record
}

/// Manual key + modifier editing, for keys that are hard to press (e.g. F13–F24).
/// Shown on its own line under [`action_editor`].
fn manual_key_editor(ui: &mut Ui, id: Id, a: &mut Action) {
    if let Action::Key { vk, mods } = a {
        egui::CollapsingHeader::new(RichText::new("Pick key manually").small()).id_salt(id.with("manual")).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                key_picker(ui, id.with("key"), vk);
                ui.checkbox(&mut mods.ctrl, "Ctrl");
                ui.checkbox(&mut mods.shift, "Shift");
                ui.checkbox(&mut mods.alt, "Alt");
                ui.checkbox(&mut mods.win, "Win");
            });
        });
    }
}

fn key_picker(ui: &mut Ui, id: impl std::hash::Hash, vk: &mut u16) {
    egui::ComboBox::from_id_salt(id)
        .width(110.0)
        .height(320.0)
        .selected_text(keys::name(*vk))
        .show_ui(ui, |ui| {
            for (k, name) in keys::all() {
                ui.selectable_value(vk, *k, name);
            }
        });
}

fn button_picker(ui: &mut Ui, id: impl std::hash::Hash, b: &mut MouseButton) {
    egui::ComboBox::from_id_salt(id).selected_text(b.label()).show_ui(ui, |ui| {
        for x in MouseButton::ALL {
            ui.selectable_value(b, x, x.label());
        }
    });
}

fn step_editor(ui: &mut Ui, id: usize, step: &mut Step) {
    match step {
        Step::KeyDown(vk) => {
            ui.label("Key down");
            key_picker(ui, ("sk", id), vk);
        }
        Step::KeyUp(vk) => {
            ui.label("Key up");
            key_picker(ui, ("sk", id), vk);
        }
        Step::KeyTap(vk) => {
            ui.label("Press key");
            key_picker(ui, ("sk", id), vk);
        }
        Step::MouseDown(b) => {
            ui.label("Button down");
            button_picker(ui, ("sb", id), b);
        }
        Step::MouseUp(b) => {
            ui.label("Button up");
            button_picker(ui, ("sb", id), b);
        }
        Step::Click(b) => {
            ui.label("Click");
            button_picker(ui, ("sb", id), b);
        }
        Step::Delay(ms) => {
            ui.label("Wait");
            ui.add(egui::DragValue::new(ms).range(1..=60_000).suffix(" ms"));
        }
        Step::Text(s) => {
            ui.label("Type text");
            ui.add(egui::TextEdit::singleline(s).desired_width(240.0));
        }
    }
}

/// winit subscribes the window to raw mouse/keyboard input, so every movement of any mouse
/// (500 Hz on the G602) wakes and repaints the UI even when the pointer is elsewhere. We use
/// none of it, so unsubscribe; normal pointer and keyboard window messages are unaffected.
fn stop_raw_input() {
    use windows_sys::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_REMOVE};
    let devices = [0x02u16, 0x06].map(|usage| RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: usage,
        dwFlags: RIDEV_REMOVE,
        hwndTarget: std::ptr::null_mut(),
    });
    let ok = unsafe {
        RegisterRawInputDevices(devices.as_ptr(), devices.len() as u32, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
    };
    if ok == 0 {
        log::warn!("could not unregister raw input");
    }
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.extreme_bg_color = Color32::from_rgb(0x2C, 0x2E, 0x33);
    v.selection.bg_fill = Color32::from_rgb(0x1E, 0x90, 0xE6);
    v.hyperlink_color = ACCENT;
    v.widgets.hovered.bg_stroke.color = ACCENT;
    v.widgets.inactive.rounding = 8.0.into();
    v.widgets.hovered.rounding = 8.0.into();
    v.widgets.active.rounding = 8.0.into();
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.slider_width = 260.0;
        s.spacing.interact_size.y = 24.0;
        for (_, font) in s.text_styles.iter_mut() {
            font.size *= 1.08;
        }
    });
}
