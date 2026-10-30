//! Background engine: one manager thread owns the devices, a hook thread handles input,
//! a macro thread plays macros. The UI talks to it through [`Engine`].

mod gkeys;
mod hook;
mod input;
mod joy;
mod macros;

pub use joy::JoyLive;

use crate::config::{Action, Config, LightSettings, Mods, MouseSettings, Resolved, Step, Trigger, WheelSettings};
use crate::devices::{discover, joystick::Joystick, litra::Litra, mouse::{Battery, Mouse}, wheel::Wheel, Kind};
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
}

pub(crate) enum Cmd {
    Config(Box<Config>),
    Foreground(String),
    Action(Action, bool),
    Shutdown,
}

pub(crate) struct Shared {
    status: Mutex<Status>,
    joy: Mutex<Option<JoyLive>>,
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
            repaint: Box::new(repaint),
        });
        let macro_tx = macros::spawn();
        hook::start(hook::Ctx { mgr: tx.clone(), ui: ui_tx, macros: macro_tx.clone(), shared: shared.clone() });

        let mgr = Manager::new(cfg, shared.clone(), macro_tx);
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
                    d.wheel = Some(WheelSettings { range, strength, centering: 0 });
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
    fn new(cfg: Config, shared: Arc<Shared>, macro_tx: Sender<MacroCmd>) -> Self {
        let _ = macro_tx.send(MacroCmd::Set(cfg.macros.clone()));
        Self {
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
                Ok(Cmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
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
                    self.wheel = Some(Wheel::new(dev));
                    self.wheel_pid = pid;
                }
            }
        }
        if self.mouse.is_some() && !self.gkeys_alive.load(Ordering::SeqCst) {
            gkeys::spawn(api, self.mouse_pid, self.gkeys_alive.clone());
        }
        if self.wheel.is_some() && !self.joy_alive.load(Ordering::SeqCst) {
            if let Some(j) = Joystick::open(api, self.wheel_pid) {
                joy::spawn(j, self.shared.clone(), self.joy_alive.clone());
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
            self.applied.dpi = Some(dpi);
            match self.current.as_ref().and_then(|c| c.mouse.dpi_stages.iter().position(|d| *d == dpi)) {
                Some(i) => {
                    self.dpi_stage = i;
                    self.dpi_shift = None;
                }
                // Not one of our stages: hold it until the user picks a stage or profile.
                None => self.dpi_shift = Some(dpi),
            }
        }
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
        if let Some(dpi) = dpi.filter(|d| self.applied.dpi != Some(*d)) {
            self.applied.dpi = Some(dpi);
            m.set_dpi(dpi)?;
        }
        let supported = self.mouse_caps.as_ref().is_some_and(|c| c.rates.contains(&s.report_rate_ms));
        if supported && self.applied.rate != Some(s.report_rate_ms) {
            self.applied.rate = Some(s.report_rate_ms);
            m.set_report_rate(s.report_rate_ms)?;
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
        let Some(w) = self.wheel.as_mut() else { return Ok(()) };
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
            }),
            active_profile: self.active,
            dpi_stage: self.dpi_stage,
            foreground: self.foreground.clone(),
            recent_apps: self.recent.clone(),
            last_error: self.last_error.clone(),
        };
        let mut cur = self.shared.status.lock().unwrap();
        if *cur != status {
            *cur = status;
            drop(cur);
            (self.shared.repaint)();
        }
    }
}
