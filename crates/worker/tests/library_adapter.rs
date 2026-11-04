//! The `OpenRazer` adapter and worker against a fake `org.razer` on a private bus,
//! for what the library's types alone can't show: the wire shapes reach the app's
//! models, writes stay guarded, and its signal streams drive reconnection.
//!
//! ```sh
//! dbus-run-session -- cargo test -p razerr-worker --test library_adapter -- --ignored
//! ```
//!
//! This must never meet a real daemon, so it stops unless it owns `org.razer` on
//! a bus that isn't the desktop session's. Only one owner can exist per bus, so
//! the phases share one test.
#![expect(
    clippy::unused_self,
    reason = "D-Bus methods take self whether or not the fake uses it"
)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use razerr_model::{
    Command, Device, Direction, Dpi, DpiStages, Effect, EffectKind, Percent, Snapshot, ZoneId,
};
use razerr_openrazer::Client;
use razerr_worker::Worker;
use zbus::{Connection, connection::Builder, interface};

const SERIAL: &str = "LIBRARY_TEST";

/// Well inside the worker's 20-second idle poll, so only a signal can be this quick.
const SIGNAL_LATENCY: Duration = Duration::from_secs(1);

type Stages = (u8, Vec<(u16, u16)>);

#[derive(Default)]
struct State {
    serials: Vec<String>,
    writes: usize,
    misrouted_writes: usize,
    transport_changed: bool,
    stages: Stages,
    brightness: f64,
    wave_direction: i32,
}

type Shared = Arc<Mutex<State>>;

struct Root(Shared);

#[interface(name = "razer.devices")]
impl Root {
    #[zbus(name = "getDevices")]
    fn devices(&self) -> Vec<String> {
        self.0.lock().expect("state").serials.clone()
    }
}

struct Misc(Shared);

#[interface(name = "razer.device.misc")]
impl Misc {
    #[zbus(name = "getVidPid")]
    fn usb(&self) -> Vec<i32> {
        let changed = self.0.lock().expect("state").transport_changed;
        vec![0x1532, if changed { 0x00c3 } else { 0x00c2 }]
    }

    #[zbus(name = "getDeviceName")]
    fn name(&self) -> String {
        "Adapter fixture".into()
    }
}

struct Sensitivity(Shared);

#[interface(name = "razer.device.dpi")]
impl Sensitivity {
    #[zbus(name = "getDPI")]
    fn dpi(&self) -> Vec<i32> {
        vec![800, 800]
    }

    #[zbus(name = "setDPI")]
    fn set_dpi(&self, x: u16, y: u16) {
        assert!(x > 0 && y > 0);
        self.0.lock().expect("state").writes += 1;
    }

    #[zbus(name = "maxDPI")]
    fn maximum(&self) -> i32 {
        30000
    }

    // One struct output, as upstream advertises `(ya(qq))`; a bare tuple would be two.
    #[zbus(name = "getDPIStages")]
    fn stages(&self) -> (Stages,) {
        (self.0.lock().expect("state").stages.clone(),)
    }

    #[zbus(name = "setDPIStages")]
    fn set_stages(&self, active: u8, values: Vec<(u16, u16)>) {
        let mut state = self.0.lock().expect("state");
        state.stages = (active, values);
        state.writes += 1;
    }
}

struct Logo(Shared);

#[interface(name = "razer.device.lighting.logo")]
impl Logo {
    #[zbus(name = "getLogoBrightness")]
    fn brightness(&self) -> f64 {
        self.0.lock().expect("state").brightness
    }

    #[zbus(name = "setLogoBrightness")]
    fn set_brightness(&self, value: f64) {
        let mut state = self.0.lock().expect("state");
        state.brightness = value;
        state.writes += 1;
    }

    #[zbus(name = "getLogoEffect")]
    fn effect(&self) -> String {
        "wave".into()
    }

    #[zbus(name = "getLogoWaveDir")]
    fn direction(&self) -> i32 {
        self.0.lock().expect("state").wave_direction
    }

    #[zbus(name = "setLogoWave")]
    fn wave(&self, direction: i32) {
        let mut state = self.0.lock().expect("state");
        state.wave_direction = direction;
        state.writes += 1;
    }
}

/// A main-zone-only effect, which a write for another zone must never reach.
struct Custom(Shared);

#[interface(name = "razer.device.lighting.custom")]
impl Custom {
    #[zbus(name = "setRippleRandomColour")]
    fn ripple(&self, refresh_rate: f64) {
        assert!(refresh_rate > 0.0);
        self.0.lock().expect("state").misrouted_writes += 1;
    }
}

fn assert_private_bus() {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").expect("run under dbus-run-session");
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    assert!(
        !address.contains(&format!("{runtime}/bus")),
        "refusing to run on the desktop session bus"
    );
}

async fn serve(state: &Shared) -> Connection {
    let path = format!("/org/razer/device/{SERIAL}");
    Builder::session()
        .serve_at("/org/razer", Root(state.clone()))
        .serve_at(path.as_str(), Misc(state.clone()))
        .serve_at(path.as_str(), Sensitivity(state.clone()))
        .serve_at(path.as_str(), Logo(state.clone()))
        .serve_at(path.as_str(), Custom(state.clone()))
        .name("org.razer")
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .build()
        .await
        .expect("org.razer is taken, so this may be a real daemon: stopping")
}

fn writes(state: &Shared) -> usize {
    state.lock().expect("state").writes
}

fn set_wave(device: &Device, direction: Direction) -> Command {
    Command::SetEffect {
        serial: device.serial.clone(),
        zone: ZoneId::Logo,
        effect: Effect::Wave { direction },
    }
}

/// The first snapshot accepted by `wanted`, which must arrive within `limit`.
async fn snapshot(
    worker: &mut Worker,
    limit: Duration,
    wanted: impl Fn(&Snapshot) -> bool,
) -> Arc<Snapshot> {
    let search = async {
        loop {
            let snapshot = worker.snapshots.borrow_and_update().clone();
            if wanted(&snapshot) {
                return snapshot;
            }
            worker.snapshots.changed().await.expect("worker");
        }
    };
    tokio::time::timeout(limit, search)
        .await
        .expect("the expected snapshot never arrived")
}

/// Bad objects don't hide good ones, and nested replies reach the models.
async fn discovery_is_partial_and_reads_nested_replies(client: &Client) -> Device {
    let report = client.probe().await.expect("probe");

    assert_eq!(report.devices.len(), 1);
    assert_eq!(report.observed_serials.len(), 2);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.serial == "bad/serial" && !warning.transient)
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.serial == "MISSING" && warning.transient)
    );

    let device = report.devices[0].clone();
    assert!(
        device.capabilities.dpi_stages,
        "(ya(qq)) must be advertised"
    );
    let stages = device.stages.as_ref().and_then(|stages| stages.fresh());
    assert_eq!(stages.expect("stages").active(), 1);

    let raw = Connection::session().await.expect("session bus");
    let reply = raw
        .call_method(
            Some("org.razer"),
            format!("/org/razer/device/{SERIAL}"),
            Some("razer.device.dpi"),
            "getDPIStages",
            &(),
        )
        .await
        .expect("raw stages");
    assert_eq!(
        reply.body().signature().to_string(),
        "(ya(qq))",
        "the fake must reply like upstream"
    );
    device
}

/// Writes use the right methods and values, and read back through both paths.
async fn writes_reach_their_zones_and_read_back(client: &Client, state: &Shared, device: &Device) {
    let stages = Command::SetDpiStages {
        serial: device.serial.clone(),
        stages: DpiStages::new(
            2,
            vec![
                Dpi::new(400, 400).expect("dpi"),
                Dpi::new(1200, 1200).expect("dpi"),
            ],
        )
        .expect("stages"),
    };
    let brightness = Command::SetBrightness {
        serial: device.serial.clone(),
        zone: ZoneId::Logo,
        brightness: Percent::new(65.0).expect("percent"),
    };
    let wave = set_wave(device, Direction::Left);
    for command in [&stages, &brightness, &wave] {
        client.execute(device, command).await.expect("write");
    }
    assert_eq!(writes(state), 3);

    let refreshed = client.probe().await.expect("readback");
    for command in [&stages, &brightness, &wave] {
        assert_eq!(command.reflected_by(&refreshed.devices[0]), Some(true));
    }
    let live = client.live(&refreshed.devices).await;
    assert_eq!(stages.reflected_by(&live.devices[0]), Some(true));
}

/// The app's existing wire values, which differ from upstream's `WAVE_RIGHT = 1`.
/// Changing them is a deliberate decision, not a side effect.
async fn wave_directions_keep_their_wire_values(client: &Client, state: &Shared, device: &Device) {
    for (direction, wire) in [(Direction::Left, 1), (Direction::Right, 2)] {
        let command = set_wave(device, direction);
        client.execute(device, &command).await.expect("wave");
        assert_eq!(state.lock().expect("state").wave_direction, wire);
    }
}

/// Everything refused here must be refused before any setter runs.
async fn guards_stop_writes_before_the_bus(client: &Client, state: &Shared, device: &Device) {
    let before = writes(state);

    let too_high = Command::SetDpi {
        serial: device.serial.clone(),
        dpi: Dpi::new(30001, 30001).expect("dpi"),
    };
    client.execute(device, &too_high).await.expect_err("range");

    // Capabilities never offer this, so this models a corrupted or forged device.
    let mut forged = device.clone();
    let logo = forged
        .capabilities
        .zones
        .iter_mut()
        .find(|zone| zone.id == ZoneId::Logo);
    logo.expect("logo").effects.push(EffectKind::RippleRandom);
    let misrouted = Command::SetEffect {
        serial: device.serial.clone(),
        zone: ZoneId::Logo,
        effect: Effect::RippleRandom,
    };
    let error = client.execute(&forged, &misrouted).await.expect_err("zone");
    assert!(
        error.to_string().contains("unsupported effect endpoint"),
        "{error}"
    );
    assert_eq!(state.lock().expect("state").misrouted_writes, 0);

    state.lock().expect("state").transport_changed = true;
    let wave = set_wave(device, Direction::Left);
    let error = client.execute(device, &wave).await.expect_err("transport");
    assert!(
        error.to_string().contains("USB identity changed"),
        "{error}"
    );
    state.lock().expect("state").transport_changed = false;

    assert_eq!(writes(state), before);
}

/// The worker reacts to the library's signal streams, not just its idle poll.
async fn worker_follows_owner_and_hotplug_signals(daemon: &Connection) {
    let mut worker = razerr_worker::spawn();
    let healthy = |snapshot: &Snapshot| {
        snapshot.daemon_available && snapshot.reliable && !snapshot.devices.is_empty()
    };
    let first = snapshot(&mut worker, Duration::from_secs(5), healthy).await;
    // The listener subscribes on its own connection, in parallel with the first read.
    tokio::time::sleep(Duration::from_millis(300)).await;

    daemon.release_name("org.razer").await.expect("release");
    let lost = snapshot(&mut worker, SIGNAL_LATENCY, |snapshot| {
        !snapshot.daemon_available
    })
    .await;
    assert!(lost.daemon_generation > first.daemon_generation);
    assert!(
        !lost.devices.is_empty(),
        "stale devices stay until proven gone"
    );

    daemon.request_name("org.razer").await.expect("reacquire");
    let back = snapshot(&mut worker, SIGNAL_LATENCY, healthy).await;

    daemon
        .emit_signal(
            None::<&str>,
            "/org/razer",
            "razer.devices",
            "device_added",
            &(),
        )
        .await
        .expect("hotplug signal");
    let revision = back.revision;
    snapshot(&mut worker, SIGNAL_LATENCY, |snapshot| {
        snapshot.revision > revision
    })
    .await;

    worker.task.abort();
}

#[tokio::test]
#[ignore = "needs a private bus; see the top of this file"]
async fn adapter_and_worker_against_a_private_daemon() {
    assert_private_bus();
    let state = Arc::new(Mutex::new(State {
        serials: vec!["bad/serial".into(), "MISSING".into(), SERIAL.into()],
        stages: (1, vec![(800, 800), (1600, 1600)]),
        brightness: 25.0,
        wave_direction: 2,
        ..State::default()
    }));
    let daemon = serve(&state).await;
    let client = Client::connect().await.expect("client");

    let device = discovery_is_partial_and_reads_nested_replies(&client).await;
    assert_eq!(writes(&state), 0, "discovery only reads");
    writes_reach_their_zones_and_read_back(&client, &state, &device).await;
    wave_directions_keep_their_wire_values(&client, &state, &device).await;
    guards_stop_writes_before_the_bus(&client, &state, &device).await;

    // Worker enumeration is all-or-nothing, so give it only the real device.
    state.lock().expect("state").serials = vec![SERIAL.into()];
    worker_follows_owner_and_hotplug_signals(&daemon).await;
}
