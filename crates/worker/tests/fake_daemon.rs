//! The real worker against a fake `org.razer` on a private bus, for what only a
//! whole write can show: how many times it is sent, and when it counts as done.
//!
//! ```sh
//! dbus-run-session -- cargo test -p razerr-worker --test fake_daemon -- --ignored
//! ```
//!
//! This must never meet a real daemon, so it stops unless it owns `org.razer` on
//! a bus that isn't the desktop session's.
#![expect(
    clippy::unused_self,
    reason = "D-Bus methods take self whether or not the fake uses it"
)]

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use razerr_model::{Command, Dpi, PollRate, Reading, Serial, Snapshot};
use razerr_worker::{Request, Worker};
use zbus::{connection::Builder, fdo, interface};

const SERIAL: &str = "FAKE0000000001";

/// When a written polling rate shows up in the device's readback.
#[derive(Clone, Copy)]
enum Apply {
    After(Duration),
    Never,
}

struct Device {
    rate: i32,
    written: Option<(i32, Instant)>,
    apply: Apply,
    rate_writes: u32,
    dpi_writes: u32,
    fail_dpi: bool,
}

impl Device {
    fn rate(&mut self) -> i32 {
        if let (Some((rate, at)), Apply::After(delay)) = (self.written, self.apply)
            && at.elapsed() >= delay
        {
            self.rate = rate;
            self.written = None;
        }
        self.rate
    }
}

type Shared = Arc<Mutex<Device>>;

struct Root;
#[interface(name = "razer.devices")]
impl Root {
    #[zbus(name = "getDevices")]
    fn devices(&self) -> Vec<String> {
        vec![SERIAL.into()]
    }
}

struct Misc(Shared);
#[interface(name = "razer.device.misc")]
impl Misc {
    #[zbus(name = "getVidPid")]
    fn usb(&self) -> Vec<i32> {
        vec![0x1532, 0x00c2]
    }
    #[zbus(name = "getDeviceName")]
    fn name(&self) -> String {
        "Fake Mouse".into()
    }
    #[zbus(name = "getDeviceType")]
    fn kind(&self) -> String {
        "mouse".into()
    }
    #[zbus(name = "getFirmware")]
    fn firmware(&self) -> String {
        "v1.0".into()
    }
    #[zbus(name = "getDriverVersion")]
    fn driver(&self) -> String {
        "3.0".into()
    }
    #[zbus(name = "getPollRate")]
    fn rate(&self) -> i32 {
        self.0.lock().expect("device").rate()
    }
    #[zbus(name = "setPollRate")]
    fn set_rate(&self, rate: u16) {
        let mut device = self.0.lock().expect("device");
        device.rate_writes += 1;
        device.written = Some((i32::from(rate), Instant::now()));
    }
}

struct DpiInterface(Shared);
#[interface(name = "razer.device.dpi")]
impl DpiInterface {
    #[zbus(name = "getDPI")]
    fn dpi(&self) -> Vec<i32> {
        vec![800, 800]
    }
    #[zbus(name = "setDPI")]
    fn set_dpi(&self, x: u16, y: u16) -> fdo::Result<()> {
        assert!(x > 0 && y > 0);
        let mut device = self.0.lock().expect("device");
        device.dpi_writes += 1;
        if device.fail_dpi {
            return Err(fdo::Error::Failed("Input/output error".into()));
        }
        Ok(())
    }
    #[zbus(name = "maxDPI")]
    fn maximum(&self) -> i32 {
        30000
    }
}

async fn serve(device: Shared) -> zbus::Connection {
    let path = format!("/org/razer/device/{SERIAL}");
    Builder::session()
        .serve_at("/org/razer", Root)
        .serve_at(path.as_str(), Misc(device.clone()))
        .serve_at(path.as_str(), DpiInterface(device))
        .name("org.razer")
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .build()
        .await
        .expect("org.razer is taken, so this may be a real daemon: stopping")
}

fn assert_private_bus() {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").expect("run under dbus-run-session");
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    assert!(
        !address.contains(&format!("{runtime}/bus")),
        "refusing to run on the desktop session bus"
    );
}

async fn run(worker: &Worker, command: Command) -> Result<(), String> {
    let (request, outcome) = Request::new(command);
    worker.commands.send(request).await.expect("worker");
    outcome.await.expect("the worker always answers")
}

async fn ready(worker: &mut Worker) -> Arc<Snapshot> {
    loop {
        let snapshot = worker.snapshots.borrow_and_update().clone();
        if snapshot.reliable && !snapshot.devices.is_empty() {
            return snapshot;
        }
        worker.snapshots.changed().await.expect("worker");
    }
}

fn rate(snapshot: &Snapshot) -> &Reading<PollRate> {
    &snapshot.devices[0]
        .polling
        .as_ref()
        .expect("polling")
        .current
}

#[tokio::test]
#[ignore = "needs a private bus; see the top of this file"]
async fn writes_run_once_and_finish_on_the_device_readback() {
    assert_private_bus();
    let device = Arc::new(Mutex::new(Device {
        rate: 1000,
        written: None,
        apply: Apply::After(Duration::from_millis(500)),
        rate_writes: 0,
        dpi_writes: 0,
        fail_dpi: false,
    }));
    let _daemon = serve(device.clone()).await;
    let mut worker = razerr_worker::spawn();
    ready(&mut worker).await;
    let serial = Serial::new(SERIAL).expect("serial");
    let set_rate = |value| Command::SetPollRate {
        serial: serial.clone(),
        rate: PollRate::new(value).expect("rate"),
    };

    // Acknowledged at once but applied later: not a failure, and sent once.
    assert_eq!(run(&worker, set_rate(500)).await, Ok(()));
    assert_eq!(device.lock().expect("device").rate_writes, 1);
    let snapshot = worker.snapshots.borrow().clone();
    assert_eq!(rate(&snapshot).fresh().map(|rate| rate.get()), Some(500));

    // Acknowledged but never applied: reported, and still not sent again.
    device.lock().expect("device").apply = Apply::Never;
    let error = run(&worker, set_rate(125))
        .await
        .expect_err("never applied");
    assert!(error.contains("still reports"), "{error}");
    assert_eq!(device.lock().expect("device").rate_writes, 2);
    let snapshot = worker.snapshots.borrow().clone();
    assert_eq!(rate(&snapshot).fresh().map(|rate| rate.get()), Some(500));
    assert_eq!(snapshot.error.as_deref(), Some(error.as_str()));

    // A setter that fails reaches the caller and isn't retried.
    device.lock().expect("device").fail_dpi = true;
    let set_dpi = Command::SetDpi {
        serial,
        dpi: Dpi::new(900, 900).expect("DPI"),
    };
    let error = run(&worker, set_dpi).await.expect_err("setter failed");
    assert!(error.contains("Input/output error"), "{error}");
    assert_eq!(device.lock().expect("device").dpi_writes, 1);

    worker.task.abort();
}
