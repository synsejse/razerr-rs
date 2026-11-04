//! An offline stand-in for the service, for `razerr-rs --demo` and the render
//! tests. It uses no D-Bus and no files, so it can't reach a real device.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use razerr_metrics::{Confidence, Point, Prediction, Usage};
use razerr_model::{Command, Device, Reading, Snapshot, SnapshotKind};

use crate::history::History;

/// How long a simulated write takes, so its progress can be seen.
const WRITE: Duration = Duration::from_millis(700);

fn fixture() -> Snapshot {
    serde_json::from_str(include_str!("../fixtures/demo.json")).expect("demo snapshot")
}

/// The simulated service: it applies commands to the fixture, and nothing else.
#[derive(Clone)]
pub struct Demo(Arc<Mutex<Snapshot>>);

impl Demo {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(fixture())))
    }
    pub fn snapshot(&self) -> Arc<Snapshot> {
        Arc::new(self.0.lock().expect("demo state").clone())
    }
    pub async fn send(&self, command: Command) -> Result<(), String> {
        tokio::time::sleep(WRITE).await;
        let mut snapshot = self.0.lock().expect("demo state");
        snapshot.revision += 1;
        snapshot.kind = SnapshotKind::Full;
        if let Some(serial) = command.serial()
            && let Some(device) = snapshot
                .devices
                .iter_mut()
                .find(|device| &device.serial == serial)
        {
            apply(device, command);
        }
        Ok(())
    }
}

fn apply(device: &mut Device, command: Command) {
    match command {
        Command::Refresh => {}
        Command::SetDpi { dpi, .. } => {
            if let Some(control) = &mut device.dpi {
                control.current = Reading::Fresh(dpi);
            }
        }
        Command::SetDpiStages { stages, .. } => {
            if let Some(control) = &mut device.dpi {
                control.current = Reading::Fresh(stages.values()[usize::from(stages.active()) - 1]);
            }
            device.stages = Some(Reading::Fresh(stages));
        }
        Command::SetPollRate { rate, .. } => {
            if let Some(polling) = &mut device.polling {
                polling.current = Reading::Fresh(rate);
            }
        }
        Command::SetIdleTime { seconds, .. } => device.idle_time = Some(Reading::Fresh(seconds)),
        Command::SetLowBattery { percent, .. } => {
            device.low_battery = Some(Reading::Fresh(percent));
        }
        Command::SetBrightness {
            zone, brightness, ..
        } => {
            if let Some(zone) = device.zones.iter_mut().find(|item| item.id == zone) {
                zone.brightness = Some(Reading::Fresh(brightness));
            }
        }
        Command::SetEffect { zone, effect, .. } => {
            if let Some(zone) = device.zones.iter_mut().find(|item| item.id == zone) {
                zone.effect = Reading::Fresh(effect);
            }
        }
    }
}

/// A made-up day of battery use: a long discharge, a charge and a short session.
pub fn history(device: Option<&Device>) -> History {
    let now = razerr_metrics::unix_now();
    let serial = device.map(|device| device.serial.clone());
    if device.is_none_or(|device| device.battery.is_none()) {
        return History {
            serial,
            now,
            ..History::default()
        };
    }
    let point = |minutes_ago: i64, session, battery, charging| Point {
        timestamp: now - minutes_ago * 60,
        session,
        battery,
        charging,
        poll_rate: 1000,
    };
    let mut points = Vec::new();
    for step in 0..72_i32 {
        let battery = 92.0 - f64::from(step) * 0.8;
        points.push(point(1380 - i64::from(step) * 8, 1, battery, false));
    }
    for step in 0..30_i32 {
        let battery = 34.0 + f64::from(step) * 2.0;
        points.push(point(780 - i64::from(step) * 5, 2, battery, true));
    }
    for step in 0..75_i32 {
        let battery = 92.0 - f64::from(step) * 0.27;
        points.push(point(620 - i64::from(step) * 8, 3, battery, false));
    }
    History {
        serial,
        usage: Usage {
            samples: 1380,
            bytes: 286_720,
        },
        prediction: Some(Prediction {
            charging: false,
            exact_context: true,
            confidence: Confidence::High,
            percent_per_hour: 2.1,
            hours_remaining: 34.0,
            hours_min: 29.5,
            hours_max: 38.0,
            observed_seconds: 41_400.0,
            intervals: 177,
        }),
        points,
        error: None,
        now,
    }
}
