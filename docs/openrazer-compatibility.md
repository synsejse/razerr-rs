# OpenRazer compatibility

This is the external protocol specification for `razerr-openrazer`, adapted
from Chromaven's audit of official OpenRazer commit
[`477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d`](https://github.com/openrazer/openrazer/tree/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d).
The pinned sources describe the audited API, not a claim about every subsequent
OpenRazer release. Each device's actual introspection determines its supported
methods and signatures.

razerr-rs talks to the session-bus daemon. It does not access USB or sysfs
directly. The probe reads once; the service owns discovery, polling, retries,
cached display values and device writes. The GUI sends validated commands to
the service over a separate session-bus interface.

## Discovery and identity

| Area | Contract |
| --- | --- |
| Discovery | Bus `org.razer`, path `/org/razer`, interface `razer.devices`, method `getDevices() → as`. Device signals `device_added` and `device_removed` have no arguments. The worker enumerates on these signals and daemon ownership changes. [Source][daemon] |
| Device object | `/org/razer/device/<serial>`. The serial must form a valid D-Bus object-path component. [Source][device] |
| Identity | `razer.device.misc` exposes `getDeviceName()`, `getDeviceType()`, `getFirmware()` and `getDriverVersion()`, each returning `s`; `getVidPid() → ai` returns the two USB identifiers. [Source][device] |
| Images | `getDeviceImage() → s`; older daemons use `getRazerUrls() → s`, with an image source in the JSON `top_img` field. A probe reads the source string without downloading the image. [Source][device] |

Serial identity is distinct from USB transport identity. A changed VID/PID for
the same serial is a transport handoff; cached battery and optional settings
must not be copied blindly to the new transport. The Rust device list has no
inherited 16-device limit.

## Sensitivity, polling and power

| Interface | Member | D-Bus signature and meaning |
| --- | --- | --- |
| `razer.device.dpi` | `getDPI` | No input, `ai` output; a zero X reading can indicate an inactive wireless receiver |
| `razer.device.dpi` | `maxDPI` | No input, `i` output |
| `razer.device.dpi` | `availableDPI` | Optional; no input, `ai` output; advertised discrete X values |
| `razer.device.dpi` | `setDPI` | `qq` input, no output |
| `razer.device.dpi` | `getDPIStages` | No input, **one struct** `(ya(qq))` output |
| `razer.device.dpi` | `setDPIStages` | **Two arguments** `ya(qq)` input, no output |
| `razer.device.misc` | `getPollRate` | No input, `i` output, in Hz |
| `razer.device.misc` | `getSupportedPollRates` | Optional; no input, `aq` output |
| `razer.device.misc` | `setPollRate` | `q` input, no output |
| `razer.device.power` | `getBattery` | No input, `d` output, in percent; `-1` means unavailable |
| `razer.device.power` | `isCharging` | No input, `b` output |
| `razer.device.power` | `getIdleTime` / `setIdleTime` | `q` output / input, in seconds; allowed setting 60–900 |
| `razer.device.power` | `getLowBatteryThreshold` / `setLowBatteryThreshold` | `y` output / input, in percent; allowed setting 5–25 |

These signatures come from the [mouse methods][mouse-methods], the
[base device][device] and the [mouse driver][mouse-driver]. Power-setting ranges
are also enforced by the [common driver commands][common-driver].

- Devices advertising `availableDPI` accept only advertised X values and
  require Y=0 when setting DPI. This includes the DeathAdder 3.5G and Black
  Edition. An initial failed list read leaves the values unknown; the
  worker may preserve a previous valid list only for the same USB identity.
  [Client][mouse-client], [models][mouse-models].
- DPI-stage active indexes are **1-based**, and there are at most five stages.
  Empty or invalid reads cannot create an active stage. Refresh must preserve
  the last valid stages on a failed read for the same USB identity. During
  the real wireless handoff check, the daemon returned active index 0 with an
  empty list while unavailable; later calls recovered. A new transport starts
  without prior-transport editor drafts. See [verification.md](verification.md).
- Advertised polling rates take precedence over the historical
  125/500/1000 Hz fallback.
- A battery percentage is valid only when finite and in 0–100. Battery level
  and charging status are independent calls. A failed charging read must not
  invent a charging state or erase a successfully read display level. A
  complete telemetry sample requires valid charging and polling context.
- Cached battery levels are display-only, expire after 120 seconds,
  and cannot train predictions. Unknown charging is not equivalent to
  discharging. Successful zero DPI marks an unavailable receiver; a transport
  error alone does not prove that a previously available device disconnected.

## Lighting

Each zone uses its own advertised interface and method prefix:

| Zone | Effect interface | Prefix | Brightness interface |
| --- | --- | --- | --- |
| Main | `razer.device.lighting.chroma` | Empty | `razer.device.lighting.brightness` |
| Logo | `razer.device.lighting.logo` | `Logo` | Same as effect interface |
| Scroll | `razer.device.lighting.scroll` | `Scroll` | Same as effect interface |
| Backlight | `razer.device.lighting.backlight` | `Backlight` | Same as effect interface |
| Left | `razer.device.lighting.left` | `Left` | Same as effect interface |
| Right | `razer.device.lighting.right` | `Right` | Same as effect interface |

`get[Zone]Brightness() → d` and `set[Zone]Brightness(d)` use percent. Main
brightness omits the zone prefix. Sources: [brightness][brightness],
[logo and scroll][zones], [backlight and sides][side-zones].

Effect setters are named `set[Zone]<suffix>` and have no output arguments:

| Effect suffix | Input signature |
| --- | --- |
| `None`, `Spectrum`, `BreathRandom` | Empty |
| `Static`, `BreathSingle` | `yyy` |
| `BreathDual` | `yyyyyy` |
| `Wave` | `i` |
| `Reactive`, `StarlightSingle` | `yyyy` |
| `StarlightDual` | `yyyyyyy` |
| `StarlightRandom` | `y` |
| `Ripple` | `yyyd` |
| `RippleRandomColour` | `d` |

RGB bytes precede a speed byte where applicable. Wave direction is an integer,
not a byte. Ripple methods are main-only and live on
`razer.device.lighting.custom`, with no zone prefix. Starlight appears on the
audited main chroma interface; do not assume secondary zones support it.
Detect effects from full method signatures. [Main effects][lighting],
[zone effects][zones].

Remembered state is optional:

| Getter | Output signature |
| --- | --- |
| `get[Zone]Effect` | `s` |
| `get[Zone]EffectColors` | `ay` |
| `get[Zone]EffectSpeed` | `i` |
| `get[Zone]WaveDir` | `i` |

The main interface omits the zone prefix. Colors may contain up to three RGB
triplets; the controls edit at most two. Read only parameters needed by
the recognized effect and publish them together. A missing or truncated color
reply must not combine a new effect with old parameters. Unknown effects or
unsupported values, such as speed 4 when the app supports 1–3, remain
unselected. Direction values supported by the controls are 1 and 2.
Failed refreshes may retain a previous complete choice for the same USB
identity. Reading this state must never apply an effect. [Source][device].

## Limits and verification

- Polling and lighting getters report the daemon's remembered state, not
  direct physical hardware measurements.
- `isCharging()` converts the driver status to a boolean. The client cannot
  distinguish an erroneous nonzero driver status from charging, and its result
  is not atomic with the battery-level read.
- `getDPI()` may return cached DPI when a sysfs file disappears. Valid DPI does
  not prove that a wireless transport is connected.
- There is no upstream transport-selection API. Conflicting objects for the
  same serial must be resolved in the daemon or driver. [Device collection][collection],
  [hotplug handling][daemon].
- Macros, matrix painting, HyperPolling pairing/LED configuration and other
  device-specific controls are outside the current rewrite scope.
- Discovery must not disable global effect synchronization. The historical
  [Polychromatic backend comparison][poly-backend] is background for this
  decision, not a separate protocol dependency.

Tests exercise pure validation and reply interpretation. Capability fixtures
are introspection XML captured from real devices. There is no fake daemon or
mock D-Bus bus. Read-only probe checks validate the actual calls on available
hardware. Completed checks and remaining hardware-dependent scenarios are
recorded in [verification.md](verification.md).

[daemon]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/daemon.py
[device]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/hardware/device_base.py
[mouse-methods]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/dbus_services/dbus_methods/mamba.py
[mouse-client]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/pylib/openrazer/client/devices/mice.py
[mouse-models]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/hardware/mouse.py#L622-L661
[mouse-driver]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/driver/razermouse_driver.c#L3094-L3170
[common-driver]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/driver/razerchromacommon.c#L1334-L1380
[lighting]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/dbus_services/dbus_methods/chroma_keyboard.py
[zones]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/dbus_services/dbus_methods/nagahexv2.py
[side-zones]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/dbus_services/dbus_methods/lanceheadte.py
[brightness]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/dbus_services/dbus_methods/deathadder_chroma.py
[collection]: https://github.com/openrazer/openrazer/blob/477f4d6d3ab7e9deb6310d5bd3742560c5cbe80d/daemon/openrazer_daemon/device.py
[poly-backend]: https://github.com/polychromatic/polychromatic/blob/cd024b28f583dea330e015d203bf0af58eaf78f4/polychromatic/backends/openrazer.py
