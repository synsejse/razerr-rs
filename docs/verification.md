# Verification

Run the automated checks inside the development shell:

```sh
nix-shell --run 'cargo test --workspace --locked'
```

This runs the unit, integration and documentation tests. Tests exercise pure
logic, temporary files/databases,
local HTTP image loading, and captures from actual hardware. No fake daemon or
invented devices are used. See [dependency maintenance](dependencies.md) for
transitive maintenance exceptions.

Connect-only lifecycle checks passed with the real GUI and service binaries on
an isolated D-Bus session: opening the GUI leaves an absent service stopped;
closing the window and `razerr-rs quit` leave an independently started service
running; quitting with neither process running succeeds without starting either.
These checks use X11 windows and do not simulate OpenRazer or devices.

## Hardware coverage

Verified on NixOS with KDE Plasma, OpenRazer 3.12.4 and a DeathAdder V3 Pro:

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
