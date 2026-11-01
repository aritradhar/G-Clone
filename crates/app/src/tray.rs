//! System tray icon and the tray-only message loop.
//!
//! When the window is closed the whole UI (window, GL context, egui) is torn down and
//! only the engine and this tray stay alive, keeping RAM and CPU near zero. Clicking the
//! tray icon asks `main` to build the window again.

use crate::{icon, AppShared};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, MSG,
    SW_RESTORE, WM_APP,
};

const WM_OPEN: u32 = WM_APP + 1;

/// HWND of the open window, 0 while running tray-only.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

pub fn set_window(hwnd: isize) {
    WINDOW.store(hwnd, Ordering::SeqCst);
}

pub fn request_open() {
    let hwnd = WINDOW.load(Ordering::SeqCst);
    unsafe {
        if hwnd != 0 {
            ShowWindow(hwnd as _, SW_RESTORE);
            SetForegroundWindow(hwnd as _);
        } else {
            PostThreadMessageW(MAIN_THREAD.load(Ordering::SeqCst), WM_OPEN, 0, 0);
        }
    }
}

/// Pumps messages (tray events) until the user asks for the window.
pub fn wait_for_open() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_OPEN {
                return;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    std::process::exit(0);
}

pub fn window_icon() -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: icon::rgba(64), width: 64, height: 64 }
}

/// Must be called on the main thread, which also runs the window / tray message loops.
pub fn create(shared: Arc<AppShared>) -> Option<TrayIcon> {
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

    let menu = Menu::new();
    let open = MenuItem::new("Open G-Clone", true, None);
    let quit = MenuItem::new("Quit", true, None);
    menu.append_items(&[&open, &quit]).ok()?;
    let quit_id = quit.id().clone();

    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == quit_id {
            shared.save();
            // Lets the device thread hand the rev lights back to the wheel first.
            shared.engine.shutdown_and_wait(std::time::Duration::from_millis(500));
            std::process::exit(0);
        }
        request_open();
    }));
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        let open = matches!(
            e,
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { .. }
        );
        if open {
            request_open();
        }
    }));

    let icon = tray_icon::Icon::from_rgba(icon::rgba(32), 32, 32).ok()?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("G-Clone")
        .with_icon(icon)
        .build()
        .map_err(|e| log::error!("tray icon: {e}"))
        .ok()
}
