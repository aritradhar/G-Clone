//! Background engine: one manager thread owns the devices, a hook thread handles input,
//! a macro thread plays macros. The UI talks to it through [`Engine`].

mod gkeys;
mod haptics;
mod hook;
mod input;
mod joy;
mod macros;
pub mod osd;
mod procs;

pub use joy::JoyLive;

use crate::config::{
    Action, Config, LightSettings, Mods, MouseSettings, Resolved, RevLights, Step, Trigger, WheelSettings,
};
use crate::devices::{
    discover,
    joystick::Joystick,
    litra::Litra,
    mouse::{Battery, Mouse},
    wheel::{Axis as WheelAxis, Wheel, REV_ALL, REV_COUNT},
    Kind,
};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::hidpp::{self, Error};
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use hidapi::HidApi;
use macros::MacroCmd;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const RESCAN_EVERY: Duration = Duration::from_secs(5);
const BATTERY_EVERY: Duration = Duration::from_secs(60);

/// Events for the UI produced by the input hooks.
#[derive(Debug, Clone)]
pub enum UiEvent {
    Captured(Trigger),
    Recorded(Step),
    /// A key combination pressed for a keystroke assignment.
    CapturedCombo { vk: u16, mods: Mods },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouseStatus {
    pub dpi: u16,
    pub dpi_values: Vec<u16>,
    /// Report interval actually in effect (ms); the Endurance switch forces 8 ms.
    pub report_rate: u8,
    /// `Some(true)` while the hardware Performance/Endurance switch is on Endurance;
    /// `None` when it can't be told apart (report rate set to 125 Hz anyway).
    pub endurance: Option<bool>,
    pub report_rates: Vec<u8>,
    pub battery: Option<(u8, bool)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LightStatus {
    pub on: bool,
    pub brightness: u16,
    pub temperature: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WheelStatus {
    pub range: u16,
    pub strength: u8,
    pub centering: u8,
    /// Sensitivity per `devices::wheel::Axis::ALL`, as last read from or written to the wheel.
    pub sensitivity: [Option<u8>; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub mouse: Option<MouseStatus>,
    pub light: Option<LightStatus>,
    pub wheel: Option<WheelStatus>,
    pub active_profile: usize,
    pub dpi_stage: usize,
    pub foreground: String,
    pub recent_apps: Vec<String>,
    pub last_error: Option<String>,
    /// Sound haptics is on but paused because G HUB's agent (real TRUEFORCE) is running.
    pub haptics_paused_for_ghub: bool,
}

pub(crate) enum Cmd {
    Config(Box<Config>),
    Foreground(String),
    Action(Action, bool),
    /// New rev-light mask computed from live wheel input.
    RevLights(u8),
    RevTest,
    /// Sound-haptics vibration update (magnitude 0 = stop).
    Haptic { magnitude: u16, period_ms: u16 },
    HapticTest,
    Shutdown,
}

/// What the wheel input thread needs to drive the rev lights itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RevSource {
    pub mode: RevLights,
    /// Joystick usage of the axis to show (accelerator or steering).
    pub usage: u16,
    pub invert: bool,
}

pub(crate) struct Shared {
    status: Mutex<Status>,
    joy: Mutex<Option<JoyLive>>,
    rev: Mutex<RevSource>,
    /// Set once the device thread has finished (after restoring the rev lights).
    stopped: AtomicBool,
    repaint: Box<dyn Fn() + Send + Sync>,
}

pub struct Engine {
    tx: Sender<Cmd>,
    shared: Arc<Shared>,
    ui_rx: Receiver<UiEvent>,
}

impl Engine {
    /// Starts all background threads. `repaint` is called whenever the UI should refresh.
    pub fn start(cfg: Config, repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx) = unbounded();
        let (ui_tx, ui_rx) = unbounded();
        let shared = Arc::new(Shared {
            status: Mutex::new(Status::default()),
            joy: Mutex::new(None),
            rev: Mutex::new(RevSource { mode: RevLights::Wheel, usage: 0, invert: false }),
            stopped: AtomicBool::new(false),
            repaint: Box::new(repaint),
        });
        let macro_tx = macros::spawn();
        hook::start(hook::Ctx { mgr: tx.clone(), ui: ui_tx, macros: macro_tx.clone(), shared: shared.clone() });

        let mgr = Manager::new(cfg, shared.clone(), macro_tx, tx.clone());
        thread::Builder::new()
            .name("gclone-devices".into())
            .spawn(move || mgr.run(rx))
            .expect("spawn device thread");
        Self { tx, shared, ui_rx }
    }

    pub fn set_config(&self, cfg: Config) {
        let _ = self.tx.send(Cmd::Config(Box::new(cfg)));
    }

    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }

    /// Live wheel axes and buttons, if the wheel is connected.
    pub fn joystick(&self) -> Option<JoyLive> {
        self.shared.joy.lock().unwrap().clone()
    }

    /// Every button / key / wheel button currently held.
    pub fn pressed(&self) -> std::collections::HashSet<Trigger> {
        hook::pressed()
    }

    /// While on, input changes request UI repaints (for the live device figures).
    pub fn set_live(&self, on: bool) {
        hook::set_live(on);
    }

    pub fn poll_events(&self) -> Vec<UiEvent> {
        self.ui_rx.try_iter().collect()
    }

    /// The next button / key press is reported as [`UiEvent::Captured`] instead of acting.
    pub fn capture_trigger(&self) {
        hook::capture_trigger();
    }

    /// The next key combination is reported as [`UiEvent::CapturedCombo`] (and swallowed).
    pub fn capture_combo(&self) {
        hook::capture_combo();
    }

    /// Reports keyboard and mouse-button input as [`UiEvent::Recorded`] steps.
    pub fn record_macro(&self) {
        hook::record();
    }

    pub fn stop_capture(&self) {
        hook::stop_capture();
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }

    /// Stops the device thread and waits (up to `timeout`) for it to hand the rev lights back
    /// to the wheel, so quitting never leaves them frozen.
    pub fn shutdown_and_wait(&self, timeout: Duration) {
        self.shutdown();
        let end = std::time::Instant::now() + timeout;
        while !self.shared.stopped.load(Ordering::SeqCst) && std::time::Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Runs a short rev-light sweep and flash on the wheel.
    pub fn test_rev_lights(&self) {
        let _ = self.tx.send(Cmd::RevTest);
    }

    /// Plays a short test vibration on the wheel.
    pub fn test_haptics(&self) {
        let _ = self.tx.send(Cmd::HapticTest);
    }

    /// Current sound-haptics level 0..1 (0 while off or silent).
    pub fn haptic_level(&self) -> f32 {
        haptics::level()
    }
}

/// Builds a first-run config from the devices' current settings so nothing changes on launch.
pub fn bootstrap_config() -> Config {
    let mut cfg = Config::default();
    let Ok(api) = HidApi::new() else { return cfg };
    let d = &mut cfg.profiles[0];
    for (kind, _, dev) in discover(&api, |_| true) {
        match kind {
            Kind::Mouse => {
                let mut m = Mouse::new(dev);
                if let (Ok(dpi), Ok(rate)) = (m.dpi(), m.report_rate()) {
                    let s = d.mouse.get_or_insert_with(MouseSettings::default);
                    if !s.dpi_stages.contains(&dpi) {
                        s.dpi_stages.push(dpi);
                        s.dpi_stages.sort_unstable();
                    }
                    s.default_stage = s.dpi_stages.iter().position(|v| *v == dpi).unwrap_or(0);
                    s.report_rate_ms = rate;
                }
            }
            Kind::Light => {
                let mut l = Litra::new(dev);
                if let (Ok(on), Ok(brightness), Ok(temperature)) = (l.is_on(), l.brightness(), l.temperature()) {
                    d.light = Some(LightSettings { on, brightness, temperature });
                }
            }
            Kind::Wheel => {
                let mut w = Wheel::new(dev);
                if let (Ok(range), Ok(strength)) = (w.range(), w.strength()) {
                    d.wheel = Some(WheelSettings { range, strength, centering: 0, ..Default::default() });
                }
            }
        }
    }
    cfg
}

/// Values last written to each device; `None` forces a write on the next sync.
#[derive(Default)]
struct Applied {
    dpi: Option<u16>,
    rate: Option<u8>,
    light_on: Option<bool>,
    brightness: Option<u16>,
    temperature: Option<u16>,
    range: Option<u16>,
    strength: Option<u8>,
    centering: Option<u8>,
    sensitivity: [Option<u8>; 4],
    rev_mode: Option<RevLights>,
    /// Last mask written while showing live input.
    rev_mask: Option<u8>,
}

struct MouseCaps {
    dpi_values: Vec<u16>,
    rates: Vec<u8>,
    battery: Option<Battery>,
    /// Report interval read back from the mouse, if known.
    actual_rate: Option<u8>,
}

/// Endurance mode on the G602 locks the report interval to 8 ms (125 Hz).
const ENDURANCE_RATE_MS: u8 = 8;

struct Manager {
    /// Own command channel, handed to the wheel input thread for rev-light updates.
    tx: Sender<Cmd>,
    /// Running sound-haptics capture, while enabled and the wheel is connected.
    haptics: Option<haptics::Haptics>,
    /// G HUB's agent is running, so a game may be streaming real TRUEFORCE.
    ghub_running: bool,
    haptics_paused: bool,
    api: Option<HidApi>,
    cfg: Config,
    shared: Arc<Shared>,
    macro_tx: Sender<MacroCmd>,
    mouse: Option<Mouse>,
    mouse_caps: Option<MouseCaps>,
    light: Option<Litra>,
    wheel: Option<Wheel>,
    wheel_pid: u16,
    joy_alive: Arc<AtomicBool>,
    mouse_pid: u16,
    gkeys_alive: Arc<AtomicBool>,
    applied: Applied,
    /// Profile currently in effect and the settings it resolved to.
    active: usize,
    current: Option<Resolved>,
    /// Profile chosen with the NextProfile action; overrides app matching.
    manual: Option<usize>,
    dpi_stage: usize,
    dpi_shift: Option<u16>,
    light_on: bool,
    foreground: String,
    recent: Vec<String>,
    last_error: Option<String>,
    last_scan: Instant,
    last_battery: Instant,
}

impl Manager {
    fn new(cfg: Config, shared: Arc<Shared>, macro_tx: Sender<MacroCmd>, tx: Sender<Cmd>) -> Self {
        let _ = macro_tx.send(MacroCmd::Set(cfg.macros.clone()));
        Self {
            tx,
            haptics: None,
            ghub_running: false,
            haptics_paused: false,
            api: None,
            cfg,
            shared,
            macro_tx,
            mouse: None,
            mouse_caps: None,
            light: None,
            wheel: None,
            wheel_pid: 0,
            joy_alive: Arc::new(AtomicBool::new(false)),
            mouse_pid: 0,
            gkeys_alive: Arc::new(AtomicBool::new(false)),
            applied: Applied::default(),
            active: 0,
            current: None,
            manual: None,
            dpi_stage: 0,
            dpi_shift: None,
            light_on: false,
            foreground: String::new(),
            recent: Vec::new(),
            last_error: None,
            last_scan: Instant::now() - RESCAN_EVERY,
            last_battery: Instant::now(),
        }
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        self.refresh_profile();
        loop {
            if self.last_scan.elapsed() >= RESCAN_EVERY {
                self.rescan();
            }
            if self.last_battery.elapsed() >= BATTERY_EVERY {
                self.read_battery();
            }
            self.sync();
            let live = hook::is_live();
            if live {
                self.read_back();
            }
            self.publish();

            // Poll faster only while the UI shows a live device page.
            let tick = if live { Duration::from_millis(250) } else { Duration::from_secs(1) };
            match rx.recv_timeout(tick) {
                Ok(Cmd::Config(cfg)) => {
                    let _ = self.macro_tx.send(MacroCmd::Set(cfg.macros.clone()));
                    self.cfg = *cfg;
                    if self.manual.is_some_and(|m| m >= self.cfg.profiles.len()) {
                        self.manual = None;
                    }
                    self.refresh_profile();
                }
                Ok(Cmd::Foreground(exe)) => {
                    let own = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
                    if own.as_deref() != Some(exe.as_str()) && exe != "explorer.exe" {
                        self.recent.retain(|e| *e != exe);
                        self.recent.insert(0, exe.clone());
                        self.recent.truncate(10);
                    }
                    self.foreground = exe;
                    self.refresh_profile();
                }
                Ok(Cmd::Action(action, down)) => self.action(action, down),
                Ok(Cmd::RevLights(mask)) => {
                    let res = self.show_rev_mask(mask);
                    self.check(Kind::Wheel, res);
                }
                Ok(Cmd::RevTest) => {
                    let res = self.rev_test();
                    self.check(Kind::Wheel, res);
                }
                Ok(Cmd::Haptic { magnitude, period_ms }) => {
                    if let Some(w) = self.wheel.as_mut() {
                        let res = w.set_vibration(magnitude, period_ms);
                        self.check(Kind::Wheel, res);
                    }
                }
                Ok(Cmd::HapticTest) => {
                    let res = self.haptic_test();
                    self.check(Kind::Wheel, res);
                }
                Ok(Cmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        self.haptics = None;
        // Never leave the rev lights frozen under our control after quitting.
        if let Some(w) = self.wheel.as_mut() {
            let _ = w.set_vibration(0, 0);
            if !matches!(self.applied.rev_mode, None | Some(RevLights::Wheel)) {
                let _ = w.set_rev_control(false);
            }
        }
        self.shared.stopped.store(true, Ordering::SeqCst);
    }

    /// Writes a live rev-light mask, if a live mode is active and the mask changed.
    fn show_rev_mask(&mut self, mask: u8) -> hidpp::Result<()> {
        let live = matches!(self.applied.rev_mode, Some(RevLights::Accelerator | RevLights::Steering));
        if let (true, Some(w)) = (live, self.wheel.as_mut()) {
            if self.applied.rev_mask != Some(mask) {
                self.applied.rev_mask = Some(mask);
                w.set_rev_lights(mask)?;
            }
        }
        Ok(())
    }

    /// A short vibration sequence: fast strong buzz, slower rumble, then a quick fade.
    fn haptic_test(&mut self) -> hidpp::Result<()> {
        let Some(w) = self.wheel.as_mut() else { return Ok(()) };
        for (mag, period, ms) in [(0x6000, 25, 700), (0x3800, 66, 700), (0x2000, 33, 300), (0x1000, 33, 300)] {
            w.set_vibration(mag, period)?;
            thread::sleep(Duration::from_millis(ms));
        }
        w.set_vibration(0, 0)
    }

    /// Rev-counter sweep twice and three flashes, then the profile's mode is re-applied.
    fn rev_test(&mut self) -> hidpp::Result<()> {
        let Some(w) = self.wheel.as_mut() else { return Ok(()) };
        w.set_rev_control(true)?;
        for _ in 0..2 {
            for lit in 0..=REV_COUNT {
                w.set_rev_lights(((1u16 << lit) - 1) as u8)?;
                thread::sleep(Duration::from_millis(120));
            }
        }
        for _ in 0..3 {
            w.set_rev_lights(REV_ALL)?;
            thread::sleep(Duration::from_millis(250));
            w.set_rev_lights(0)?;
            thread::sleep(Duration::from_millis(250));
        }
        self.applied.rev_mode = None;
        self.applied.rev_mask = None;
        Ok(())
    }

    fn pick_profile(&self) -> usize {
        if let Some(name) = &self.cfg.pinned_profile {
            if let Some(i) = self.cfg.profiles.iter().position(|p| &p.name == name) {
                return i;
            }
        }
        self.manual.unwrap_or_else(|| self.cfg.profile_for_app(&self.foreground))
    }

    /// Re-evaluates which profile applies and reseeds runtime state from what changed.
    fn refresh_profile(&mut self) {
        let index = self.pick_profile();
        let next = self.cfg.resolved(index);
        let switched = index != self.active || self.current.is_none();
        let prev = self.current.take();

        if switched || prev.as_ref().map(|p| &p.mouse) != Some(&next.mouse) {
            self.dpi_stage = next.mouse.default_stage.min(next.mouse.dpi_stages.len().saturating_sub(1));
            self.dpi_shift = None;
        }
        if switched || prev.as_ref().map(|p| &p.light) != Some(&next.light) {
            self.light_on = next.light.on;
        }
        if switched || prev.as_ref().map(|p| &p.bindings) != Some(&next.bindings) {
            hook::set_bindings(&next.bindings);
        }
        if switched {
            log::info!("profile -> {}", self.cfg.profiles[index].name);
        }
        self.active = index;
        self.current = Some(next);
    }

    fn action(&mut self, action: Action, down: bool) {
        let stages = self.current.as_ref().map_or(0, |c| c.mouse.dpi_stages.len());
        match action {
            Action::DpiUp if down && stages > 0 => {
                self.dpi_shift = None;
                self.dpi_stage = (self.dpi_stage + 1).min(stages - 1);
            }
            Action::DpiDown if down => {
                self.dpi_shift = None;
                self.dpi_stage = self.dpi_stage.saturating_sub(1);
            }
            Action::DpiCycle if down && stages > 0 => {
                self.dpi_shift = None;
                self.dpi_stage = (self.dpi_stage + 1) % stages;
            }
            Action::DpiShift(dpi) => self.dpi_shift = down.then_some(dpi),
            Action::LightToggle if down => self.light_on = !self.light_on,
            Action::NextProfile if down => {
                // Cycles Default -> ... -> last -> back to automatic app matching.
                let next = self.manual.map_or(0, |m| m + 1);
                self.manual = (next < self.cfg.profiles.len()).then_some(next);
                self.refresh_profile();
            }
            Action::Launch(target) if down => {
                if let Err(e) = std::process::Command::new("explorer").arg(&target).spawn() {
                    self.last_error = Some(format!("launch {target}: {e}"));
                }
            }
            _ => {}
        }
    }

    fn rescan(&mut self) {
        self.last_scan = Instant::now();
        // Only worth a process snapshot while sound haptics could actually run.
        let haptics_wanted = self.current.as_ref().is_some_and(|c| c.wheel.haptics.enabled && c.wheel.haptics.pause_with_ghub);
        self.ghub_running = haptics_wanted && procs::is_running(procs::GHUB_AGENT);
        let need = (self.mouse.is_none(), self.light.is_none(), self.wheel.is_none());
        let need_joy = !self.joy_alive.load(Ordering::SeqCst);
        let need_gkeys = !self.gkeys_alive.load(Ordering::SeqCst);
        if need == (false, false, false) && !need_joy && !need_gkeys {
            return;
        }
        let api = match self.api.as_mut() {
            Some(api) => {
                let _ = api.refresh_devices();
                api
            }
            None => match HidApi::new() {
                Ok(api) => self.api.insert(api),
                Err(e) => {
                    self.last_error = Some(format!("HID init failed: {e}"));
                    return;
                }
            },
        };
        let wanted = |k: Kind| match k {
            Kind::Mouse => need.0,
            Kind::Light => need.1,
            Kind::Wheel => need.2,
        };
        for (kind, pid, dev) in discover(api, wanted) {
            log::info!("connected {kind:?} 046D:{pid:04X}");
            match kind {
                Kind::Mouse => {
                    let mut m = Mouse::new(dev);
                    self.mouse_caps = Some(MouseCaps {
                        dpi_values: m.dpi_values().unwrap_or_default(),
                        rates: m.report_rates().unwrap_or_default(),
                        battery: m.battery().ok(),
                        actual_rate: None,
                    });
                    self.applied.dpi = None;
                    self.applied.rate = None;
                    self.mouse = Some(m);
                    self.mouse_pid = pid;
                }
                Kind::Light => {
                    self.applied.light_on = None;
                    self.applied.brightness = None;
                    self.applied.temperature = None;
                    self.light = Some(Litra::new(dev));
                }
                Kind::Wheel => {
                    self.applied.range = None;
                    self.applied.strength = None;
                    self.applied.centering = None;
                    self.applied.rev_mode = None;
                    self.applied.rev_mask = None;
                    let mut w = Wheel::new(dev);
                    // Start from the wheel's real values so untouched axes are left alone.
                    for (i, axis) in WheelAxis::ALL.into_iter().enumerate() {
                        self.applied.sensitivity[i] = w.sensitivity(axis).ok();
                    }
                    self.wheel = Some(w);
                    self.wheel_pid = pid;
                }
            }
        }
        if self.mouse.is_some() && !self.gkeys_alive.load(Ordering::SeqCst) {
            gkeys::spawn(api, self.mouse_pid, self.gkeys_alive.clone());
        }
        if self.wheel.is_some() && !self.joy_alive.load(Ordering::SeqCst) {
            if let Some(j) = Joystick::open(api, self.wheel_pid) {
                joy::spawn(j, self.shared.clone(), self.joy_alive.clone(), self.tx.clone());
            }
        }
    }

    /// Picks up DPI changes made on the mouse itself (G10/G11 or another app), so the UI
    /// shows the real value and we don't fight the change.
    fn read_back(&mut self) {
        let Some(m) = self.mouse.as_mut() else { return };
        if let (Ok(rate), Some(caps)) = (m.report_rate(), self.mouse_caps.as_mut()) {
            // Leaving Endurance mode: put the user's report rate back.
            if rate != ENDURANCE_RATE_MS && Some(rate) != self.applied.rate {
                self.applied.rate = None;
            }
            caps.actual_rate = Some(rate);
        }
        let Ok(dpi) = m.dpi() else { return };
        if self.applied.dpi != Some(dpi) {
            let had = self.applied.dpi.is_some();
            self.applied.dpi = Some(dpi);
            match self.current.as_ref().and_then(|c| c.mouse.dpi_stages.iter().position(|d| *d == dpi)) {
                Some(i) => {
                    self.dpi_stage = i;
                    self.dpi_shift = None;
                }
                // Not one of our stages: hold it until the user picks a stage or profile.
                None => self.dpi_shift = Some(dpi),
            }
            if had {
                self.dpi_popup(dpi);
            }
        }
    }

    /// Briefly shows the new DPI on screen (unless turned off in Settings).
    fn dpi_popup(&self, dpi: u16) {
        if !self.cfg.dpi_popup {
            return;
        }
        let stages = self.current.as_ref().map(|c| c.mouse.dpi_stages.as_slice()).unwrap_or(&[]);
        let detail = match stages.iter().position(|d| *d == dpi) {
            _ if self.dpi_shift == Some(dpi) && !stages.contains(&dpi) => "DPI shift".to_string(),
            Some(i) => format!("Stage {} of {}", i + 1, stages.len()),
            None => String::new(),
        };
        osd::show(format!("{dpi} DPI"), detail);
    }

    fn read_battery(&mut self) {
        self.last_battery = Instant::now();
        if let (Some(m), Some(caps)) = (self.mouse.as_mut(), self.mouse_caps.as_mut()) {
            if let Ok(b) = m.battery() {
                caps.battery = Some(b);
            }
        }
    }

    /// Writes whatever differs between the wanted and the applied state.
    fn sync(&mut self) {
        let Some(r) = self.current.clone() else { return };
        let res = self.sync_mouse(&r.mouse);
        self.check(Kind::Mouse, res);
        let res = self.sync_light(&r.light);
        self.check(Kind::Light, res);
        let res = self.sync_wheel(&r.wheel);
        self.check(Kind::Wheel, res);
    }

    fn sync_mouse(&mut self, s: &MouseSettings) -> hidpp::Result<()> {
        let Some(m) = self.mouse.as_mut() else { return Ok(()) };
        let dpi = self.dpi_shift.or_else(|| s.dpi_stages.get(self.dpi_stage).copied());
        let mut popup = None;
        if let Some(dpi) = dpi.filter(|d| self.applied.dpi != Some(*d)) {
            // The first write after connecting just restores the profile; no popup for that.
            if self.applied.dpi.replace(dpi).is_some() {
                popup = Some(dpi);
            }
            m.set_dpi(dpi)?;
        }
        let supported = self.mouse_caps.as_ref().is_some_and(|c| c.rates.contains(&s.report_rate_ms));
        if supported && self.applied.rate != Some(s.report_rate_ms) {
            self.applied.rate = Some(s.report_rate_ms);
            m.set_report_rate(s.report_rate_ms)?;
        }
        if let Some(dpi) = popup {
            self.dpi_popup(dpi);
        }
        Ok(())
    }

    fn sync_light(&mut self, s: &LightSettings) -> hidpp::Result<()> {
        let Some(l) = self.light.as_mut() else { return Ok(()) };
        if self.applied.brightness != Some(s.brightness) {
            self.applied.brightness = Some(s.brightness);
            l.set_brightness(s.brightness)?;
        }
        if self.applied.temperature != Some(s.temperature) {
            self.applied.temperature = Some(s.temperature);
            l.set_temperature(s.temperature)?;
        }
        if self.applied.light_on != Some(self.light_on) {
            self.applied.light_on = Some(self.light_on);
            l.set_on(self.light_on)?;
        }
        Ok(())
    }

    fn sync_wheel(&mut self, s: &WheelSettings) -> hidpp::Result<()> {
        // Sound haptics: capture runs only while enabled and the wheel is present, and never
        // alongside G HUB's agent, through which games stream real TRUEFORCE.
        let h = s.haptics;
        self.haptics_paused = h.enabled && h.pause_with_ghub && self.ghub_running;
        if h.enabled && self.wheel.is_some() && !self.haptics_paused {
            if self.haptics.as_ref().map(|x| x.band()) != Some(h.band) {
                self.haptics = None; // stop the old capture before starting a new one
                self.haptics = Some(haptics::Haptics::start(h.band, h.strength, self.tx.clone()));
            }
            if let Some(x) = &self.haptics {
                x.set_strength(h.strength);
            }
        } else if self.haptics.take().is_some() {
            log::info!("sound haptics stopped");
        }
        // Tell the wheel input thread what the rev lights should follow.
        let axes = &self.cfg.hardware.axes;
        *self.shared.rev.lock().unwrap() = match s.rev_lights {
            RevLights::Accelerator => RevSource { mode: s.rev_lights, usage: axes.throttle, invert: axes.invert_pedals },
            RevLights::Steering => RevSource { mode: s.rev_lights, usage: axes.steering, invert: false },
            mode => RevSource { mode, usage: 0, invert: false },
        };
        let Some(w) = self.wheel.as_mut() else { return Ok(()) };
        if self.applied.rev_mode != Some(s.rev_lights) {
            self.applied.rev_mode = Some(s.rev_lights);
            self.applied.rev_mask = None;
            w.set_rev_control(s.rev_lights != RevLights::Wheel)?;
            if s.rev_lights != RevLights::Wheel {
                // Live modes start dark until the first input arrives.
                w.set_rev_lights(0)?;
                self.applied.rev_mask = Some(0);
            }
        }
        if self.applied.range != Some(s.range) {
            self.applied.range = Some(s.range);
            w.set_range(s.range)?;
        }
        if self.applied.strength != Some(s.strength) {
            self.applied.strength = Some(s.strength);
            w.set_strength(s.strength)?;
        }
        if self.applied.centering != Some(s.centering) {
            self.applied.centering = Some(s.centering);
            w.set_centering(s.centering)?;
        }
        for (i, (axis, want)) in WheelAxis::ALL.into_iter().zip(s.sensitivity.as_array()).enumerate() {
            if let Some(v) = want.filter(|v| self.applied.sensitivity[i] != Some(*v)) {
                self.applied.sensitivity[i] = Some(v);
                w.set_sensitivity(axis, v)?;
            }
        }
        Ok(())
    }

    /// A HID I/O error means the device is gone; protocol errors are just reported.
    fn check(&mut self, kind: Kind, res: hidpp::Result<()>) {
        match res {
            Ok(()) => {}
            Err(Error::Hid(e)) => {
                log::warn!("{kind:?} disconnected: {e}");
                match kind {
                    Kind::Mouse => {
                        self.mouse = None;
                        self.mouse_caps = None;
                    }
                    Kind::Light => self.light = None,
                    Kind::Wheel => self.wheel = None,
                }
            }
            Err(e) => {
                log::warn!("{kind:?}: {e}");
                self.last_error = Some(format!("{kind:?}: {e}"));
            }
        }
    }

    fn publish(&self) {
        let status = Status {
            mouse: self.mouse_caps.as_ref().filter(|_| self.mouse.is_some()).map(|c| MouseStatus {
                dpi: self.applied.dpi.unwrap_or(0),
                dpi_values: c.dpi_values.clone(),
                report_rate: c.actual_rate.or(self.applied.rate).unwrap_or(0),
                endurance: match (c.actual_rate, self.applied.rate) {
                    (Some(_), Some(ENDURANCE_RATE_MS)) | (None, _) => None,
                    (Some(actual), _) => Some(actual == ENDURANCE_RATE_MS),
                },
                report_rates: c.rates.clone(),
                battery: c.battery.map(|b| (b.percent, b.charging != crate::devices::mouse::Charging::Discharging)),
            }),
            light: self.light.as_ref().map(|_| LightStatus {
                on: self.applied.light_on.unwrap_or(false),
                brightness: self.applied.brightness.unwrap_or(0),
                temperature: self.applied.temperature.unwrap_or(0),
            }),
            wheel: self.wheel.as_ref().map(|_| WheelStatus {
                range: self.applied.range.unwrap_or(0),
                strength: self.applied.strength.unwrap_or(0),
                centering: self.applied.centering.unwrap_or(0),
                sensitivity: self.applied.sensitivity,
            }),
            active_profile: self.active,
            dpi_stage: self.dpi_stage,
            foreground: self.foreground.clone(),
            recent_apps: self.recent.clone(),
            last_error: self.last_error.clone(),
            haptics_paused_for_ghub: self.haptics_paused,
        };
        let mut cur = self.shared.status.lock().unwrap();
        if *cur != status {
            *cur = status;
            drop(cur);
            (self.shared.repaint)();
        }
    }
}
