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
| G602 button remapping (left/right/middle/back/forward) | ✅ |
| G602 extra buttons G6–G11 (read from the receiver's vendor channel) | ✅ use **Detect** once per button |
| G602 Performance / Endurance switch | ✅ shown live (Endurance = mouse held at 125 Hz) |
| G923 range, force-feedback strength, centering spring | ✅ |
| G923 live wheel angle, pedals, buttons + button actions | ✅ |
| Litra Glow power, brightness, colour temperature | ✅ |
| Macros (record / edit, once / while held / toggle) | ✅ |
| Per-app profiles, tray, start with Windows, single instance | ✅ |
| Wheel / pedal sensitivity curves, RPM LEDs, TRUEFORCE | ❌ protocol unknown |

## Features

**Mouse (G602)**
- Top and side views of the mouse with callouts showing each button's assignment.
  Click a button to reprogram it; pressed buttons light up live
- DPI stages (250–2500), report rate (125 / 250 / 500 Hz), battery level
- Reads DPI changes made on the mouse itself, so the UI always shows the real value
- G6–G11 never reach Windows; G-Clone reads them from the receiver's vendor channel (like
  G HUB's filter driver does) so they can run keystrokes, macros, DPI shift and so on

**Racing wheel (G923 for Xbox / PC)**
- Live wheel figure that turns with the real wheel, with the operating-range arc and angle readout
- Live clutch / brake / gas pedal meters, with pedal axis calibration
- Operating range (40–900°), force-feedback strength, centering spring
- Wheel buttons can trigger actions (keystrokes, macros, profile switching, …)

**Light (Litra Glow)**
- Power, brightness (20–250 lm) and colour temperature (2700–6500 K)

**Everywhere**
- Assignments: keystrokes with modifiers, mouse buttons, macros, DPI up / down / cycle / shift,
  light toggle, next profile, launch app or URL
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

The executable is `target/release/gclone.exe`. Copy it anywhere. G-Clone stores its settings in a
`data` folder next to the exe; set `GCLONE_DATA` to use a different folder.

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
  binding engine.
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
5. **Sensitivity curves, RPM LEDs and TRUEFORCE are missing.** The HID++ features (`0x8127`,
   `0x807A`) aren't publicly documented. *Plan:* capture G HUB's traffic for these features
   with a USB sniffer, then implement them.
6. **Lighting effects on the G602 (`0x8070`) aren't exposed.** *Plan:* add DPI-LED brightness /
   effect controls once mapped.
7. **No installer.** *Plan:* a small installer or a `winget` manifest that puts the exe and its
   `data` folder in one place.
8. **G HUB conflicts.** Both apps fight if G HUB is running. *Plan:* detect `lghub*.exe` and
   offer to quit it.

## Credits

Protocol knowledge comes from the open-source community: the Linux `hid-logitech-hidpp`
driver, [Solaar](https://github.com/pwr-Solaar/Solaar),
[libratbag](https://github.com/libratbag/libratbag) and
[litra-rs](https://github.com/timrogers/litra-rs).
