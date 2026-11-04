# Architecture

razerr-rs is a Cargo workspace for OpenRazer device controls, desktop
notifications and battery history. The GUI and background service communicate
over the session bus; domain logic stays in independent library crates.

The device backend, background service, history store, desktop integration,
settings, image loader and iced GUI are implemented. The
[verification record](verification.md) distinguishes tested behavior from
checks that still need hardware or desktop interaction.

## Processes and ownership

```text
razerr-rs GUI ── session D-Bus ── razerr-rs-service
     │                              ├── device actor ── session D-Bus ── org.razer
     │                              ├── tray and notifications
     │                              └── history writer ──► history.sqlite3
     ├── read-only history.sqlite3
     ├── settings.toml ── file watcher ─────► service settings
     └── bounded image loader and cache

razerr-rs probe ── session D-Bus ── org.razer
```

The service owns device writes and continuous polling. Closing a window does
not transfer those responsibilities to the GUI. The diagnostic `probe`
subcommand is a separate, read-only path: it does not open application settings
or history and never changes OpenRazer's daemon-wide synchronization setting.

| Component | Responsibility |
| --- | --- |
| `crates/model` | Validated domain values, commands, capabilities, readings and snapshots; no I/O, runtime or D-Bus types |
| `crates/openrazer` | Adapter over the `openrazer` library: maps its typed methods and exact-signature introspection to app capabilities, device reads/writes and pure reply interpretation |
| `crates/worker` | Device actor, command coalescing, polling/retries, hotplug observation and cache ownership |
| `crates/ipc` | Native D-Bus snapshot encoding, service interfaces, proxies and command validation at the service boundary |
| `crates/storage` | XDG paths, typed TOML settings, atomic saves and last-valid reloads |
| `crates/metrics` | Private Diesel schema/connections, recording, sessions, retention, history and empirical predictions |
| `crates/images` | Bounded PNG/JPEG loading, HTTP(S) downloads, validated disk cache and RGBA output |
| `crates/desktop` | StatusNotifierItem tray, notification transport and launching sibling executables |
| `crates/ui` | iced state, views, edits, change tracking, service subscriptions, the offline demo, images and read-only history presentation |
| `apps/razerr-rs-service` | Service lifecycle, settings watch, notification policy and the dedicated history thread |
| `apps/razerr-rs` | GUI process and diagnostic CLI entry point |

## Application D-Bus protocol

All application IPC uses the user's session bus. The service owns
`org.razerr_rs.Service`; the GUI application name is `org.razerr_rs.App`.
OpenRazer remains a separate service, `org.razer`. Its external API and device
limitations are documented in [OpenRazer compatibility](openrazer-compatibility.md).

`/org/razerr_rs/Devices` exposes `org.razerr_rs.Devices1`:

| Member | Wire type or input signature | Meaning |
| --- | --- | --- |
| `State` property | `a{sv}` | Snapshot: revision, continuity/daemon generations, kind, devices, reliability, daemon availability, busy state and optional error |
| `Refreshing` property | `b` | Full refresh in progress |
| `Error` signal | `s` | Asynchronous device-operation failure |
| `Refresh()` | No input | Run a full refresh and reply when it ends |
| `SetDpi` | `sqq` | Serial, X and Y |
| `SetDpiStages` | `sya(qq)` | Serial, one-based active stage and stage values |
| `SetPollRate`, `SetIdleTime` | `sq` | Serial and validated value |
| `SetLowBattery` | `sy` | Serial and threshold |
| `SetBrightness` | `ssd` | Serial, zone name and percentage |
| `SetEffect` | `ssa{sv}` | Serial, zone name and typed effect dictionary |

The snapshot uses native numbers, booleans, strings, arrays and nested
dictionaries on the wire. Optional fields are omitted. The codec converts
through Serde values internally; it does not send JSON text in `State`.
Deserialization passes constrained scalar values through their constructors.
The model remains independent of `zbus::wire`.

Property changes use `org.freedesktop.DBus.Properties.PropertiesChanged`.
Revisions let consumers discard repeated snapshots. A device setter replies when
the worker has finished: success means the device was read back, and a failure
arrives as a D-Bus error as well as in the state. A command replaced in the queue
replies with how its replacement ended. Firmware can acknowledge a setter before
applying it, so the worker reads back for up to about 2.2 seconds until the device
shows the value. A write is sent once and never repeated, because a failed or
unconfirmed write may still have reached the device. A value that never appears is
reported; a setting with no fresh reading is not treated as a failure. Clients
need a method timeout above the worst case; `razerr_ipc::Client` uses 45 seconds.
The worker validates again against its current device
state and re-reads USB identity immediately before sending a write to OpenRazer.
The separate identity read and setter cannot be made atomic through upstream
OpenRazer; a changed identity is rejected and followed by discovery.

`/org/razerr_rs/Control` exposes `org.razerr_rs.Control1`: `Open`, `Settings`,
`Refresh`, `ClearHistory`, `Stop`, `Quit` and `Status`. These take no arguments.
`Status` returns a JSON string for command-line inspection; the GUI uses the
typed `State` property. `ClearHistory` acknowledges the database operation's
result. `Stop` shuts down monitoring while preserving an open window; `Quit`
also asks the GUI to quit through `org.freedesktop.Application.ActivateAction`.

## Device actor and readings

A single Tokio actor owns the OpenRazer connection and serializes writes. A
bounded command channel feeds a queue keyed by control, serial and lighting
zone; repeated requests for the same setting coalesce. Surviving commands
retain their last-request order, including interacting DPI and stage writes.
Brightness and effect requests remain separate. Successful writes are read back in full until
the device shows them, so the published value reflects any normalization by the
daemon; see the setter replies above.

Normal live polling runs every 20 seconds. Unreliable discovery and inactive
receivers retry after two seconds. Incomplete battery samples receive two
quick retries before normal polling resumes; a failed required charging read
counts as incomplete even when the percentage is fresh. Hotplug and daemon
owner signals request full refreshes. Periodic enumeration and USB identity
checks also detect changes when signals are missed. A changed transport or a
receiver waking up triggers capability discovery again.

Live reads cover USB identity, DPI, polling rate, battery/charging, DPI stages
and low-battery threshold. Full reads additionally discover capabilities and
read identity metadata, supported ranges, power settings and lighting state.
Capabilities require matching input and output signatures. Unknown remembered
effects or invalid parameters leave the selection unavailable.

`Reading<T>` distinguishes `Fresh`, `Stale` and `Unavailable`. Failed reads
may retain a display value only for the same known USB transport. Battery
display caching expires after 120 seconds and never supplies a training
sample. A successful zero-DPI reply identifies an inactive receiver; a failed
read does not prove a disconnected receiver has returned. Failed discovery
preserves known devices with an unreliable snapshot, rather than interpreting
the failure as physical removal.

Snapshots are published through a Tokio watch channel as `Full`, `Live` or
`Ack`. Busy/error acknowledgements do not count as observations. A separate
broadcast channel carries errors. Monotonic continuity and daemon-generation
counters carry missed failures/owner changes through coalesced snapshots. The
history writer breaks continuity and the monitor resets its baseline from
these counters, even if the intermediate unavailable value was overwritten.
OpenRazer calls have bounded timeouts. The adapter opts into D-Bus activation
to preserve startup behavior when a daemon activation service is installed;
aborting the actor also aborts its signal-listener task.

## History and predictions

The service sends observations and settings changes to one dedicated history
thread. Its synchronous Diesel connection never blocks the async service
loop. Only fresh battery levels, known charging state, known USB identity and
fresh supported polling context become `Sample` values. Unsupported polling
is represented by zero, without inventing a rate.

Recording throttles ordinary samples and splits sessions on context changes,
missing observations, unreliable discovery, wall-clock discontinuities and
suspend-sized gaps. Unchanged readbacks within one wall-clock second are
deduplicated without resetting learning anchors. Database processing failures
also break continuity.
Disabling history closes the writer and stops collection; clearing remains
available while collection is disabled. History settings changes and re-enabling
collection start fresh in-memory session state.

`razerr-metrics` exposes plain `Sample`, `Point`, `Prediction`, `Usage` and
`Store` APIs. Diesel rows and connections stay private. Predictions use actual
intervals for the same serial and USB transport, prefer exact polling context,
and can use nearby rate contexts when evidence is sufficient. Charging is
estimated across separate charge bands. Evidence thresholds, outlier handling,
confidence and ranges are pure functions; insufficient evidence returns no
estimate. History queries are bounded and downsample real observations while
retaining session identifiers for chart gaps.

The service is the normal database writer. GUI history work runs in blocking
tasks using read-only connections. Readers never create or migrate databases.
Writers use WAL, a busy timeout and transactional embedded migrations; unknown
newer schema versions are rejected. See [migrations](migrations.md) for the
schema contract, rollback behavior and regeneration workflow.

## Settings, assets and images

| Data | Location |
| --- | --- |
| Settings | `$XDG_CONFIG_HOME/razerr-rs/settings.toml` |
| History | `$XDG_STATE_HOME/razerr-rs/history.sqlite3` |
| Image cache | `$XDG_CACHE_HOME/razerr-rs/images/` |

Paths are resolved once and passed explicitly. Application directories are
private; settings and database files are created with mode `0600`. Settings
saves validate the whole document, preserve comments and unknown keys, and
atomically replace the file. The service watches edits, replacements and
deletion with a short debounce, ignoring access events from its own reads.
A second startup read after watcher registration closes the startup edit race.
Invalid reloads retain the last valid settings. Missing settings restore
defaults. The value ranges and TOML format are described in [settings](settings.md).

Fonts and the window icon are embedded, so rendering does not depend on
runtime asset lookup or the working directory. Desktop integration installs
the application icon separately; asset license notices accompany installations.
The image loader accepts absolute local paths and HTTP(S) URLs, decodes only
PNG/JPEG, and returns RGBA pixels. Encoded data is capped at 8 MiB, each axis at
4096 pixels and total pixels at 4,194,304. Downloads have connect/overall
timeouts and a redirect limit. URL-keyed cache entries are decoded before use;
corrupt entries are refetched, while valid cached images work offline. Dropping
the load future cancels network work; bounded file/decode tasks may finish on
the blocking pool.

## Desktop lifecycle and GUI integration

The service obtains its well-known bus name without replacing an existing
owner. A second normal service process exits while the original keeps running;
CLI subcommands forward to that owner without auto-starting an absent service. The systemd user unit is `Type=dbus`
and restarts failures. An unexpected worker exit makes the service exit with
failure after cleanup. Normal shutdown stops the settings watcher and device
actor, drains and joins the history thread, and shuts down tray/notifications.

The tray uses StatusNotifierItem through ksni and remains available to
re-register when the desktop watcher returns. Notification policy lives in the
service: quiet initial baseline, three-second connection debounce, transport
handoff handling, battery hysteresis and mute acknowledgement. Desktop code
handles transport, tracked notification IDs and default-click actions. Failed
battery delivery remains eligible for a later retry. GUI launches resolve an
executable sibling directly, without a shell or PATH lookup, and reap the
child separately.

GUI integration uses plain iced widgets and local draft state. Its service
subscription obtains snapshots and asynchronous errors; commands use the IPC
client. History/prediction work uses the read-only metrics API, and pictures
use the bounded image loader. Settings drafts save through storage. The GUI
does not own polling, device writes, the tray or history collection. IPC clients
bind to an existing unique service owner, so connecting cannot indirectly
activate an installed service. Packaging and the service manager own service
startup and lifetime; the GUI reconnects when the service becomes available.
The GUI registers `org.freedesktop.Application` with Activate and
settings/quit actions. Closing or quitting the GUI leaves the service
running. Explicit service CLI and tray Stop/Quit actions remain available.
Native rendering and lifecycle checks are recorded in
[verification](verification.md).

Views are split by screen/control group. A changed USB identity resets device
editor drafts, even when the serial stays the same and a new getter is unavailable. `iced_t3` adds Tab and Enter/Space support to buttons and switches, and sliders
use text alternatives with Escape cancelling a pending drag; it has widget-tree
event tests for these. Confirmations initially focus Cancel and trap Tab until dismissal.

The design comes from the pinned `iced_t3` Git dependency: theme, fonts,
widgets and client-side window decorations. Anything generic that the app needs
belongs there rather than here. `ring.rs`, `dpi_pad.rs` and `chart.rs` hold the
canvas programs specific to this app: the battery ring, the DPI vector pad and the
battery history chart, whose line is a monotone curve (`curve.rs`) that can't show a level
that wasn't measured. Canvas gradients use opaque stops, because translucent ones
show a hard step under MSAA on some GPUs.

The sidebar is navigation with the device at its foot. The Sensitivity page has
the active stage, polling rate, and DPI editor. All three edit local drafts;
the sensitivity Apply action sends them together. The pad shows a sensitivity as a point (X DPI, Y DPI) on logarithmic axes,
with every stage as a numbered vector. It edits one target at a time (the current
DPI or one stage), the same target the separate X/Y sliders and inputs edit. The
lock between the rows constrains drags to the diagonal.

Every setting on the device pages is a draft, and so are Preferences. One docked
bar serves them all: it appears only when it has something to report, with Revert
and the page's action beside the status. Apply sends the section's validated commands in order, stopping at a
write failure (nothing is sent if any draft is invalid). Once the device confirms,
its reported values replace the drafts, even if it normalized them.

`activity.rs` tracks the one change in flight and which control it belongs to.
That control shows progress and the title bar names the action, then both show a
check for 1.6 seconds. Other controls keep their look and ignore input until the
device answers, and selections only ever show what the device reported. The app
talks to a `Backend`: the service, or for `--demo` an in-process stand-in in
`demo.rs` that uses no D-Bus and no files.

On Wayland, iced/winit does not expose xdg-activation tokens: a second launch
updates the existing window, but the compositor may keep it behind other
windows. X11 raising is supported. This upstream focus limitation is not
worked around with unsafe code.

## Development and verification boundaries

Workspace code forbids unsafe Rust. Libraries use `thiserror`; binaries add
context with `anyhow`. Dependencies and lints are centralized, versions are
pinned and `Cargo.lock` is checked in. Development uses `shell.nix` and its
nixpkgs Rust toolchain. Build and test directly with Cargo; see
[development](development.md) for the local commands.

Pure policy tests cover interpretation, caching, scheduling, notification
rules and predictions. File/database tests use real temporary files; image
HTTP tests use a local fixture server. Introspection fixtures are captured
from actual hardware with provenance. Writes can't be tested on a real mouse, so
`crates/worker/tests/fake_daemon.rs` runs the worker against a fake OpenRazer on a
private bus, and `crates/worker/tests/library_adapter.rs` does the same for the
adapter's wire shapes, write guards and signal-driven reconnection; both are
ignored by default and refuse the desktop session bus. The
demo devices in `crates/ui/fixtures` are made up and labelled as such. Real service and device
checks are recorded in [verification](verification.md). Dependency maintenance
notices and narrow advisory exceptions are documented in
[dependencies](dependencies.md).

DPI-stage and charging read failures get one bounded retry after 150 ms before
publication. Invalid stage data remains unavailable (or cached for the same USB
identity); zero DPI values are never accepted. Missing stage/charging readings
get two quick polling retries, then back off. The UI receives concise read
errors; full daemon diagnostics are retained in service logs.
