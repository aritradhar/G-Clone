//! On-screen popup (e.g. "1500 DPI") shown briefly near the bottom of the screen the cursor
//! is on. A plain Win32 layered window on its own thread, so it works while the main UI is
//! closed to the tray and costs nothing while hidden.

use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_OSD_SHOW: u32 = WM_APP + 21;
const TIMER_HIDE: usize = 1;
const SHOW_MS: u32 = 1500;
/// Size at 96 DPI; scaled for the monitor.
const WIDTH: i32 = 220;
const HEIGHT: i32 = 84;
const BOTTOM_MARGIN: i32 = 120;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: u32 = 2;

static HWND_OSD: AtomicIsize = AtomicIsize::new(0);
/// (big line, small line) currently shown.
static TEXT: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));
static STARTED: OnceLock<()> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// Shows `big` (e.g. "1500 DPI") with an optional `small` line under it.
pub fn show(big: impl Into<String>, small: impl Into<String>) {
    STARTED.get_or_init(|| {
        std::thread::Builder::new().name("gclone-osd".into()).spawn(run).ok();
    });
    *TEXT.lock().unwrap() = (big.into(), small.into());
    // The window may still be getting created on first use; retry briefly.
    for _ in 0..50 {
        let hwnd = HWND_OSD.load(Ordering::SeqCst);
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_OSD_SHOW, 0, 0);
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn run() {
    unsafe {
        let hinst = GetModuleHandleW(null());
        let class = wide("GCloneOsd");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class.as_ptr(),
            wide("G-Clone OSD").as_ptr(),
            WS_POPUP,
            0,
            0,
            WIDTH,
            HEIGHT,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            log::warn!("OSD window could not be created");
            return;
        }
        SetLayeredWindowAttributes(hwnd, 0, 235, LWA_ALPHA);
        let pref = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, (&pref as *const u32).cast(), 4);
        HWND_OSD.store(hwnd as isize, Ordering::SeqCst);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Positions the popup at the bottom centre of the monitor under the cursor.
unsafe fn place(hwnd: HWND) {
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = std::mem::zeroed();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(mon, &mut mi);
    let work = mi.rcWork;
    // Size for this monitor's scale; GetDpiForWindow follows the window, so move it first.
    SetWindowPos(hwnd, HWND_TOPMOST, work.left, work.top, WIDTH, HEIGHT, SWP_NOACTIVATE);
    let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
    let (w, h) = ((WIDTH as f32 * scale) as i32, (HEIGHT as f32 * scale) as i32);
    let x = work.left + (work.right - work.left - w) / 2;
    let y = work.bottom - h - (BOTTOM_MARGIN as f32 * scale) as i32;
    SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_OSD_SHOW => {
            place(hwnd);
            InvalidateRect(hwnd, null(), 1);
            // Restarting the timer keeps the popup up while the value keeps changing.
            SetTimer(hwnd, TIMER_HIDE, SHOW_MS, None);
            0
        }
        WM_TIMER if wparam == TIMER_HIDE => {
            KillTimer(hwnd, TIMER_HIDE);
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let bg = CreateSolidBrush(rgb(0x20, 0x22, 0x26));
    FillRect(hdc, &rc, bg);
    DeleteObject(bg);
    // Teal accent bar on the left, matching the app.
    let accent = CreateSolidBrush(rgb(0x00, 0xC8, 0xD7));
    let bar = RECT { left: 0, top: 0, right: (rc.bottom / 14).max(4), bottom: rc.bottom };
    FillRect(hdc, &bar, accent);
    DeleteObject(accent);

    let (big, small) = TEXT.lock().unwrap().clone();
    let h = rc.bottom;
    SetBkMode(hdc, TRANSPARENT as i32);
    let face = wide("Segoe UI");
    let draw = |text: &str, size: i32, weight: i32, color: COLORREF, area: RECT| {
        let font = CreateFontW(-size, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, 0, face.as_ptr());
        let old = SelectObject(hdc, font);
        SetTextColor(hdc, color);
        let mut text: Vec<u16> = text.encode_utf16().collect();
        let mut area = area;
        DrawTextW(hdc, text.as_mut_ptr(), text.len() as i32, &mut area, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, old);
        DeleteObject(font);
    };
    if small.is_empty() {
        draw(&big, h * 2 / 5, 600, rgb(0xF2, 0xF2, 0xF2), rc);
    } else {
        draw(&big, h * 2 / 5, 600, rgb(0xF2, 0xF2, 0xF2), RECT { bottom: h * 64 / 100, ..rc });
        draw(&small, h / 5, 400, rgb(0x9A, 0x9C, 0xA2), RECT { top: h * 58 / 100, bottom: h * 92 / 100, ..rc });
    }
    EndPaint(hwnd, &ps);
}
