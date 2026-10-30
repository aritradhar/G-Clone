//! Global low-level mouse/keyboard hooks (remapping, trigger capture, macro recording)
//! and the foreground-window watcher, all on one message-loop thread.
//!
//! Hook callbacks must return within a few ms or Windows silently removes them, so they
//! only do table lookups and `SendInput`; anything slower goes to another thread.

use super::{input, macros::MacroCmd, Cmd, Shared, UiEvent};
use crate::config::{Action, Binding, Mods, MouseButton, Step, Trigger};
use crossbeam_channel::Sender;
use std::collections::{HashMap, HashSet};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Instant;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CAPTURE_OFF: u8 = 0;
const CAPTURE_TRIGGER: u8 = 1;
const CAPTURE_RECORD: u8 = 2;
/// Next key combination (modifiers + key) becomes a keystroke assignment.
const CAPTURE_COMBO: u8 = 3;

const MOD_CTRL: u8 = 1;
const MOD_SHIFT: u8 = 2;
const MOD_ALT: u8 = 4;
const MOD_WIN: u8 = 8;

fn modifier_bit(vk: u16) -> Option<u8> {
    match vk {
        0x10 | 0xA0 | 0xA1 => Some(MOD_SHIFT),
        0x11 | 0xA2 | 0xA3 => Some(MOD_CTRL),
        0x12 | 0xA4 | 0xA5 => Some(MOD_ALT),
        0x5B | 0x5C => Some(MOD_WIN),
        _ => None,
    }
}

pub(super) struct Ctx {
    pub mgr: Sender<Cmd>,
    pub ui: Sender<UiEvent>,
    pub macros: Sender<MacroCmd>,
    pub shared: Arc<Shared>,
}

struct State {
    ctx: Ctx,
    table: RwLock<Arc<HashMap<Trigger, Action>>>,
    capture: AtomicU8,
    /// Triggers whose release must be swallowed (captured or bound while pressed).
    swallow_up: Mutex<HashSet<Trigger>>,
    /// Bound triggers currently held, to ignore keyboard auto-repeat for one-shot actions.
    held: Mutex<HashSet<Trigger>>,
    last_record: Mutex<Option<Instant>>,
    /// Every trigger currently held, bound or not, for the live device figures.
    pressed: Mutex<HashSet<Trigger>>,
    /// Modifiers held during combo capture, and the last one pressed (for a lone modifier).
    combo_mods: AtomicU8,
    combo_last_mod: Mutex<Option<u16>>,
}

static STATE: OnceLock<State> = OnceLock::new();
/// Set while the UI shows a live figure; only then do input events trigger repaints.
static LIVE: AtomicBool = AtomicBool::new(false);
/// Whether the active bindings use mouse buttons / keys as triggers.
static BOUND_MOUSE: AtomicBool = AtomicBool::new(false);
static BOUND_KEY: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
/// Posted to the hook thread to (un)install hooks after the need for them changed.
const WM_REFRESH_HOOKS: u32 = WM_APP + 7;

type LowLevelProc = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;

pub(super) fn set_live(on: bool) {
    if LIVE.swap(on, Ordering::Relaxed) != on {
        refresh_hooks();
    }
}

fn refresh_hooks() {
    let tid = HOOK_THREAD.load(Ordering::SeqCst);
    if tid != 0 {
        unsafe {
            PostThreadMessageW(tid, WM_REFRESH_HOOKS, 0, 0);
        }
    }
}

/// Which low-level hooks are needed right now: (mouse, keyboard). Every hooked mouse move
/// costs a context switch into this process, so hooks are only installed while in use.
fn hooks_wanted() -> (bool, bool) {
    let capturing = STATE.get().is_some_and(|s| s.capture.load(Ordering::SeqCst) != CAPTURE_OFF);
    let live = is_live();
    let extra = capturing || live;
    (BOUND_MOUSE.load(Ordering::SeqCst) || extra, BOUND_KEY.load(Ordering::SeqCst) || extra)
}

/// Installs or removes the hooks to match [`hooks_wanted`]. Runs on the hook thread only.
unsafe fn apply_hooks(mouse: &mut HHOOK, kbd: &mut HHOOK) {
    let (want_mouse, want_kbd) = hooks_wanted();
    let module = GetModuleHandleW(std::ptr::null());
    for (want, hook, id, proc_) in [
        (want_mouse, &mut *mouse, WH_MOUSE_LL, Some(mouse_proc as LowLevelProc) as HOOKPROC),
        (want_kbd, &mut *kbd, WH_KEYBOARD_LL, Some(keyboard_proc as LowLevelProc) as HOOKPROC),
    ] {
        if want && hook.is_null() {
            *hook = SetWindowsHookExW(id, proc_, module, 0);
            if hook.is_null() {
                log::error!("failed to install input hook {id}");
            }
        } else if !want && !hook.is_null() {
            UnhookWindowsHookEx(*hook);
            *hook = null_mut();
            // Releases can't be seen any more; forget what was held.
            if let Some(s) = STATE.get() {
                let is_mouse = id == WH_MOUSE_LL;
                s.pressed.lock().unwrap().retain(|t| match t {
                    Trigger::Mouse(_) => !is_mouse,
                    Trigger::Key(_) => is_mouse,
                    Trigger::Joy(_) | Trigger::GKey(_) => true,
                });
            }
        }
    }
    log::debug!("hooks: mouse {} keyboard {}", !mouse.is_null(), !kbd.is_null());
}

pub(super) fn is_live() -> bool {
    LIVE.load(Ordering::Relaxed)
}

pub(super) fn pressed() -> HashSet<Trigger> {
    STATE.get().map(|s| s.pressed.lock().unwrap().clone()).unwrap_or_default()
}

pub(super) fn start(ctx: Ctx) {
    let _ = STATE.set(State {
        ctx,
        table: RwLock::new(Arc::new(HashMap::new())),
        capture: AtomicU8::new(CAPTURE_OFF),
        swallow_up: Mutex::new(HashSet::new()),
        held: Mutex::new(HashSet::new()),
        last_record: Mutex::new(None),
        pressed: Mutex::new(HashSet::new()),
        combo_mods: AtomicU8::new(0),
        combo_last_mod: Mutex::new(None),
    });
    std::thread::Builder::new()
        .name("gclone-hooks".into())
        .spawn(|| unsafe {
            HOOK_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
            let (mut mouse, mut kbd): (HHOOK, HHOOK) = (null_mut(), null_mut());
            apply_hooks(&mut mouse, &mut kbd);
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                null_mut(),
                Some(foreground_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            if let Some(exe) = exe_of_window(GetForegroundWindow()) {
                send_foreground(exe);
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                if msg.message == WM_REFRESH_HOOKS {
                    apply_hooks(&mut mouse, &mut kbd);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn hook thread");
}

pub(super) fn set_bindings(bindings: &[Binding]) {
    if let Some(s) = STATE.get() {
        let map = bindings.iter().map(|b| (b.trigger, b.action.clone())).collect();
        *s.table.write().unwrap() = Arc::new(map);
    }
    BOUND_MOUSE.store(bindings.iter().any(|b| matches!(b.trigger, Trigger::Mouse(_))), Ordering::SeqCst);
    BOUND_KEY.store(bindings.iter().any(|b| matches!(b.trigger, Trigger::Key(_))), Ordering::SeqCst);
    refresh_hooks();
}

pub(super) fn capture_trigger() {
    set_capture(CAPTURE_TRIGGER);
}

pub(super) fn record() {
    if let Some(s) = STATE.get() {
        *s.last_record.lock().unwrap() = None;
    }
    set_capture(CAPTURE_RECORD);
}

pub(super) fn capture_combo() {
    if let Some(s) = STATE.get() {
        s.combo_mods.store(0, Ordering::SeqCst);
        *s.combo_last_mod.lock().unwrap() = None;
    }
    set_capture(CAPTURE_COMBO);
}

pub(super) fn stop_capture() {
    set_capture(CAPTURE_OFF);
}

fn set_capture(mode: u8) {
    if let Some(s) = STATE.get() {
        s.capture.store(mode, Ordering::SeqCst);
    }
    refresh_hooks();
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let msg = wparam as u32;
    if code == HC_ACTION as i32 && msg != WM_MOUSEMOVE {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        if info.flags & LLMHF_INJECTED == 0 {
            let xbutton = (info.mouseData >> 16) as u16;
            let event = match msg {
                WM_LBUTTONDOWN => Some((MouseButton::Left, true)),
                WM_LBUTTONUP => Some((MouseButton::Left, false)),
                WM_RBUTTONDOWN => Some((MouseButton::Right, true)),
                WM_RBUTTONUP => Some((MouseButton::Right, false)),
                WM_MBUTTONDOWN => Some((MouseButton::Middle, true)),
                WM_MBUTTONUP => Some((MouseButton::Middle, false)),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let b = if xbutton == 1 { MouseButton::Back } else { MouseButton::Forward };
                    Some((b, msg == WM_XBUTTONDOWN))
                }
                _ => None,
            };
            if let Some((button, down)) = event {
                if handle(Trigger::Mouse(button), down) {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        if info.flags & LLKHF_INJECTED == 0 {
            let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            if handle(Trigger::Key(info.vkCode as u16), down) {
                return 1;
            }
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

/// Processes one button/key edge from any source (hooks or the wheel's joystick thread).
/// Returns true when the event must be swallowed.
pub(super) fn handle(trigger: Trigger, down: bool) -> bool {
    let Some(s) = STATE.get() else { return false };

    {
        let mut pressed = s.pressed.lock().unwrap();
        let changed = if down { pressed.insert(trigger) } else { pressed.remove(&trigger) };
        if changed && is_live() {
            (s.ctx.shared.repaint)();
        }
    }

    match s.capture.load(Ordering::Relaxed) {
        // Left click stays usable so the user can press "Cancel" in the UI.
        CAPTURE_TRIGGER if trigger != Trigger::Mouse(MouseButton::Left) => {
            if down {
                s.capture.store(CAPTURE_OFF, Ordering::SeqCst);
                refresh_hooks();
                s.swallow_up.lock().unwrap().insert(trigger);
                send_ui(s, UiEvent::Captured(trigger));
            }
            return true;
        }
        CAPTURE_RECORD => {
            record_step(s, trigger, down);
            return false;
        }
        CAPTURE_COMBO => {
            if let Trigger::Key(vk) = trigger {
                return combo_step(s, vk, down);
            }
        }
        _ => {}
    }

    if !down && s.swallow_up.lock().unwrap().remove(&trigger) {
        return true;
    }

    let table = s.table.read().unwrap().clone();
    let Some(action) = table.get(&trigger) else { return false };
    let repeat = {
        let mut held = s.held.lock().unwrap();
        if down {
            !held.insert(trigger)
        } else {
            held.remove(&trigger);
            false
        }
    };

    match action {
        Action::Disabled => {}
        Action::Key { vk, mods } => input::combo(*vk, *mods, down),
        Action::Media(vk) => input::key(*vk, down),
        Action::Mouse(b) if !repeat => input::mouse(*b, down),
        Action::Macro(name) if !repeat => {
            let cmd = if down { MacroCmd::Press(name.clone()) } else { MacroCmd::Release(name.clone()) };
            let _ = s.ctx.macros.send(cmd);
        }
        Action::Mouse(_) | Action::Macro(_) => {}
        other if !repeat => {
            let _ = s.ctx.mgr.send(Cmd::Action(other.clone(), down));
        }
        _ => {}
    }
    true
}

/// Builds a key combination from keyboard input; all of it is swallowed so shortcuts like
/// Alt+F4 don't fire while being recorded. A lone modifier (press + release) also counts.
fn combo_step(s: &State, vk: u16, down: bool) -> bool {
    let finish = |vk: u16, mods: u8| {
        s.capture.store(CAPTURE_OFF, Ordering::SeqCst);
        refresh_hooks();
        let mods = Mods {
            ctrl: mods & MOD_CTRL != 0,
            shift: mods & MOD_SHIFT != 0,
            alt: mods & MOD_ALT != 0,
            win: mods & MOD_WIN != 0,
        };
        send_ui(s, UiEvent::CapturedCombo { vk, mods });
    };
    match (modifier_bit(vk), down) {
        (Some(bit), true) => {
            s.combo_mods.fetch_or(bit, Ordering::SeqCst);
            *s.combo_last_mod.lock().unwrap() = Some(vk);
        }
        (Some(bit), false) => {
            let before = s.combo_mods.fetch_and(!bit, Ordering::SeqCst);
            // Released the only modifier without pressing another key: assign the modifier itself.
            if before == bit && *s.combo_last_mod.lock().unwrap() == Some(vk) {
                finish(vk, 0);
            }
        }
        (None, true) => {
            let mods = s.combo_mods.load(Ordering::SeqCst);
            // Releases of these keys arrive after capture ends; swallow them too.
            let mut swallow = s.swallow_up.lock().unwrap();
            swallow.insert(Trigger::Key(vk));
            for m in [0x10u16, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C] {
                if modifier_bit(m).is_some_and(|b| mods & b != 0) {
                    swallow.insert(Trigger::Key(m));
                }
            }
            drop(swallow);
            finish(vk, mods);
        }
        (None, false) => {}
    }
    true
}

fn record_step(s: &State, trigger: Trigger, down: bool) {
    let step = match trigger {
        Trigger::Key(vk) => {
            // Ignore auto-repeat while recording.
            let mut held = s.held.lock().unwrap();
            if down && !held.insert(trigger) {
                return;
            }
            if !down {
                held.remove(&trigger);
            }
            if down { Step::KeyDown(vk) } else { Step::KeyUp(vk) }
        }
        // Left click is reserved for operating the UI (the Stop button).
        Trigger::Mouse(MouseButton::Left) | Trigger::Joy(_) | Trigger::GKey(_) => return,
        Trigger::Mouse(b) => {
            if down { Step::MouseDown(b) } else { Step::MouseUp(b) }
        }
    };
    let now = Instant::now();
    if let Some(prev) = s.last_record.lock().unwrap().replace(now) {
        let ms = now.duration_since(prev).as_millis() as u32;
        if ms > 0 {
            send_ui(s, UiEvent::Recorded(Step::Delay(ms)));
        }
    }
    send_ui(s, UiEvent::Recorded(step));
}

fn send_ui(s: &State, ev: UiEvent) {
    let _ = s.ctx.ui.send(ev);
    (s.ctx.shared.repaint)();
}

unsafe extern "system" fn foreground_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if let Some(exe) = exe_of_window(hwnd) {
        send_foreground(exe);
    }
}

fn send_foreground(exe: String) {
    if let Some(s) = STATE.get() {
        let _ = s.ctx.mgr.send(Cmd::Foreground(exe));
    }
}

/// File name of the process owning `hwnd`, lower-cased (e.g. `forzahorizon5.exe`).
unsafe fn exe_of_window(hwnd: HWND) -> Option<String> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return None;
    }
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
    CloseHandle(process);
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit('\\').next().map(str::to_lowercase)
}
