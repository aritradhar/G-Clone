// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod figures;
mod icon;
mod tray;
mod ui;

use eframe::egui;
use gclone_core::config::Config;
use gclone_core::engine::{self, Engine};
use std::sync::{Arc, Mutex};

/// State that outlives the window: the window is created and destroyed on demand.
pub struct AppShared {
    pub engine: Engine,
    pub cfg: Mutex<Config>,
    /// egui context of the open window, used by the engine to request repaints.
    pub ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl AppShared {
    pub fn save(&self) {
        if let Err(e) = self.cfg.lock().unwrap().save() {
            log::error!("cannot save {}: {e}", Config::path().display());
        }
    }
}

/// Ensures one running copy. A second launch signals the first to show its window and exits.
fn single_instance() {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
    };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (mutex_name, event_name) = (wide("Local\\GClone.Instance"), wide("Local\\GClone.Open"));
    unsafe {
        CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr());
            if !ev.is_null() {
                SetEvent(ev);
            }
            std::process::exit(0);
        }
        // Auto-reset event; handles are process-lifetime, so they're intentionally never closed.
        let ev = CreateEventW(std::ptr::null(), 0, 0, event_name.as_ptr()) as isize;
        std::thread::Builder::new()
            .name("gclone-instance".into())
            .spawn(move || loop {
                WaitForSingleObject(ev as _, INFINITE);
                tray::request_open();
            })
            .ok();
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    single_instance();

    let cfg = Config::load().unwrap_or_else(|| {
        let c = engine::bootstrap_config();
        if let Err(e) = c.save() {
            log::error!("cannot save {}: {e}", Config::path().display());
        }
        c
    });
    let minimized = cfg.start_hidden || std::env::args().any(|a| a == autostart::MINIMIZED_ARG);

    let ctx: Arc<Mutex<Option<egui::Context>>> = Arc::default();
    let repaint_ctx = ctx.clone();
    let engine = Engine::start(cfg.clone(), move || {
        if let Some(c) = repaint_ctx.lock().unwrap().as_ref() {
            c.request_repaint();
        }
    });
    let shared = Arc::new(AppShared { engine, cfg: Mutex::new(cfg), ctx });
    let _tray = tray::create(shared.clone());

    if minimized {
        tray::wait_for_open();
    }
    loop {
        run_window(&shared);
        tray::wait_for_open();
    }
}

/// Shows the window until the user closes it, then frees everything UI-related.
fn run_window(shared: &Arc<AppShared>) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("G-Clone")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(tray::window_icon()),
        run_and_return: true,
        ..Default::default()
    };
    let s = shared.clone();
    if let Err(e) = eframe::run_native("G-Clone", options, Box::new(move |cc| Ok(Box::new(ui::App::new(cc, s))))) {
        log::error!("window failed: {e}");
    }
    log::info!("window closed, running tray-only");
    tray::set_window(0);
    *shared.ctx.lock().unwrap() = None;
    shared.engine.set_live(false);
    shared.engine.stop_capture();
    shared.save();
    // The GL driver stays mapped after the window is gone; hand its pages back to Windows
    // so tray mode costs ~1 MB of RAM. They're only paged back in if the window reopens.
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}
