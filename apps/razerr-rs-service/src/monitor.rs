//! Notification decisions, independent of D-Bus and desktop availability.

use std::{collections::HashMap, time::Duration};

use razerr_model::{Availability, Serial, Snapshot, UsbId};

const DISCONNECT_DELAY: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Connected,
    Disconnected,
    ConnectionChanged,
    Low,
    Critical,
}

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Connected => "Device connected",
            Self::Disconnected => "Device disconnected",
            Self::ConnectionChanged => "Connection changed",
            Self::Low => "Low battery",
            Self::Critical => "Battery critically low",
        }
    }

    pub fn is_battery(self) -> bool {
        matches!(self, Self::Low | Self::Critical)
    }
}

#[derive(Clone, Debug)]
pub struct Event {
    pub serial: Serial,
    pub name: String,
    pub kind: Kind,
    pub battery: Option<f64>,
}

#[derive(Default)]
struct Remembered {
    name: String,
    connected: bool,
    seen: bool,
    usb: Option<UsbId>,
    missing_since: Option<Duration>,
    warned: u8,
}

impl Remembered {
    /// Presence is distinct from a usable wireless reading. An unavailable
    /// receiver still exists, but cannot establish a new transport identity.
    fn observe_connection(
        &mut self,
        ready: bool,
        available: bool,
        usb: Option<UsbId>,
    ) -> Option<Kind> {
        let mut event = (!self.connected && ready).then_some(Kind::Connected);
        if available && let Some(usb) = usb.filter(|usb| usb.vendor > 0 && usb.product > 0) {
            if ready && self.connected && self.usb.is_some_and(|previous| previous != usb) {
                event = Some(Kind::ConnectionChanged);
            }
            self.usb = Some(usb);
        }
        self.seen = true;
        self.connected = true;
        self.missing_since = None;
        event
    }

    fn finish_snapshot(&mut self, ready: bool, now: Duration) {
        if self.seen || !self.connected {
            return;
        }
        if ready {
            self.missing_since.get_or_insert(now);
        } else {
            self.connected = false;
        }
    }
}

#[derive(Default)]
pub struct Monitor {
    ready: bool,
    devices: HashMap<Serial, Remembered>,
    continuity: u64,
    daemon_generation: u64,
}

impl Monitor {
    pub fn update(&mut self, snapshot: &Snapshot, now: Duration) -> Vec<Event> {
        self.generations(snapshot.continuity, snapshot.daemon_generation);
        if !self.begin_snapshot(snapshot.daemon_available, snapshot.reliable) {
            return Vec::new();
        }
        let mut events = Vec::new();
        for device in &snapshot.devices {
            let entry = self.devices.entry(device.serial.clone()).or_default();
            entry.name = device
                .name
                .clone()
                .unwrap_or_else(|| device.serial.to_string());
            if let Some(kind) = entry.observe_connection(
                self.ready,
                device.availability != Availability::Unavailable,
                device.usb,
            ) {
                events.push(Event {
                    serial: device.serial.clone(),
                    name: entry.name.clone(),
                    kind,
                    battery: None,
                });
            }
            if device.availability != Availability::Unavailable
                && let Some(battery) = &device.battery
                && let (Some(level), Some(charging)) = (battery.level.fresh(), battery.charging)
                && let Some(threshold) = battery_threshold(device.low_battery.as_ref())
                && let Some(kind) =
                    battery_alert(level.get(), charging, threshold, &mut entry.warned)
            {
                events.push(Event {
                    serial: device.serial.clone(),
                    name: entry.name.clone(),
                    kind,
                    battery: Some(level.get()),
                });
            }
        }
        self.finish_snapshot(now);
        events
    }

    fn generations(&mut self, continuity: u64, daemon: u64) {
        if self.daemon_generation != daemon {
            self.ready = false;
            self.daemon_generation = daemon;
        }
        if self.continuity != continuity {
            for entry in self.devices.values_mut() {
                entry.missing_since = None;
            }
            self.continuity = continuity;
        }
    }

    /// A daemon outage needs a quiet new baseline. An unreliable read only
    /// cancels pending disconnects; it retains the last connection identity.
    fn begin_snapshot(&mut self, daemon_available: bool, reliable: bool) -> bool {
        if !daemon_available || !reliable {
            if !daemon_available {
                self.ready = false;
            }
            for remembered in self.devices.values_mut() {
                remembered.missing_since = None;
            }
            return false;
        }
        for remembered in self.devices.values_mut() {
            remembered.seen = false;
        }
        true
    }

    fn finish_snapshot(&mut self, now: Duration) {
        for entry in self.devices.values_mut() {
            entry.finish_snapshot(self.ready, now);
        }
        self.ready = true;
    }

    /// Consume successful and muted alerts; a failed notification can retry.
    pub fn acknowledge(&mut self, event: &Event) {
        if let Some(entry) = self.devices.get_mut(&event.serial) {
            match event.kind {
                Kind::Low => entry.warned = entry.warned.max(1),
                Kind::Critical => entry.warned = 2,
                _ => {}
            }
        }
    }

    pub fn deadline(&self) -> Option<Duration> {
        self.devices
            .values()
            .filter_map(|entry| entry.missing_since.map(|since| since + DISCONNECT_DELAY))
            .min()
    }

    pub fn tick(&mut self, now: Duration) -> Vec<Event> {
        let mut events = Vec::new();
        for (serial, entry) in &mut self.devices {
            if entry
                .missing_since
                .is_some_and(|since| now >= since + DISCONNECT_DELAY)
            {
                entry.connected = false;
                entry.missing_since = None;
                events.push(Event {
                    serial: serial.clone(),
                    name: entry.name.clone(),
                    kind: Kind::Disconnected,
                    battery: None,
                });
            }
        }
        events
    }
}

fn battery_threshold(
    reading: Option<&razerr_model::Reading<razerr_model::LowBatteryThreshold>>,
) -> Option<f64> {
    match reading {
        None => Some(15.0),
        Some(reading) => reading.displayed().map(|value| f64::from(value.get())),
    }
}

fn battery_alert(level: f64, charging: bool, threshold: f64, warned: &mut u8) -> Option<Kind> {
    if !level.is_finite() || !(0.0..=100.0).contains(&level) {
        return None;
    }
    if charging || threshold <= 0.0 || level > threshold + 5.0 {
        *warned = 0;
    }
    if charging || threshold <= 0.0 || level > threshold {
        return None;
    }
    let severity = if level <= 5.0 { 2 } else { 1 };
    (severity > *warned).then_some(if severity == 2 {
        Kind::Critical
    } else {
        Kind::Low
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_USB: UsbId = UsbId {
        vendor: 1,
        product: 2,
    };
    const SECOND_USB: UsbId = UsbId {
        vendor: 1,
        product: 3,
    };

    // These are reducer keys and numeric connection states, not Device fixtures.
    fn connected_monitor() -> (Monitor, Serial) {
        let key = Serial::new("connection_state").expect("state key");
        let entry = Remembered {
            connected: true,
            usb: Some(FIRST_USB),
            ..Remembered::default()
        };
        (
            Monitor {
                ready: true,
                devices: HashMap::from([(key.clone(), entry)]),
                ..Monitor::default()
            },
            key,
        )
    }

    fn connection(
        monitor: &mut Monitor,
        key: &Serial,
        available: bool,
        usb: Option<UsbId>,
    ) -> Option<Kind> {
        monitor
            .devices
            .get_mut(key)
            .expect("remembered state")
            .observe_connection(monitor.ready, available, usb)
    }

    fn absent(monitor: &mut Monitor, now: u64) {
        assert!(monitor.begin_snapshot(true, true));
        monitor.finish_snapshot(Duration::from_secs(now));
    }

    fn check_battery(monitor: &mut Monitor, key: &Serial, level: f64) -> Option<Kind> {
        battery_alert(
            level,
            false,
            15.0,
            &mut monitor.devices.get_mut(key).expect("state").warned,
        )
    }

    #[test]
    fn generations_preserve_missed_outages_and_observation_gaps() {
        let (mut monitor, key) = connected_monitor();
        absent(&mut monitor, 10);
        assert!(monitor.deadline().is_some());
        monitor.generations(1, 0);
        assert!(monitor.ready);
        assert!(monitor.deadline().is_none());
        absent(&mut monitor, 11);
        monitor.generations(2, 1);
        assert!(!monitor.ready);
        assert!(monitor.deadline().is_none());
        assert_eq!(connection(&mut monitor, &key, true, Some(SECOND_USB)), None);
    }

    #[test]
    fn a_disconnect_waits_three_seconds_and_is_emitted_once() {
        let (mut monitor, key) = connected_monitor();
        absent(&mut monitor, 10);
        assert_eq!(monitor.deadline(), Some(Duration::from_secs(13)));
        absent(&mut monitor, 11); // repeated missing scans must not move the deadline
        assert_eq!(monitor.deadline(), Some(Duration::from_secs(13)));
        assert!(monitor.tick(Duration::from_millis(12_999)).is_empty());
        let events = monitor.tick(Duration::from_secs(13));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Disconnected);
        assert_eq!(events[0].serial, key);
        assert!(!monitor.devices[&key].connected);
        assert_eq!(monitor.deadline(), None);
        assert!(monitor.tick(Duration::from_secs(20)).is_empty());

        assert!(monitor.begin_snapshot(true, true));
        assert_eq!(
            connection(&mut monitor, &key, true, Some(SECOND_USB)),
            Some(Kind::Connected)
        );
        monitor.finish_snapshot(Duration::from_secs(21));
        assert_eq!(connection(&mut monitor, &key, true, Some(SECOND_USB)), None);
    }

    #[test]
    fn a_brief_absence_cancels_disconnect_without_a_reconnect_alert() {
        let (mut monitor, key) = connected_monitor();
        absent(&mut monitor, 10);
        assert!(monitor.begin_snapshot(true, true));
        assert_eq!(connection(&mut monitor, &key, true, Some(FIRST_USB)), None);
        monitor.finish_snapshot(Duration::from_secs(12));
        assert_eq!(monitor.deadline(), None);
        assert!(monitor.tick(Duration::from_secs(20)).is_empty());
        assert!(monitor.devices[&key].connected);
    }

    #[test]
    fn unreliable_reads_cancel_deadlines_but_keep_transport_identity() {
        let (mut monitor, key) = connected_monitor();
        absent(&mut monitor, 10);
        assert!(!monitor.begin_snapshot(true, false));
        assert!(monitor.ready);
        assert_eq!(monitor.deadline(), None);
        assert!(monitor.tick(Duration::from_secs(20)).is_empty());
        assert_eq!(monitor.devices[&key].usb, Some(FIRST_USB));

        assert!(monitor.begin_snapshot(true, true));
        assert_eq!(
            connection(&mut monitor, &key, true, Some(SECOND_USB)),
            Some(Kind::ConnectionChanged)
        );
        monitor.finish_snapshot(Duration::from_secs(21));
        assert_eq!(connection(&mut monitor, &key, true, Some(SECOND_USB)), None);
    }

    #[test]
    fn daemon_loss_cancels_disconnects_and_reestablishes_a_quiet_baseline() {
        let (mut monitor, key) = connected_monitor();
        absent(&mut monitor, 10);
        assert!(!monitor.begin_snapshot(false, false));
        assert!(!monitor.ready);
        assert_eq!(monitor.deadline(), None);
        assert!(monitor.tick(Duration::from_secs(20)).is_empty());

        assert!(monitor.begin_snapshot(true, true));
        assert_eq!(connection(&mut monitor, &key, true, Some(SECOND_USB)), None);
        monitor.finish_snapshot(Duration::from_secs(21));
        assert!(monitor.ready);
        assert_eq!(
            connection(&mut monitor, &key, true, Some(FIRST_USB)),
            Some(Kind::ConnectionChanged)
        );
    }

    #[test]
    fn devices_absent_after_daemon_restart_are_baselined_without_disconnects() {
        let (mut monitor, key) = connected_monitor();
        assert!(!monitor.begin_snapshot(false, false));
        absent(&mut monitor, 10);
        assert!(!monitor.devices[&key].connected);
        assert_eq!(monitor.deadline(), None);
        assert!(monitor.tick(Duration::from_secs(20)).is_empty());
        assert_eq!(
            connection(&mut monitor, &key, true, Some(SECOND_USB)),
            Some(Kind::Connected)
        );
    }

    #[test]
    fn only_usable_usb_identity_changes_count_as_transport_handoffs() {
        let mut entry = Remembered {
            warned: 1,
            ..Remembered::default()
        };
        assert_eq!(entry.observe_connection(false, true, None), None);
        assert_eq!(entry.observe_connection(true, true, Some(FIRST_USB)), None);
        assert_eq!(
            entry.observe_connection(true, true, Some(SECOND_USB)),
            Some(Kind::ConnectionChanged)
        );
        assert_eq!(entry.observe_connection(true, true, Some(SECOND_USB)), None);
        assert_eq!(entry.observe_connection(true, false, Some(FIRST_USB)), None);
        for usb in [
            None,
            Some(UsbId {
                vendor: 0,
                product: 2,
            }),
            Some(UsbId {
                vendor: 1,
                product: 0,
            }),
        ] {
            assert_eq!(entry.observe_connection(true, true, usb), None);
            assert_eq!(entry.usb, Some(SECOND_USB));
        }
        entry.missing_since = Some(Duration::from_secs(10));
        assert_eq!(
            entry.observe_connection(true, true, Some(FIRST_USB)),
            Some(Kind::ConnectionChanged)
        );
        assert_eq!(entry.missing_since, None);
        assert_eq!(entry.warned, 1); // a cable switch must not replay a battery alert
    }

    #[test]
    fn acknowledging_delivered_or_muted_battery_alerts_prevents_replay() {
        let (mut monitor, key) = connected_monitor();
        let low = Event {
            serial: key.clone(),
            name: String::new(),
            kind: Kind::Low,
            battery: Some(14.0),
        };
        let critical = Event {
            kind: Kind::Critical,
            battery: Some(5.0),
            ..low.clone()
        };
        for _ in 0..2 {
            assert_eq!(check_battery(&mut monitor, &key, 14.0), Some(Kind::Low));
        } // failed delivery is not acknowledged and remains eligible
        monitor.acknowledge(&low); // caller uses the same path for successful and muted alerts
        assert_eq!(check_battery(&mut monitor, &key, 14.0), None);
        assert_eq!(check_battery(&mut monitor, &key, 5.0), Some(Kind::Critical));
        monitor.acknowledge(&critical);
        monitor.acknowledge(&low); // a late lower-severity acknowledgement cannot downgrade it
        assert_eq!(monitor.devices[&key].warned, 2);
        assert_eq!(check_battery(&mut monitor, &key, 5.0), None);
        monitor.acknowledge(&Event {
            kind: Kind::Connected,
            ..low
        });
        assert_eq!(monitor.devices[&key].warned, 2);
    }

    #[test]
    fn an_unknown_advertised_threshold_does_not_use_the_default() {
        use razerr_model::{LowBatteryThreshold, Reading};
        assert_eq!(battery_threshold(None), Some(15.0));
        assert_eq!(battery_threshold(Some(&Reading::Unavailable)), None);
        let threshold = LowBatteryThreshold::new(5).expect("threshold");
        assert_eq!(
            battery_threshold(Some(&Reading::Fresh(threshold))),
            Some(5.0)
        );
        let stale = Reading::Stale {
            value: threshold,
            age: Duration::from_secs(20),
        };
        assert_eq!(battery_threshold(Some(&stale)), Some(5.0));
    }

    #[test]
    fn battery_hysteresis_requires_acknowledgment_and_rearms() {
        let mut warned = 0;
        assert_eq!(
            battery_alert(15.0, false, 15.0, &mut warned),
            Some(Kind::Low)
        );
        assert_eq!(
            battery_alert(14.0, false, 15.0, &mut warned),
            Some(Kind::Low)
        ); // failed delivery
        warned = 1; // successful or muted
        assert_eq!(battery_alert(16.0, false, 15.0, &mut warned), None);
        assert_eq!(battery_alert(14.0, false, 15.0, &mut warned), None);
        assert_eq!(
            battery_alert(5.0, false, 15.0, &mut warned),
            Some(Kind::Critical)
        );
        warned = 2;
        assert_eq!(battery_alert(5.0, false, 15.0, &mut warned), None);
        assert_eq!(battery_alert(14.0, true, 15.0, &mut warned), None);
        assert_eq!(warned, 0);
        assert_eq!(
            battery_alert(14.0, false, 15.0, &mut warned),
            Some(Kind::Low)
        );
        warned = 1;
        assert_eq!(battery_alert(21.0, false, 15.0, &mut warned), None);
        assert_eq!(warned, 0);
        assert_eq!(battery_alert(f64::NAN, false, 15.0, &mut warned), None);
        assert_eq!(battery_alert(0.0, false, 0.0, &mut warned), None);
    }
}
