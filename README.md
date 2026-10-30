# G-Clone

A fast, lightweight replacement for Logitech G HUB, written in Rust.

G-Clone talks to Logitech devices directly over HID++ and ships as a single ~7 MB
executable. It starts in about 150 ms, and when its window is closed it keeps running in the
system tray using about 1 MB of RAM and next to no CPU.

> Not affiliated with or endorsed by Logitech. "Logitech", "G HUB", "G602", "G923" and
> "Litra" are trademarks of Logitech.

## Features

**Mouse (G602)**
- Interactive figure of the mouse: click any button to reprogram it, and pressed buttons light up live
- DPI stages (250–2500), report rate (125 / 250 / 500 Hz), battery level
- Reads DPI changes made on the mouse itself, so the UI always shows the real value

**Racing wheel (G923 for Xbox / PC)**
- Live wheel figure that turns with the real wheel, with the operating-range arc and angle readout
- Live clutch / brake / gas pedal meters, with pedal axis calibration
- Operating range (40–900°), force-feedback strength, centering spring
- Wheel buttons can trigger actions (keystrokes, macros, profile switching, …)

**Light (Litra Glow)**
- Power, brightness (20–250 lm) and colour temperature (2700–6500 K)

**Everywhere**
- Button and key remapping: keystrokes with modifiers, mouse buttons, macros, DPI up / down / cycle / shift, light toggle, next profile, launch app or URL
- Macros: record or edit steps (keys, clicks, delays, text); play once, repeat while held, or toggle
- Per-app profiles that switch automatically when a game or app gets focus
- Tray icon, start with Windows (minimized to tray), single instance

## Supported hardware

| Device | USB ID | Notes |
|---|---|---|
| G602 wireless mouse (via its receiver) | `046D:C537` | HID++ 2.0, device index 1 |
| Litra Glow | `046D:C900` | Illumination feature `0x1990` |
| G923 Xbox / PC | `046D:C26E` | Force feedback `0x8123`, joystick input for wheel & pedals |
| G923 PlayStation / PC, G920 | `046D:C266`, `046D:C262` | Same protocol, untested |

Other HID++ devices may work with small additions to `crates/core/src/devices`.

## Building

Requirements: Windows 10/11, Rust (stable, MSVC toolchain) and the Visual Studio C++ Build Tools.

```bash
cargo build --release
```

The executable is `target/release/gclone.exe`. Copy it anywhere. G-Clone stores its settings in a
`data` folder next to the exe; set `GCLONE_DATA` to use a different folder.

**Quit G HUB first.** Both apps write device settings, so running them together makes them
fight.

### Developer tools

```bash
cargo run -p gclone-core --example probe                # dump every Logitech HID collection + HID++ feature table
cargo run -p gclone-core --example status               # read current settings of all devices
cargo run -p gclone-core --example axes -- 10           # print live wheel / pedal input for 10 s
cargo run -p gclone-core --example raw -- c537 2201 2 01   # send one raw HID++ call
cargo test
```

## How it works

```
crates/core   gclone-core
  hidpp/      HID++ transport: opens every vendor HID collection of a device, routes
              short/long/very-long reports, matches replies, queues notifications
  devices/    mouse, litra, wheel drivers + joystick reader (Windows HidP_* parser)
  engine/     device manager thread (hot-plug, diff-only writes), low-level input hooks,
              macro player, wheel input thread, foreground-app watcher
  config.rs   profiles / bindings / macros as TOML
crates/app    gclone: egui UI, device figures, tray, autostart
```

- **Remapping.** The G602 doesn't offer button diverting over HID++, so remapping is done in
  software with low-level mouse and keyboard hooks plus `SendInput`. Hook callbacks only do a
  table lookup, so they add no noticeable latency.
- **Low resource use.** Each device thread wakes only when it has work to do. The UI repaints
  only on changes. Closing the window destroys the whole UI and GL context and trims the
  working set; the tray brings it back on demand.
- **Live input.** The wheel and pedals are read from the wheel's joystick HID collection in
  shared mode, so games keep working normally.

## Known limitations

- Hook-based bindings can't tell which device sent an input, so a binding on e.g. Middle click
  applies to every mouse.
- Wheel buttons still reach games; an assignment adds an action on top of the button.
- G923 RPM LEDs, pedal / wheel sensitivity curves and TRUEFORCE are not implemented yet
  (their HID++ features aren't publicly documented).

## Credits

Protocol knowledge comes from the open-source community: the Linux `hid-logitech-hidpp`
driver, [Solaar](https://github.com/pwr-Solaar/Solaar),
[libratbag](https://github.com/libratbag/libratbag) and
[litra-rs](https://github.com/timrogers/litra-rs).
