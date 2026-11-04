# Verification

Run the automated checks inside the development shell:

```sh
nix-shell --run 'cargo test --workspace --locked'
```

This runs the unit, integration and documentation tests. Tests exercise pure
logic, temporary files/databases, local HTTP image loading, captures from actual
hardware, and the views rendered and clicked without a window. See
[dependency maintenance](dependencies.md) for transitive maintenance exceptions.

Two things are simulated, and neither can reach a device. `--demo` and the render
tests use the made-up devices in `crates/ui/fixtures`. The write path test, ignored
by default, runs the real worker against a fake `org.razer` on a private bus; see
[development](development.md#write-path-test).

Connect-only lifecycle checks passed with the real GUI and service binaries on
an isolated D-Bus session: opening the GUI leaves an absent service stopped;
closing the window and `razerr-rs quit` leave an independently started service
running; quitting with neither process running succeeds without starting either.
These checks use X11 windows and do not simulate OpenRazer or devices.

## Interface checks

Checked on 2026-11-04 with an RX 9060 XT (RADV), in a private headless sway:

- Rendering, including the chart gradient under MSAA, the effect list and dialogs.
- Writes through the real GUI, service and D-Bus against a fake `org.razer`: a late
  apply stays in progress until the readback, a failed setter leaves the selection
  alone, each write is sent once, and killing the service mid-write leaves no
  control stuck and repeats nothing. The app recovers when the service returns.
- Moving the window by the sidebar header and title bar, resizing from a corner and
  an edge, and closing it, which leaves the service running.

Under Xvfb with openbox, in software: maximize and restore by button and by
double-click, minimize, a second launch restoring a minimized window, Tab focus,
and `razerr-rs quit`. Tiny-skia dims a dialog a little more on each focus change;
this doesn't happen on the GPU.

A real device's snapshot was also rendered through the harness. The redesigned GUI
has not yet changed a setting on real hardware.

## Hardware coverage

Verified on NixOS with KDE Plasma, OpenRazer 3.12.4 and a DeathAdder V3 Pro, before
the iced_t3 redesign:

- Wired (`1532:00c2`, 1000 Hz) and wireless (`1532:00c3`, 8000 Hz) discovery,
  live state, DPI/stages, battery/charging and the high-polling warning.
- Physical handoff, full unplug/replug, connection notifications and automatic
  recovery, with history sessions separated across transport changes and gaps.
- Service restart, input validation, a same-value DPI write, settings reloads,
  history controls and tray registration.
- Wayland/X11 rendering, themes, single-instance forwarding, debug/release
  builds. Earlier staged installations were also checked.

The user confirmed the handoff display and notifications on 2026-10-29.
Transient invalid OpenRazer stage replies recovered automatically. Captured
snapshots guard against editor drafts surviving a transport change; their
provenance is kept beside the [fixtures](../crates/ui/tests/captures/README.md).

## Remaining coverage and limitations

Lighting/keyboard devices, discrete single-axis DPI, low/critical battery
notifications, notification clicks, panel restart, login activation and long
battery estimates still need hands-on coverage. Native keyboard automation
was gated by KDE permissions; widget event tests cover keyboard behavior.

On Wayland, iced/winit cannot reliably raise an existing window through its
exposed API. Activation updates the window, but the compositor may leave it
behind another window. X11 raising is supported.
