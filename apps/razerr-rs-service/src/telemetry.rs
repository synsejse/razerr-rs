//! SQLite belongs to one thread; the async service never blocks on a query.

use std::{
    collections::HashSet,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use razerr_metrics::{Sample, Store, unix_now};
use razerr_model::{Availability, Snapshot, SnapshotKind};
use razerr_storage::{Paths, Settings};
use tokio::sync::oneshot;

enum Request {
    Configure(Settings),
    Observe {
        snapshot: Arc<Snapshot>,
        timestamp: i64,
        monotonic: Duration,
    },
    Clear(oneshot::Sender<Result<(), String>>),
    Stop,
}

pub struct Telemetry {
    sender: mpsc::Sender<Request>,
    task: thread::JoinHandle<()>,
}

impl Telemetry {
    pub fn start(paths: Paths, settings: Settings) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let task = thread::Builder::new()
            .name("razerr-history".into())
            .spawn(move || Writer::new(paths, settings).run(receiver))?;
        Ok(Self { sender, task })
    }

    fn send(&self, request: Request) {
        if self.sender.send(request).is_err() {
            tracing::error!("history thread stopped");
        }
    }

    pub fn configure(&self, settings: &Settings) {
        self.send(Request::Configure(settings.clone()));
    }

    pub fn observe(&self, snapshot: Arc<Snapshot>, monotonic: Duration) {
        if snapshot.kind == SnapshotKind::Ack {
            return;
        }
        self.send(Request::Observe {
            snapshot,
            timestamp: unix_now(),
            monotonic,
        });
    }

    pub async fn clear(&self) -> Result<(), String> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Request::Clear(reply))
            .map_err(|_| "history thread stopped".to_owned())?;
        response
            .await
            .map_err(|_| "history thread stopped".to_owned())?
    }

    pub async fn shutdown(self) {
        let _ = self.sender.send(Request::Stop);
        if let Err(error) = tokio::task::spawn_blocking(move || self.task.join()).await {
            tracing::error!(%error, "joining history thread failed");
        }
    }
}

struct Writer {
    paths: Paths,
    settings: Settings,
    store: Option<Store>,
    serials: HashSet<razerr_model::Serial>,
    retry_after: Duration,
    last_prune: Option<i64>,
    last_error: Option<String>,
    continuity: u64,
}

impl Writer {
    fn new(paths: Paths, settings: Settings) -> Self {
        Self {
            paths,
            settings,
            store: None,
            serials: HashSet::new(),
            retry_after: Duration::ZERO,
            last_prune: None,
            last_error: None,
            continuity: 0,
        }
    }

    fn open(&mut self) -> Result<&mut Store, String> {
        if self.store.is_none() {
            self.paths
                .create_state_dir()
                .map_err(|error| error.to_string())?;
            let mut store = Store::open_writer(&self.paths.history_file())
                .map_err(|error| error.to_string())?;
            store
                .configure(
                    self.settings.history.sample_seconds,
                    self.settings.history.days,
                )
                .map_err(|error| error.to_string())?;
            self.store = Some(store);
        }
        self.store
            .as_mut()
            .ok_or_else(|| "history failed to open".to_owned())
    }

    /// The next sample of every device starts a new session.
    fn break_sessions(&mut self) {
        if let Some(store) = &mut self.store {
            store.gap(None);
        }
        self.serials.clear();
    }

    fn run(mut self, receiver: mpsc::Receiver<Request>) {
        for request in receiver {
            let result = match request {
                Request::Stop => break,
                Request::Configure(settings) => self.configure(settings),
                Request::Clear(reply) => {
                    let result = self
                        .open()
                        .and_then(|store| store.clear(None).map_err(|error| error.to_string()));
                    self.serials.clear();
                    if !self.settings.history.enabled {
                        self.store = None;
                    }
                    let _ = reply.send(result.clone());
                    result
                }
                Request::Observe {
                    snapshot,
                    timestamp,
                    monotonic,
                } => {
                    let result = self.observe(&snapshot, timestamp, monotonic);
                    if result.is_err() {
                        // Even a busy database must not bridge an observation
                        // that could not be recorded or fully processed.
                        self.break_sessions();
                    }
                    result
                }
            };
            match result {
                Ok(()) => self.last_error = None,
                Err(error) => {
                    if self.last_error.as_ref() != Some(&error) {
                        tracing::error!(%error, "battery history");
                    }
                    self.last_error = Some(error);
                }
            }
        }
    }

    fn configure(&mut self, settings: Settings) -> Result<(), String> {
        if settings.history != self.settings.history {
            self.break_sessions();
            self.retry_after = Duration::ZERO;
            self.last_prune = None;
        }
        self.settings = settings;
        if !self.settings.history.enabled {
            self.store = None;
        }
        if let Some(store) = &mut self.store {
            store
                .configure(
                    self.settings.history.sample_seconds,
                    self.settings.history.days,
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn observe(
        &mut self,
        snapshot: &Snapshot,
        timestamp: i64,
        monotonic: Duration,
    ) -> Result<(), String> {
        if !self.settings.history.enabled || monotonic < self.retry_after {
            return Ok(());
        }
        if self.store.is_none() {
            self.retry_after = monotonic + Duration::from_secs(60);
            self.open()?;
            self.retry_after = Duration::ZERO;
        }
        if self.continuity != snapshot.continuity {
            self.break_sessions();
            self.continuity = snapshot.continuity;
        }
        let Some(store) = &mut self.store else {
            return Ok(());
        };
        if self
            .last_prune
            .is_none_or(|last| timestamp < last || timestamp - last >= 86400)
        {
            store.prune(timestamp).map_err(|error| error.to_string())?;
            self.last_prune = Some(timestamp);
        }
        if !snapshot.daemon_available || !snapshot.reliable {
            self.break_sessions();
            return Ok(());
        }
        let current: HashSet<_> = snapshot
            .devices
            .iter()
            .map(|device| device.serial.clone())
            .collect();
        for serial in self.serials.difference(&current) {
            store.gap(Some(serial));
        }
        self.serials = current;
        for device in &snapshot.devices {
            match sample(device, timestamp, monotonic) {
                Some(sample) => {
                    store.record(&sample).map_err(|error| error.to_string())?;
                }
                None => store.gap(Some(&device.serial)),
            }
        }
        Ok(())
    }
}

fn sample(device: &razerr_model::Device, timestamp: i64, monotonic: Duration) -> Option<Sample> {
    if device.availability == Availability::Unavailable {
        return None;
    }
    let battery = device.battery.as_ref()?;
    let poll_rate = match &device.polling {
        Some(polling) => polling.current.fresh()?.get(),
        None => 0,
    };
    Some(Sample {
        serial: device.serial.clone(),
        timestamp,
        monotonic,
        battery: *battery.level.fresh()?,
        charging: battery.charging?,
        poll_rate,
        usb: device.usb?,
    })
}
