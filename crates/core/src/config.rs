//! User configuration: profiles, bindings and macros, stored as TOML.
//!
//! Profile 0 is always "Default" and has every section set. Other profiles may leave a
//! section as `None`, meaning "use the Default profile's settings".

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButton {
    pub const ALL: [MouseButton; 5] = [Self::Left, Self::Right, Self::Middle, Self::Back, Self::Forward];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left click",
            Self::Right => "Right click",
            Self::Middle => "Middle click",
            Self::Back => "Back (side)",
            Self::Forward => "Forward (side)",
        }
    }
}

/// Physical input that a binding reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trigger {
    Mouse(MouseButton),
    Key(u16),
    /// Game-controller button (1-based HID button number) on the wheel.
    Joy(u16),
    /// Extra mouse button reported only on the receiver's vendor channel (bit number);
    /// Windows never sees these (G602 G6–G11).
    GKey(u8),
}

/// Physical G602 buttons shown on the mouse figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MousePart {
    Left,
    Right,
    Middle,
    G4,
    G5,
    G6,
    G7,
    G8,
    G9,
    G10,
    G11,
}

impl MousePart {
    pub const ALL: [MousePart; 11] = [
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::G4,
        Self::G5,
        Self::G6,
        Self::G7,
        Self::G8,
        Self::G9,
        Self::G10,
        Self::G11,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left click",
            Self::Right => "Right click",
            Self::Middle => "Middle click",
            Self::G4 => "G4",
            Self::G5 => "G5",
            Self::G6 => "G6",
            Self::G7 => "G7",
            Self::G8 => "G8",
            Self::G9 => "G9",
            Self::G10 => "G10",
            Self::G11 => "G11",
        }
    }

    /// What the button sends out of the box, where known.
    pub fn default_trigger(self) -> Option<Trigger> {
        match self {
            Self::Left => Some(Trigger::Mouse(MouseButton::Left)),
            Self::Right => Some(Trigger::Mouse(MouseButton::Right)),
            Self::Middle => Some(Trigger::Mouse(MouseButton::Middle)),
            Self::G4 => Some(Trigger::Mouse(MouseButton::Back)),
            Self::G5 => Some(Trigger::Mouse(MouseButton::Forward)),
            _ => None,
        }
    }
}

/// Which joystick usage (HID Generic Desktop usage id) drives each wheel control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisMap {
    pub steering: u16,
    pub throttle: u16,
    pub brake: u16,
    pub clutch: u16,
    /// Logitech pedals report 255 when released.
    pub invert_pedals: bool,
}

impl Default for AxisMap {
    fn default() -> Self {
        Self { steering: 0x30, throttle: 0x31, brake: 0x32, clutch: 0x35, invert_pedals: true }
    }
}

/// Buttons on the G923 (Xbox) wheel face shown on the wheel figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WheelPart {
    A,
    B,
    X,
    Y,
    LB,
    RB,
    View,
    Menu,
    LSB,
    RSB,
    Xbox,
    Plus,
    Minus,
    Enter,
    DialLeft,
    DialRight,
}

impl WheelPart {
    pub const ALL: [WheelPart; 16] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::LB,
        Self::RB,
        Self::View,
        Self::Menu,
        Self::LSB,
        Self::RSB,
        Self::Xbox,
        Self::Plus,
        Self::Minus,
        Self::Enter,
        Self::DialLeft,
        Self::DialRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::LB => "LB (left paddle)",
            Self::RB => "RB (right paddle)",
            Self::View => "View",
            Self::Menu => "Menu",
            Self::LSB => "LSB",
            Self::RSB => "RSB",
            Self::Xbox => "Xbox",
            Self::Plus => "+",
            Self::Minus => "−",
            Self::Enter => "Enter (dial)",
            Self::DialLeft => "Dial left",
            Self::DialRight => "Dial right",
        }
    }

    /// HID button number, following the Linux G920 mapping; others must be detected.
    pub fn default_button(self) -> Option<u16> {
        match self {
            Self::A => Some(1),
            Self::B => Some(2),
            Self::X => Some(3),
            Self::Y => Some(4),
            Self::RB => Some(5),
            Self::LB => Some(6),
            Self::Menu => Some(7),
            Self::View => Some(8),
            Self::RSB => Some(9),
            Self::LSB => Some(10),
            Self::Xbox => Some(11),
            _ => None,
        }
    }
}

/// Facts about the physical hardware, shared by all profiles.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Hardware {
    /// What each G602 button actually sends, learned with "Detect" (overrides defaults).
    #[serde(default)]
    pub mouse_map: Vec<(MousePart, Trigger)>,
    #[serde(default)]
    pub axes: AxisMap,
    /// Custom labels for wheel buttons.
    #[serde(default)]
    pub wheel_button_names: Vec<(u16, String)>,
    /// Which HID button each wheel-face part sends, learned with "Detect".
    #[serde(default)]
    pub wheel_map: Vec<(WheelPart, u16)>,
}

impl Hardware {
    /// Buttons that also reach Windows (left/right/middle, G4 = Back, G5 = Forward) always use
    /// their Windows trigger: only that one can be swallowed, so a remap fully replaces it.
    /// Their G-key bit, if learned, is ignored.
    pub fn trigger_of(&self, part: MousePart) -> Option<Trigger> {
        part.default_trigger().or_else(|| self.mouse_map.iter().find(|(p, _)| *p == part).map(|(_, t)| *t))
    }

    pub fn set_trigger(&mut self, part: MousePart, trigger: Trigger) {
        self.mouse_map.retain(|(p, _)| *p != part);
        self.mouse_map.push((part, trigger));
    }

    pub fn wheel_button_name(&self, n: u16) -> String {
        if let Some((_, s)) = self.wheel_button_names.iter().find(|(b, _)| *b == n) {
            return s.clone();
        }
        match WheelPart::ALL.iter().find(|p| self.button_of(**p) == Some(n)) {
            Some(p) => p.label().to_string(),
            None => format!("Button {n}"),
        }
    }

    pub fn button_of(&self, part: WheelPart) -> Option<u16> {
        self.wheel_map.iter().find(|(p, _)| *p == part).map(|(_, b)| *b).or(part.default_button())
    }

    pub fn set_button(&mut self, part: WheelPart, button: u16) {
        self.wheel_map.retain(|(p, _)| *p != part);
        self.wheel_map.push((part, button));
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Disabled,
    Key {
        vk: u16,
        #[serde(default)]
        mods: Mods,
    },
    Mouse(MouseButton),
    /// Media / volume key (virtual-key code, see [`MEDIA_KEYS`]).
    Media(u16),
    Macro(String),
    DpiUp,
    DpiDown,
    DpiCycle,
    /// Temporarily use this DPI while held (sniper button).
    DpiShift(u16),
    LightToggle,
    NextProfile,
    /// Opens a program, file or URL.
    Launch(String),
}

/// Media actions offered in the UI: (virtual-key code, label).
pub const MEDIA_KEYS: [(u16, &str); 6] = [
    (0xAF, "Volume up"),
    (0xAE, "Volume down"),
    (0xAD, "Mute"),
    (0xB3, "Play / Pause"),
    (0xB0, "Next track"),
    (0xB1, "Previous track"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub trigger: Trigger,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MouseSettings {
    pub dpi_stages: Vec<u16>,
    pub default_stage: usize,
    pub report_rate_ms: u8,
}

impl Default for MouseSettings {
    fn default() -> Self {
        Self { dpi_stages: vec![500, 1000, 1500, 2500], default_stage: 2, report_rate_ms: 2 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightSettings {
    pub on: bool,
    pub brightness: u16,
    pub temperature: u16,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self { on: false, brightness: 100, temperature: 4500 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WheelSettings {
    pub range: u16,
    pub strength: u8,
    pub centering: u8,
}

impl Default for WheelSettings {
    fn default() -> Self {
        Self { range: 900, strength: 100, centering: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Executable names (e.g. `forza_horizon5.exe`) that activate this profile.
    #[serde(default)]
    pub apps: Vec<String>,
    pub mouse: Option<MouseSettings>,
    pub light: Option<LightSettings>,
    pub wheel: Option<WheelSettings>,
    pub bindings: Option<Vec<Binding>>,
}

impl Profile {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), apps: Vec::new(), mouse: None, light: None, wheel: None, bindings: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroMode {
    /// Play once per press.
    Once,
    /// Repeat while the button is held.
    WhileHeld,
    /// Press to start repeating, press again to stop.
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Step {
    KeyDown(u16),
    KeyUp(u16),
    KeyTap(u16),
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    Click(MouseButton),
    Delay(u32),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub name: String,
    pub mode: MacroMode,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub macros: Vec<Macro>,
    /// When set, this profile stays active regardless of the focused app.
    #[serde(default)]
    pub pinned_profile: Option<String>,
    #[serde(default)]
    pub start_hidden: bool,
    #[serde(default)]
    pub hardware: Hardware,
}

impl Default for Config {
    fn default() -> Self {
        let mut default = Profile::new("Default");
        default.mouse = Some(MouseSettings::default());
        default.light = Some(LightSettings::default());
        default.wheel = Some(WheelSettings::default());
        default.bindings = Some(Vec::new());
        Self {
            profiles: vec![default],
            macros: Vec::new(),
            pinned_profile: None,
            start_hidden: false,
            hardware: Hardware::default(),
        }
    }
}

/// A profile with every `None` section filled in from the Default profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub mouse: MouseSettings,
    pub light: LightSettings,
    pub wheel: WheelSettings,
    pub bindings: Vec<Binding>,
}

impl Config {
    pub fn resolved(&self, index: usize) -> Resolved {
        let d = &self.profiles[0];
        let p = self.profiles.get(index).unwrap_or(d);
        Resolved {
            mouse: p.mouse.clone().or_else(|| d.mouse.clone()).unwrap_or_default(),
            light: p.light.clone().or_else(|| d.light.clone()).unwrap_or_default(),
            wheel: p.wheel.clone().or_else(|| d.wheel.clone()).unwrap_or_default(),
            bindings: p.bindings.clone().or_else(|| d.bindings.clone()).unwrap_or_default(),
        }
    }

    /// Index of the first profile listing `exe`, or 0 (Default).
    pub fn profile_for_app(&self, exe: &str) -> usize {
        self.profiles
            .iter()
            .position(|p| p.apps.iter().any(|a| a.eq_ignore_ascii_case(exe)))
            .unwrap_or(0)
    }

    pub fn path() -> PathBuf {
        data_dir().join("config.toml")
    }

    pub fn load() -> Option<Self> {
        let text = std::fs::read_to_string(Self::path()).ok()?;
        match toml::from_str::<Config>(&text) {
            Ok(mut c) => {
                c.normalize();
                Some(c)
            }
            Err(e) => {
                log::error!("config.toml is invalid, starting fresh: {e}");
                None
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        // Write-then-rename so a crash never leaves a half-written config.
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    /// Guarantees the invariants the rest of the app relies on.
    fn normalize(&mut self) {
        if self.profiles.is_empty() {
            *self = Self::default();
            return;
        }
        // Older configs learned G-key bits for buttons that also reach Windows (G4/G5): move
        // bindings on those bits to the Windows trigger, which can be fully replaced.
        let moved: Vec<(Trigger, Trigger)> = self
            .hardware
            .mouse_map
            .iter()
            .filter_map(|(part, learned)| part.default_trigger().map(|os| (*learned, os)))
            .collect();
        for profile in &mut self.profiles {
            for b in profile.bindings.iter_mut().flatten() {
                if let Some((_, os)) = moved.iter().find(|(learned, _)| *learned == b.trigger) {
                    b.trigger = *os;
                }
            }
        }
        self.hardware.mouse_map.retain(|(part, _)| part.default_trigger().is_none());

        let fallback = Self::default().profiles.remove(0);
        let d = &mut self.profiles[0];
        d.mouse.get_or_insert_with(|| fallback.mouse.unwrap());
        d.light.get_or_insert_with(|| fallback.light.unwrap());
        d.wheel.get_or_insert_with(|| fallback.wheel.unwrap());
        d.bindings.get_or_insert_with(Vec::new);
    }
}

/// Where config and logs live: `$GCLONE_DATA`, else a `data` folder next to the exe
/// (keeps everything on the drive the app is installed on).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("GCLONE_DATA") {
        return d.into();
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("data")))
        .unwrap_or_else(|| PathBuf::from("data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_round_trip() {
        let mut c = Config::default();
        c.profiles[0].bindings = Some(vec![
            Binding { trigger: Trigger::Mouse(MouseButton::Back), action: Action::DpiShift(400) },
            Binding { trigger: Trigger::Key(0x70), action: Action::Key { vk: 0x41, mods: Mods { ctrl: true, ..Default::default() } } },
            Binding { trigger: Trigger::Mouse(MouseButton::Middle), action: Action::Disabled },
            Binding { trigger: Trigger::Joy(5), action: Action::LightToggle },
        ]);
        c.hardware.set_trigger(MousePart::G7, Trigger::Key(0x31));
        c.hardware.wheel_button_names.push((5, "RB".into()));
        let mut p = Profile::new("Racing");
        p.apps.push("forza.exe".into());
        p.wheel = Some(WheelSettings { range: 540, strength: 80, centering: 20 });
        c.profiles.push(p);
        c.macros.push(Macro {
            name: "hello".into(),
            mode: MacroMode::Once,
            steps: vec![Step::KeyTap(0x48), Step::Delay(20), Step::Text("hi".into()), Step::Click(MouseButton::Left)],
        });
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(c, back);
        assert_eq!(back.hardware.trigger_of(MousePart::G7), Some(Trigger::Key(0x31)));
        assert_eq!(back.hardware.trigger_of(MousePart::G4), Some(Trigger::Mouse(MouseButton::Back)));
        assert_eq!(back.profile_for_app("FORZA.EXE"), 1);
        assert_eq!(back.resolved(1).mouse, back.resolved(0).mouse);
        assert_eq!(back.resolved(1).wheel.range, 540);
    }

    #[test]
    fn learned_gkey_for_g4_moves_to_back() {
        let mut c = Config::default();
        c.hardware.set_trigger(MousePart::G4, Trigger::GKey(3));
        c.hardware.set_trigger(MousePart::G7, Trigger::GKey(6));
        c.profiles[0].bindings = Some(vec![
            Binding { trigger: Trigger::GKey(3), action: Action::Media(0xAF) },
            Binding { trigger: Trigger::GKey(6), action: Action::DpiUp },
        ]);
        c.normalize();
        let b = c.profiles[0].bindings.as_ref().unwrap();
        assert_eq!(b[0].trigger, Trigger::Mouse(MouseButton::Back));
        assert_eq!(b[1].trigger, Trigger::GKey(6));
        assert_eq!(c.hardware.trigger_of(MousePart::G7), Some(Trigger::GKey(6)));
    }
}
