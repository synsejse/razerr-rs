use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use razerr_model::{Command, Snapshot, SnapshotKind};
use razerr_openrazer::{Client, ProbeReport};
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
    time::Instant,
};
use zbus::{MatchRule, MessageStream, message::Type};

use crate::{
    cache,
    queue::{Pending, Request, RetrySchedule},
};

/// Pauses between readbacks of a write. Firmware can acknowledge a setter before
/// applying it, so an old value straight after a write proves nothing yet.
const SETTLE_DELAYS: [Duration; 5] = [
    Duration::from_millis(150),
    Duration::from_millis(250),
    Duration::from_millis(400),
    Duration::from_millis(600),
    Duration::from_millis(800),
];

#[derive(Clone, Debug)]
pub enum WorkerEvent {
    Error(String),
}

pub struct Worker {
    pub commands: mpsc::Sender<Request>,
    pub snapshots: watch::Receiver<Arc<Snapshot>>,
    pub events: broadcast::Receiver<WorkerEvent>,
    pub task: JoinHandle<()>,
}

pub fn spawn() -> Worker {
    let (commands, receiver) = mpsc::channel(256);
    let (snapshots, snapshot_receiver) = watch::channel(Arc::new(Snapshot::default()));
    let (events, event_receiver) = broadcast::channel(64);
    let task = tokio::spawn(run(receiver, snapshots, events));
    Worker {
        commands,
        snapshots: snapshot_receiver,
        events: event_receiver,
        task,
    }
}

struct Actor {
    client: Option<Client>,
    state: Snapshot,
    snapshots: watch::Sender<Arc<Snapshot>>,
    events: broadcast::Sender<WorkerEvent>,
    last_read: Instant,
    owner_generation: Arc<AtomicU64>,
    seen_owner: u64,
}

impl Actor {
    fn publish(&mut self, kind: SnapshotKind) {
        self.state.kind = kind;
        self.state.revision = self.state.revision.saturating_add(1);
        self.snapshots.send_replace(Arc::new(self.state.clone()));
    }

    fn error(&mut self, error: String) {
        if self.state.error.as_ref() != Some(&error) {
            let _ = self.events.send(WorkerEvent::Error(error.clone()));
        }
        self.state.error = Some(error);
    }

    fn fail(&mut self, error: String) {
        self.state.continuity = self.state.continuity.saturating_add(1);
        self.state.daemon_generation = self.state.daemon_generation.saturating_add(1);
        let elapsed = self.last_read.elapsed();
        for device in &mut self.state.devices {
            cache::stale(device, elapsed);
        }
        self.last_read = Instant::now();
        self.state.reliable = false;
        self.state.daemon_available = false;
        self.state.refreshing = false;
        self.error(error);
        self.client = None;
        self.publish(SnapshotKind::Full);
    }

    fn sync_owner(&mut self) -> bool {
        let generation = self.owner_generation.load(Ordering::Acquire);
        if self.seen_owner == generation {
            return false;
        }
        self.seen_owner = generation;
        self.fail("OpenRazer connection changed; reconnecting".into());
        true
    }

    async fn refresh(&mut self, full: bool) {
        self.sync_owner();
        if full {
            self.state.refreshing = true;
            self.publish(SnapshotKind::Ack);
        }
        if self.client.is_none() {
            match Client::connect().await {
                Ok(client) => self.client = Some(client),
                Err(error) => {
                    self.fail(error.to_string());
                    return;
                }
            }
        }
        let Some(client) = &self.client else {
            return;
        };
        let serials = match client.serials().await {
            Ok(serials) => serials,
            Err(error) => {
                self.fail(error.to_string());
                return;
            }
        };
        let mut full = full
            || !self.state.reliable
            || serials.len() != self.state.devices.len()
            || serials.iter().any(|serial| {
                !self
                    .state
                    .devices
                    .iter()
                    .any(|device| &device.serial == serial)
            });
        let mut report = if full {
            match client.probe().await {
                Ok(report) => report,
                Err(error) => {
                    self.fail(error.to_string());
                    return;
                }
            }
        } else {
            client.live(&self.state.devices).await
        };
        if !full
            && report.devices.iter().any(|device| {
                self.state
                    .devices
                    .iter()
                    .find(|old| old.serial == device.serial)
                    .is_some_and(|old| {
                        needs_discovery(old.usb, device.usb, old.availability, device.availability)
                    })
            })
        {
            report = match client.probe().await {
                Ok(report) => report,
                Err(error) => {
                    self.fail(error.to_string());
                    return;
                }
            };
            full = true;
        }
        // Firmware can briefly return invalid stage data or fail a power read
        // around writes. Retry reads once before publishing a visible error.
        if report.warnings.iter().any(retryable_read) {
            for warning in &report.warnings {
                tracing::debug!(?warning, "retrying device read");
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
            if self.owner_generation.load(Ordering::Acquire) == self.seen_owner {
                report = if full {
                    match client.probe().await {
                        Ok(report) => report,
                        Err(error) => {
                            self.fail(error.to_string());
                            return;
                        }
                    }
                } else {
                    client.live(&self.state.devices).await
                };
            }
        }
        // Discard a read spanning an ownership change, including a fast restart.
        if !self.sync_owner() {
            self.apply(report, full);
        }
    }

    fn apply(&mut self, mut report: ProbeReport, full: bool) {
        let elapsed = self.last_read.elapsed();
        let mut reliable = report.observed_serials.len() == report.devices.len()
            && !report
                .warnings
                .iter()
                .any(|warning| matches!(warning.operation.as_str(), "getDevices" | "Introspect"));
        for new in &mut report.devices {
            if let Some(old) = self
                .state
                .devices
                .iter()
                .find(|old| old.serial == new.serial)
            {
                merge_readings(old, new, &report.warnings, elapsed, full);
            }
            if new.usb.is_none() || new.name.is_none() {
                reliable = false;
            }
        }
        for serial in &report.observed_serials {
            if !report.devices.iter().any(|device| &device.serial == serial)
                && let Some(old) = self
                    .state
                    .devices
                    .iter()
                    .find(|device| &device.serial == serial)
            {
                let mut old = old.clone();
                cache::stale(&mut old, elapsed);
                report.devices.push(old);
            }
        }
        if !reliable
            || report.devices.iter().any(|device| {
                device.battery.as_ref().is_some_and(|battery| {
                    battery.level.fresh().is_none()
                        || battery.charging.is_none()
                        || device
                            .polling
                            .as_ref()
                            .is_some_and(|polling| polling.current.fresh().is_none())
                })
            })
            || self.state.devices.iter().any(|old| {
                !report.devices.iter().any(|new| {
                    new.serial == old.serial
                        && new.usb == old.usb
                        && new.availability == old.availability
                })
            })
        {
            self.state.continuity = self.state.continuity.saturating_add(1);
        }
        self.state.devices = report.devices;
        self.state.daemon_available = true;
        self.state.reliable = reliable;
        self.state.refreshing = false;
        if report.warnings.is_empty() {
            self.state.error = None;
        } else {
            self.error(
                report
                    .warnings
                    .iter()
                    .map(|warning| {
                        tracing::warn!(?warning, "device read failed after refresh");
                        read_error(warning)
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        self.last_read = Instant::now();
        self.publish(if full {
            SnapshotKind::Full
        } else {
            SnapshotKind::Live
        });
    }

    /// A refresh that reached the daemon succeeded, even if some readings failed;
    /// those stay visible in the snapshot.
    async fn full_refresh(&mut self) -> Result<(), String> {
        self.refresh(true).await;
        if self.state.daemon_available {
            Ok(())
        } else {
            Err(self
                .state
                .error
                .clone()
                .unwrap_or_else(|| "OpenRazer is unavailable".into()))
        }
    }

    /// Runs one write, exactly once, and reports how it ended. A failed or
    /// unconfirmed write is never repeated, because it may have reached the device.
    async fn execute(&mut self, command: &Command) -> Result<(), String> {
        if self.sync_owner() {
            self.refresh(true).await;
            let error = "Device connection changed; repeat the command after refresh".to_owned();
            self.error(error.clone());
            self.publish(SnapshotKind::Ack);
            return Err(error);
        }
        let result = async {
            if !self.state.reliable || !self.state.daemon_available {
                return Err("device connection is unavailable".to_owned());
            }
            let device = self
                .state
                .devices
                .iter()
                .find(|device| Some(&device.serial) == command.serial())
                .ok_or("device was removed")?;
            let client = self.client.as_ref().ok_or("OpenRazer is unavailable")?;
            client
                .execute(device, command)
                .await
                .map_err(|error| error.to_string())
        }
        .await;
        let result = match result {
            // Re-read after a successful write: stages can alter both DPI axes,
            // and the daemon can normalize settings rather than apply them literally.
            Ok(()) => self.confirm(command).await,
            Err(error) => {
                // A setter may fail because the transport changed between enqueue
                // and execution. Rediscover before processing subsequent commands.
                self.refresh(true).await;
                Err(error)
            }
        };
        if let Err(error) = &result {
            self.error(error.clone());
            self.publish(SnapshotKind::Ack);
        }
        result
    }

    /// Reads back until the device shows `command`. No fresh reading to compare
    /// with is not a failure: nothing contradicts the write.
    async fn confirm(&mut self, command: &Command) -> Result<(), String> {
        let mut delays = SETTLE_DELAYS.into_iter();
        let readback = loop {
            self.refresh(true).await;
            let readback = self.readback(command);
            match next_read(readback, &mut delays) {
                Some(delay) => tokio::time::sleep(delay).await,
                None => break readback,
            }
        };
        match readback {
            Readback::Applied | Readback::Unread => Ok(()),
            Readback::Different => Err(
                "The device accepted this change but still reports another value. \
                 It was not sent again; refresh to check."
                    .into(),
            ),
            Readback::Lost => Err(
                "The connection dropped while checking this change. It may have \
                 applied; refresh to check."
                    .into(),
            ),
        }
    }

    fn readback(&self, command: &Command) -> Readback {
        if !self.state.daemon_available {
            return Readback::Lost;
        }
        let shown = self
            .state
            .devices
            .iter()
            .find(|device| Some(&device.serial) == command.serial())
            .and_then(|device| command.reflected_by(device));
        match shown {
            Some(true) => Readback::Applied,
            Some(false) => Readback::Different,
            None => Readback::Unread,
        }
    }
}

/// What a device showed after a write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Readback {
    Applied,
    Different,
    /// No fresh reading of that setting.
    Unread,
    /// The connection went away, so nothing more can be learned.
    Lost,
}

/// How long to wait before reading again, or `None` once `readback` settles the
/// question or the delays run out.
fn next_read(readback: Readback, delays: &mut impl Iterator<Item = Duration>) -> Option<Duration> {
    match readback {
        Readback::Applied | Readback::Lost => None,
        Readback::Different | Readback::Unread => delays.next(),
    }
}

fn retryable_read(warning: &razerr_openrazer::ReadWarning) -> bool {
    matches!(warning.operation.as_str(), "getDPIStages" | "isCharging")
}

fn read_error(warning: &razerr_openrazer::ReadWarning) -> String {
    let message = match warning.operation.as_str() {
        "getDPIStages" => "Could not read DPI stages. Retrying automatically.",
        "isCharging" => "Charging status is unavailable. Retrying automatically.",
        _ => {
            return format!(
                "{}: {}: {}",
                warning.serial,
                warning.operation,
                warning
                    .message
                    .lines()
                    .rfind(|line| !line.trim().is_empty())
                    .unwrap_or("Read failed")
                    .trim()
            );
        }
    };
    format!("{}: {message}", warning.serial)
}

fn merge_readings(
    old: &razerr_model::Device,
    new: &mut razerr_model::Device,
    warnings: &[razerr_openrazer::ReadWarning],
    elapsed: Duration,
    full: bool,
) {
    if old.usb.is_some()
        && old.usb == new.usb
        && warnings.iter().any(|warning| {
            warning.serial == new.serial.as_str()
                && warning.operation == "getDPI"
                && warning.transient
        })
    {
        new.availability = old.availability;
    }
    cache::merge(old, new, elapsed, full);
    if full {
        for zone in &mut new.zones {
            // Only I/O failures retain remembered lighting. An unknown
            // effect or unsupported parameter must remain unavailable.
            let prefix = razerr_openrazer::protocol::zone_prefix(zone.id);
            let io_failed = warnings.iter().any(|warning| {
                warning.serial == new.serial.as_str()
                    && (warning.operation.starts_with(&format!("get{prefix}Effect"))
                        || warning.operation == format!("get{prefix}WaveDir"))
                    && warning.transient
            });
            if io_failed
                && old.usb.is_some()
                && old.usb == new.usb
                && let Some(old_zone) = old.zones.iter().find(|old| old.id == zone.id)
                && old_zone.effect.displayed().is_some_and(|effect| {
                    new.capabilities
                        .zones
                        .iter()
                        .any(|cap| cap.id == zone.id && cap.effects.contains(&effect.kind()))
                })
            {
                zone.effect = razerr_openrazer::interpret::merge_reading(
                    &old_zone.effect,
                    None,
                    elapsed,
                    None,
                );
            }
        }
    }
}

fn needs_discovery(
    previous_usb: Option<razerr_model::UsbId>,
    current_usb: Option<razerr_model::UsbId>,
    previous: razerr_model::Availability,
    current: razerr_model::Availability,
) -> bool {
    previous_usb != current_usb
        || (previous == razerr_model::Availability::Unavailable
            && current == razerr_model::Availability::Responding)
}

async fn run(
    mut commands: mpsc::Receiver<Request>,
    snapshots: watch::Sender<Arc<Snapshot>>,
    events: broadcast::Sender<WorkerEvent>,
) {
    let owner_generation = Arc::new(AtomicU64::new(0));
    let mut actor = Actor {
        client: None,
        state: Snapshot::default(),
        snapshots,
        events,
        last_read: Instant::now(),
        owner_generation: Arc::clone(&owner_generation),
        seen_owner: 0,
    };
    let (signal_sender, mut signals) = mpsc::channel(1);
    let _listener = AbortOnDrop(tokio::spawn(watch_changes(signal_sender, owner_generation)));
    let mut pending = Pending::default();
    let mut retries = RetrySchedule::default();
    let mut deadline = Instant::now();
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break; };
                pending.push(command);
                tokio::task::yield_now().await;
                while let Ok(command) = commands.try_recv() { pending.push(command); }
                while let Some(request) = pending.pop() {
                    let result = if matches!(request.command, Command::Refresh) {
                        actor.full_refresh().await
                    } else {
                        actor.execute(&request.command).await
                    };
                    request.finish(&result);
                    while let Ok(command) = commands.try_recv() { pending.push(command); }
                }
                deadline = Instant::now() + retries.next(&actor.state);
            }
            Some(()) = signals.recv() => {
                actor.refresh(true).await;
                deadline = Instant::now() + retries.next(&actor.state);
            }
            () = tokio::time::sleep_until(deadline) => {
                actor.refresh(!actor.state.reliable).await;
                deadline = Instant::now() + retries.next(&actor.state);
            }
        }
    }
}

struct AbortOnDrop(JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Only real session-bus signals are observed. A periodic enumeration during
/// the live pass also recovers from a lost signal or a temporary bus outage.
async fn watch_changes(changed: mpsc::Sender<()>, generation: Arc<AtomicU64>) {
    while !changed.is_closed() {
        if let Err(error) = listen_changes(&changed, &generation).await {
            tracing::debug!(%error, "OpenRazer change listener reconnecting");
            generation.fetch_add(1, Ordering::Release);
        }
        if changed.is_closed() {
            return;
        }
        let _ = changed.try_send(());
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn listen_changes(changed: &mpsc::Sender<()>, generation: &AtomicU64) -> Result<(), String> {
    let client = Client::connect().await.map_err(|error| error.to_string())?;
    let device_rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.razer")
        .interface("razer.devices")
        .path("/org/razer")
        .build()
        .map_err(|error| error.to_string())?;
    let owner_rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .interface("org.freedesktop.DBus")
        .path("/org/freedesktop/DBus")
        .member("NameOwnerChanged")
        .arg(0, "org.razer")
        .build()
        .map_err(|error| error.to_string())?;
    let capacity = std::num::NonZeroUsize::new(16);
    let mut devices = MessageStream::for_match_rule(device_rule, client.connection(), capacity)
        .await
        .map_err(|error| error.to_string())?;
    let mut owners = MessageStream::for_match_rule(owner_rule, client.connection(), capacity)
        .await
        .map_err(|error| error.to_string())?;
    loop {
        tokio::select! {
            () = changed.closed() => return Ok(()),
            message = devices.next() => {
                let Some(Ok(message)) = message else { return Err("device signal stream disconnected".into()); };
                if message.header().member().is_some_and(|member| matches!(member.as_str(), "device_added" | "device_removed")) {
                    let _ = changed.try_send(());
                }
            }
            message = owners.next() => {
                let Some(Ok(_)) = message else { return Err("owner signal stream disconnected".into()); };
                generation.fetch_add(1, Ordering::Release);
                let _ = changed.try_send(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Readback, SETTLE_DELAYS, needs_discovery, next_read};
    use razerr_model::{Availability, UsbId};

    /// Answers with `readbacks` in turn, repeating the last one, and returns the
    /// last answer and how many reads it took.
    fn settle(readbacks: &[Readback]) -> (Readback, usize) {
        let mut delays = SETTLE_DELAYS.into_iter();
        let mut reads = 0;
        loop {
            reads += 1;
            let readback = readbacks[(reads - 1).min(readbacks.len() - 1)];
            if next_read(readback, &mut delays).is_none() {
                return (readback, reads);
            }
        }
    }

    #[test]
    fn a_late_readback_is_not_a_failure() {
        use Readback::{Applied, Different};
        assert_eq!(settle(&[Different, Different, Applied]), (Applied, 3));
    }

    #[test]
    fn an_immediate_readback_needs_no_waiting() {
        assert_eq!(settle(&[Readback::Applied]), (Readback::Applied, 1));
    }

    #[test]
    fn a_value_that_never_arrives_stops_after_the_last_read() {
        assert_eq!(settle(&[Readback::Different]), (Readback::Different, 6));
    }

    #[test]
    fn a_lost_connection_ends_the_wait() {
        use Readback::{Different, Lost};
        assert_eq!(settle(&[Different, Lost]), (Lost, 2));
    }

    #[test]
    fn a_missing_reading_gets_the_same_chance_to_recover() {
        use Readback::{Applied, Unread};
        assert_eq!(settle(&[Unread, Applied]), (Applied, 2));
    }

    #[test]
    fn read_failures_are_concise_and_only_known_unstable_getters_retry() {
        use super::{read_error, retryable_read};
        use razerr_openrazer::ReadWarning;
        let mut warning = ReadWarning {
            serial: "PM2405H30011232".into(),
            operation: "getDPIStages".into(),
            message: "horizontal DPI must be positive".into(),
            transient: false,
        };
        assert!(retryable_read(&warning));
        assert!(read_error(&warning).contains("Could not read DPI stages"));
        warning.operation = "isCharging".into();
        warning.message = "Traceback (most recent call last):\n  File driver.py\nOSError: [Errno 5] Input/output error\n".into();
        assert!(retryable_read(&warning));
        assert!(!read_error(&warning).contains("Traceback"));
        assert!(read_error(&warning).contains("Charging status is unavailable"));
        warning.operation = "getBattery".into();
        assert!(!retryable_read(&warning));
        assert_eq!(
            read_error(&warning),
            "PM2405H30011232: getBattery: OSError: [Errno 5] Input/output error"
        );
    }

    #[test]
    fn coalesced_owner_changes_remain_visible_after_recovery() {
        use super::*;
        let (snapshots, receiver) = watch::channel(Arc::new(Snapshot::default()));
        let (events, _) = broadcast::channel(1);
        let owner_generation = Arc::new(AtomicU64::new(0));
        let mut actor = Actor {
            client: None,
            state: Snapshot::default(),
            snapshots,
            events,
            last_read: Instant::now(),
            owner_generation: Arc::clone(&owner_generation),
            seen_owner: 0,
        };
        owner_generation.fetch_add(2, Ordering::Release);
        assert!(actor.sync_owner());
        assert!(!actor.sync_owner());
        actor.state.daemon_available = true;
        actor.state.reliable = true;
        actor.publish(SnapshotKind::Full);
        // Only the latest watch value is read, but the missed outage persists.
        let latest = receiver.borrow();
        assert!(latest.daemon_available);
        assert_eq!(latest.continuity, 1);
        assert_eq!(latest.daemon_generation, 1);
    }

    #[test]
    fn transport_changes_and_receiver_wakeup_require_new_capabilities() {
        let wired = Some(UsbId {
            vendor: 0x1532,
            product: 0x00c2,
        });
        let wireless = Some(UsbId {
            vendor: 0x1532,
            product: 0x00c3,
        });
        assert!(!needs_discovery(
            wired,
            wired,
            Availability::Responding,
            Availability::Responding
        ));
        assert!(needs_discovery(
            wired,
            wireless,
            Availability::Responding,
            Availability::Responding
        ));
        assert!(needs_discovery(
            wired,
            None,
            Availability::Responding,
            Availability::Responding
        ));
        assert!(needs_discovery(
            wired,
            wired,
            Availability::Unavailable,
            Availability::Responding
        ));
        assert!(!needs_discovery(
            wired,
            wired,
            Availability::Unavailable,
            Availability::Unavailable
        ));
    }
}
