use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use razerr_metrics::{Point, Prediction, Sample, Store, Usage};
use razerr_model::{Device, Serial};

#[derive(Clone, Debug, Default)]
pub struct History {
    pub serial: Option<Serial>,
    pub points: Vec<Point>,
    pub prediction: Option<Prediction>,
    pub usage: Usage,
    pub error: Option<String>,
    pub now: i64,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_secs()).ok())
        .unwrap_or_default()
}

pub async fn load(path: PathBuf, device: Option<Device>, allow_prediction: bool) -> History {
    let serial = device.as_ref().map(|device| device.serial.clone());
    let fallback = serial.clone();
    tokio::task::spawn_blocking(move || {
        let now = now();
        let mut history = History {
            serial,
            now,
            ..History::default()
        };
        if !path.exists() {
            return history;
        }
        let result = (|| {
            let mut store = Store::open_reader(&path).map_err(|error| error.to_string())?;
            history.usage = store.usage().map_err(|error| error.to_string())?;
            if let Some(device) = &device {
                history.points = store
                    .history(&device.serial, now.saturating_sub(86400), 512)
                    .map_err(|error| error.to_string())?;
                if allow_prediction && let Some(sample) = sample(device, now) {
                    history.prediction =
                        store.predict(&sample).map_err(|error| error.to_string())?;
                }
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            history.error = Some(error);
        }
        history
    })
    .await
    .unwrap_or_else(|error| History {
        serial: fallback,
        error: Some(error.to_string()),
        ..History::default()
    })
}

fn sample(device: &Device, timestamp: i64) -> Option<Sample> {
    if device.availability != razerr_model::Availability::Responding {
        return None;
    }
    let battery = device.battery.as_ref()?;
    let poll_rate = match &device.polling {
        Some(polling) => polling.current.fresh()?.get(),
        None if device.capabilities.poll_rate => return None,
        None => 0,
    };
    Some(Sample {
        serial: device.serial.clone(),
        timestamp,
        monotonic: Duration::ZERO,
        battery: *battery.level.fresh()?,
        charging: battery.charging?,
        usb: device.usb?,
        poll_rate,
    })
}

pub fn estimate(prediction: Option<&Prediction>) -> String {
    prediction.map_or_else(
        || "Learning from your battery history…".into(),
        |prediction| {
            let hours = prediction.hours_remaining;
            let destination = if prediction.charging {
                "until full"
            } else {
                "remaining"
            };
            if hours < 1.0 {
                format!("About {:.0} min {destination}", hours * 60.0)
            } else {
                format!("About {hours:.1} h {destination}")
            }
        },
    )
}

pub fn details(prediction: Option<&Prediction>) -> String {
    prediction.map_or_else(|| "Estimates appear only after enough real observations. Charging needs evidence in every remaining charge band.".into(), |value| {
        format!("Range {:.1}–{:.1} h · {:?} confidence · {} rate context · {} intervals over {:.0} min",
            value.hours_min,value.hours_max,value.confidence,if value.exact_context {"exact"} else {"nearby"},value.intervals,value.observed_seconds/60.0)
    })
}
