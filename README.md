# G-Clone

A fast, lightweight replacement for Logitech G HUB, written in Rust.

G-Clone talks to Logitech devices directly over HID++ and ships as a single ~7 MB
executable. The window opens in about 80 ms. When the window is closed, G-Clone keeps
running in the system tray using about 7 MB of RAM and next to no CPU.

> Not affiliated with or endorsed by Logitech. "Logitech", "G HUB", "G602", "G923" and
> "Litra" are trademarks of Logitech.

## Status

Working day to day on the hardware below. See [What's left](#whats-left) for gaps.

| Area | State |
|---|---|
| G602 DPI stages, report rate, battery, live DPI read-back | ✅ |
| G602 button remapping (left/right/middle/G4 = back/G5 = forward) | ✅ |
| G602 extra buttons G6–G11 (read from the receiver's vendor channel) | ✅ use **Detect** once per button |
| Keystrokes recorded by pressing the combination | ✅ |
| Media / browser actions (volume, mute, tracks, browser back / forward / refresh) | ✅ |
| G602 Performance / Endurance switch | ✅ shown live; DPI lights turn green in Endurance |
| G923 range, force-feedback strength, centering spring | ✅ |
| G923 steering and pedal sensitivity (same values as G HUB's sliders) | ✅ |
| G923 rev lights: off, accelerator, steering angle, or left to games | ✅ |
| On-screen DPI popup when DPI changes | ✅ |
| G923 live wheel angle, pedals, buttons + button actions | ✅ |
| Litra Glow power, brightness, colour temperature | ✅ |
| Macros (record / edit, once / while held / toggle) | ✅ |
| Per-app profiles, tray, start with Windows, single instance | ✅ |
| G923 TRUEFORCE, combined pedals | ❌ protocol unknown |

## Features

**Mouse (G602)**
- Top and side views of the mouse with callouts showing each button's assignment.
  Click a button to reprogram it; pressed buttons light up live
- DPI stages (250–2500), report rate (125 / 250 / 500 Hz), battery level
- Reads DPI changes made on the mouse itself, so the UI always shows the real value
- Small on-screen popup with the new DPI and stage whenever it changes, even with the window
  closed (can be turned off in Settings)
- G6–G11 never reach Windows; G-Clone reads them from the receiver's vendor channel (like
  G HUB's filter driver does) so they can run keystrokes, macros, DPI shift and so on

**Racing wheel (G923 for Xbox / PC)**
- Live wheel figure that turns with the real wheel, with the operating-range arc and angle readout
- Live clutch / brake / gas pedal meters, with pedal axis calibration
- Operating range (40–900°), force-feedback strength, centering spring
- Steering, clutch, brake and accelerator sensitivity (0–100, same scale as G HUB). Sliders you
  never move leave the wheel's current value untouched
- Rev (RPM) lights: leave them to the wheel and games, keep them off, or have them fill with
  the accelerator or the steering angle. A Test button runs a sweep; quitting hands them back
  to the wheel
- Wheel buttons can trigger actions (keystrokes, macros, profile switching, …)

**Light (Litra Glow)**
- Power, brightness (20–250 lm) and colour temperature (2700–6500 K)

**Everywhere**
- Assignments: keystrokes with modifiers, mouse buttons, media and browser keys (volume up /
  down, mute, play / pause, next / previous track, browser back / forward / refresh), macros,
  DPI up / down / cycle / shift, light toggle, next profile, launch app or URL
- Keystrokes are recorded by clicking the keystroke and pressing the combination
  (e.g. Ctrl + Win + →). The keys are blocked while recording, so shortcuts don't fire.
  You can still pick a key manually for ones that are hard to press
- Macros: record or edit steps (keys, clicks, delays, text); play once, repeat while held, or toggle
- Per-app profiles that switch automatically when a game or app gets focus
- Tray icon, start with Windows (minimized to tray), single instance

## Supported hardware

| Device | USB ID | Notes |
|---|---|---|
| G602 wireless mouse (via its receiver) | `046D:C537` | HID++ 2.0 at device index 1; G-keys on usage `FF80:0080` |
| Litra Glow | `046D:C900` | Illumination feature `0x1990` |
| G923 Xbox / PC | `046D:C26E` | Force feedback `0x8123`, joystick input for wheel & pedals |
| G923 PlayStation / PC, G920 | `046D:C266`, `046D:C262` | Same protocol, untested |

Other HID++ devices may work with small additions to `crates/core/src/devices`.

## Performance

Measured on the release build (Windows 11, 4K display at 150 %, NVIDIA GPU):

| | |
|---|---|
| Executable | 6.8 MB |
| Open window | ~80 ms warm, ~1.4 s cold |
| Window open, idle | ~0 % CPU; ~80 MB RAM (about half is the GPU driver) |
| Window open, mouse moving over it | capped at 60 fps, ~1.8 ms CPU per frame |
| Tray only (window closed) | ~7 MB RAM, ~0.15 % of one core |

How it stays light:
- The UI repaints only on changes, at most 60 fps. Vsync is off, because NVIDIA's OpenGL driver
  busy-waits on vsync (~15 ms CPU per frame).
- The window unsubscribes from Windows raw mouse input, so mouse movement elsewhere doesn't
  wake it.
- Closing the window destroys the whole UI and GL context and trims the working set.
- The low-level input hooks are installed only while a binding, Detect / recording, or a live
  device page needs them.
- Device threads block on I/O and wake only when there is work to do.

## Building

Requirements: Windows 10/11, Rust (stable, MSVC toolchain) and the Visual Studio C++ Build Tools.

```bash
cargo build --release
```

The executable is `target/release/gclone.exe`. Copy it anywhere.

**Settings location.** G-Clone stores its settings in the folder named by the `GCLONE_DATA`
environment variable, or otherwise in a `data` folder next to the exe. If you run more than one
copy of the exe (say a dev build and an installed one), set `GCLONE_DATA` so they share one set
of settings; otherwise each copy keeps its own and the assignments seem to vanish when you switch.
The Settings page shows the file in use. If the file is ever unreadable it's kept as
`config.toml.invalid-<time>` rather than overwritten.

**Quit G HUB first.** Both apps write device settings, so running them together makes them
fight.

### Developer tools

```bash
cargo run -p gclone-core --example probe                    # dump every Logitech HID collection + HID++ feature table
cargo run -p gclone-core --example status                   # read current settings of all devices
cargo run -p gclone-core --example axes -- 10               # print live wheel / pedal input for 10 s
cargo run -p gclone-core --example listen -- c537 30        # print raw reports from every readable collection
cargo run -p gclone-core --example raw -- c537 2201 2 01    # send one raw HID++ call
cargo test
```

Set `GCLONE_TRACE_REPAINT=1` to log every UI frame together with what caused it.

## How it works

```
crates/core   gclone-core
  hidpp/      HID++ transport: opens the vendor HID collections of a device, routes
              short/long/very-long reports, matches replies, queues notifications
  devices/    mouse, litra, wheel drivers + joystick reader (Windows HidP_* parser)
  engine/     device manager thread (hot-plug, diff-only writes, read-back), on-demand
              low-level input hooks, G-key reader, wheel input thread, macro player,
              foreground-app watcher
  config.rs   profiles / bindings / macros / learned hardware maps as TOML
crates/app    gclone: egui UI, device figures, tray, autostart
```

- **Remapping.** The G602 has no HID++ button diverting, so left/right/middle/back/forward are
  remapped with low-level mouse and keyboard hooks plus `SendInput`. G6–G11 and the mode switch
  come from the receiver's vendor collection (`80 <mask> ..` reports) and go through the same
  binding engine. G4/G5 appear on both paths; G-Clone binds them to the Windows Back/Forward
  signal, because only that one can be blocked and replaced.
- **Live input.** The wheel and pedals are read from the wheel's joystick HID collection in
  shared mode, so games keep working normally.

## What's left

Known limitations, with the planned fix for each:

1. **Hook bindings apply to every mouse and keyboard.** Low-level hooks can't tell which device
   sent an input. *Plan:* use Raw Input with `RIDEV_INPUTSINK` to tag each event with its device
   and only act on G602 events. This only matters for the five standard buttons; G6–G11 are
   already device-specific.
2. **Wheel buttons still reach games.** An assignment adds an action on top of the button.
   *Plan:* document it, and offer "assign only while no game is focused" per profile. Hiding a
   button from games needs a driver.
3. **Button numbers need confirming.** G923 face buttons beyond A/B/X/Y/LB/RB/View/Menu/LSB/RSB/Xbox,
   and G602 G6–G11, need one **Detect** each. *Plan:* ship a confirmed default map once verified,
   so Detect becomes optional.
4. **Mode switch is inferred.** The G602 Performance / Endurance switch only sends a pulse;
   G-Clone infers the mode from the forced 125 Hz report rate. If you choose 125 Hz yourself,
   the mode shows as unknown.
5. **TRUEFORCE and combined pedals are missing.** Sensitivity was decoded by watching the
   wheel's replies to G HUB (undocumented feature `0x80A3`: fn2(axis) → `[0x64, value]`, fn3
   `[axis, 0x64, value]`; pedals stored as 100 − value). The rev lights (`0x807A`) were found by
   careful testing: fn3 `[2]` hands them to the host (`[0]` back to the wheel), then fn6
   `[0, mask]` sets the 5 lights (one bit each). *Plan:* capture G HUB while changing the
   TRUEFORCE and combined-pedals settings. Game telemetry for the rev lights would also be nice.
6. **G602 lighting: not controllable (investigated).** The mouse has one LED zone on an early,
   undocumented version of `0x8070`. It accepts effects 1–5 but nothing changes visibly, and
   G HUB offers no lighting settings for the G602 either. The DPI lights are shown as status
   only (green in Endurance mode).
7. **No installer.** *Plan:* a small installer or a `winget` manifest that puts the exe and its
   `data` folder in one place.
8. **G HUB conflicts.** Both apps fight if G HUB is running. *Plan:* detect `lghub*.exe` and
   offer to quit it.

## Credits

Protocol knowledge comes from the open-source community: the Linux `hid-logitech-hidpp`
driver, [Solaar](https://github.com/pwr-Solaar/Solaar),
[libratbag](https://github.com/libratbag/libratbag) and
[litra-rs](https://github.com/timrogers/litra-rs).
