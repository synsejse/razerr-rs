//! Numeric scenarios from `Chromaven/tests/test_metrics.c`, retaining its epochs,
//! time steps, sensor quantization, rates, thresholds and expected tolerances.

use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};

use diesel::{Connection, SqliteConnection, connection::SimpleConnection};
use razerr_metrics::{Confidence, Sample, Store};
use razerr_model::{Percent, Serial, UsbId};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
const EPOCH: i64 = 1_700_000_000;

fn serial(value: &str) -> Serial {
    Serial::new(value).expect("numeric scenario label")
}

fn sample(label: &str, minutes: u32, battery: f64) -> Sample {
    Sample {
        serial: serial(label),
        timestamp: EPOCH + i64::from(minutes) * 60,
        monotonic: Duration::from_secs(1000 + u64::from(minutes) * 60),
        battery: Percent::new(battery).expect("valid sample vector"),
        charging: false,
        poll_rate: 1000,
        usb: UsbId {
            vendor: 0x1532,
            product: 0x00c3,
        },
    }
}

fn fixture() -> Result<(tempfile::TempDir, Store)> {
    let directory = tempfile::tempdir()?;
    let store = Store::open_writer(&directory.path().join("history.sqlite3"))?;
    Ok((directory, store))
}

fn discharge(
    store: &mut Store,
    label: &str,
    start: u32,
    minutes: u32,
    battery: f64,
    per_hour: f64,
    poll_rate: u16,
) -> Result<Sample> {
    let mut current = sample(label, start, battery);
    for minute in 0..=minutes {
        current = sample(
            label,
            start + minute,
            (battery - per_hour * f64::from(minute) / 60.0 + 0.00001).floor(),
        );
        current.poll_rate = poll_rate;
        store.record(&current)?;
    }
    Ok(current)
}

#[test]
fn unchanged_subsecond_readbacks_keep_sampling_and_learning_continuity() -> Result {
    let (_directory, mut store) = fixture()?;
    let mut current = sample("rapid_readback", 0, 80.0);
    assert!(store.record(&current)?);
    for _ in 0..9 {
        current.monotonic += Duration::from_millis(100);
        assert!(!store.record(&current)?);
    }
    assert_eq!(store.usage()?.samples, 1);
    current = sample("rapid_readback", 1, 79.0);
    assert!(store.record(&current)?);
    let points = store.history(&current.serial, EPOCH, 32)?;
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].session, points[1].session);
    // A different context at the same wall-clock second is not a duplicate.
    current.monotonic += Duration::from_millis(100);
    current.poll_rate = 8000;
    assert!(store.record(&current)?);
    let points = store.history(&current.serial, EPOCH, 32)?;
    assert_ne!(points[0].session, points[1].session);
    assert_eq!(store.usage()?.samples, 3);
    Ok(())
}

#[test]
fn sampling_reference_vectors() -> Result {
    let (_directory, mut store) = fixture()?;
    let mut current = sample("sampling", 0, 80.0);
    assert!(store.record(&current)?);
    assert!(!store.record(&current)?);
    for seconds in (20_u32..=120).step_by(20) {
        current.timestamp = EPOCH + i64::from(seconds);
        current.monotonic = Duration::from_secs(1000 + u64::from(seconds));
        assert_eq!(store.record(&current)?, seconds % 60 == 0);
    }
    current.timestamp += 20;
    current.monotonic += Duration::from_secs(20);
    current.battery = Percent::new(79.0)?;
    assert!(store.record(&current)?);
    let points = store.history(&serial("sampling"), EPOCH, 32)?;
    assert_eq!(points.len(), 4);
    assert_eq!(points[0].battery, 80.0);
    assert_eq!(points[3].battery, 79.0);
    assert_eq!(points[0].session, points[3].session);
    let points = store.history(&serial("sampling"), EPOCH, 2)?;
    assert_eq!(points.len(), 2);
    assert!(points[0].timestamp < points[1].timestamp);
    assert_eq!(points[1].timestamp, current.timestamp);
    assert_eq!(store.history(&serial("unknown"), EPOCH, 32)?.len(), 0);
    assert!(store.history(&serial("sampling"), EPOCH, 4097).is_err());
    assert!(store.configure(9, 90).is_err());
    assert!(store.configure(60, 366).is_err());
    store.configure(3600, 90)?;
    current = sample("hourly", 0, 50.0);
    assert!(store.record(&current)?);
    for minute in 1..=60 {
        current = sample("hourly", minute, 50.0);
        assert_eq!(store.record(&current)?, minute == 60);
    }
    let points = store.history(&serial("hourly"), EPOCH, 32)?;
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].session, points[1].session);
    Ok(())
}

fn discharge_vectors(store: &mut Store) -> Result<Sample> {
    let mut current = discharge(store, "mouse", 0, 20, 90.0, 12.0, 1000)?;
    assert!(store.predict(&current)?.is_none());
    current = discharge(store, "mouse", 21, 79, 85.8, 12.0, 1000)?;
    let prediction = store
        .predict(&current)?
        .ok_or("missing discharge prediction")?;
    assert!(!prediction.charging && prediction.exact_context);
    assert!((prediction.percent_per_hour - 12.0).abs() < 0.7);
    assert!((prediction.hours_remaining - current.battery.get() / 12.0).abs() < 0.4);
    assert!(
        prediction.hours_min < prediction.hours_remaining
            && prediction.hours_max > prediction.hours_remaining
    );
    assert!(prediction.intervals >= 8 && prediction.observed_seconds >= 5400.0);
    current.poll_rate = 500;
    let prediction = store.predict(&current)?.ok_or("nearby rate prediction")?;
    assert!(!prediction.exact_context && prediction.confidence == Confidence::Low);
    current.poll_rate = 8000;
    assert!(store.predict(&current)?.is_none());
    current.poll_rate = 1000;
    current.usb.product = 0x00c2;
    assert!(store.predict(&current)?.is_none());
    current.usb.product = 0x00c3;
    current.serial = serial("another_device");
    assert!(store.predict(&current)?.is_none());
    current = discharge(store, "mouse", 101, 80, 69.0, 24.0, 8000)?;
    let prediction = store.predict(&current)?.ok_or("8000Hz prediction")?;
    assert!(prediction.exact_context && (prediction.percent_per_hour - 24.0).abs() < 0.8);
    current.poll_rate = 1000;
    let prediction = store.predict(&current)?.ok_or("1000Hz prediction")?;
    assert!(prediction.exact_context && (prediction.percent_per_hour - 12.0).abs() < 0.7);
    current.timestamp -= 86400;
    assert!(store.predict(&current)?.is_none());
    Ok(current)
}

#[test]
fn discharge_reference_vectors() -> Result {
    let (_directory, mut store) = fixture()?;
    discharge_vectors(&mut store)?;
    Ok(())
}

#[test]
fn charging_reference_vectors() -> Result {
    let (_directory, mut store) = fixture()?;
    let mut current = sample("charging", 0, 20.0);
    for minute in 0..=120 {
        let battery = if minute <= 60 {
            20.0 + f64::from(minute)
        } else if minute <= 90 {
            80.0 + f64::from(minute - 60) / 2.0
        } else {
            95.0 + f64::from(minute - 90) / 6.0
        };
        current = sample("charging", minute, battery);
        current.charging = true;
        store.record(&current)?;
        if minute == 90 {
            assert!(store.predict(&current)?.is_none());
        }
    }
    assert!(store.predict(&current)?.is_none());
    current.battery = Percent::new(80.0)?;
    let prediction = store.predict(&current)?.ok_or("charging prediction")?;
    assert!(prediction.charging && prediction.exact_context);
    assert!((prediction.percent_per_hour - 30.0).abs() < 0.1);
    assert!((prediction.hours_remaining - 1.0).abs() < 0.1);
    current.battery = Percent::new(98.0)?;
    let prediction = store.predict(&current)?.ok_or("98% prediction")?;
    assert!((prediction.hours_remaining - 0.2).abs() < 0.02);
    current.charging = false;
    assert!(store.predict(&current)?.is_none());

    for minute in 0..=84 {
        let battery = if minute <= 24 {
            95.0 + f64::from(minute) / 6.0
        } else {
            99.0 + f64::from(minute - 24) / 60.0
        };
        current = sample("slow_tail", minute, battery);
        current.charging = true;
        store.record(&current)?;
        if minute == 24 {
            assert!(store.predict(&current)?.is_none());
        }
    }
    current.battery = Percent::new(99.0)?;
    let prediction = store.predict(&current)?.ok_or("slow-tail prediction")?;
    assert!((prediction.percent_per_hour - 1.0).abs() < 0.01);
    assert!((prediction.hours_remaining - 1.0).abs() < 0.01);
    assert!(prediction.hours_max > 1.0);

    for (label, pattern) in [("quantized_charge", 0), ("quantized_slow_charge", 1)] {
        let start = 219 + pattern;
        let step_minutes = 1 + pattern;
        for cycle in 0..2 {
            store.gap(Some(&serial(label)));
            for step in start..=255 {
                current = sample(
                    label,
                    cycle * 120 + (step - start) * step_minutes,
                    f64::from(step) * 100.0 / 255.0,
                );
                current.charging = true;
                store.record(&current)?;
                if cycle == 0 && step == 254 {
                    assert!(store.predict(&current)?.is_none());
                }
            }
        }
        let rate = 6000.0 / f64::from(255 * step_minutes);
        current.battery = Percent::new(95.0)?;
        let prediction = store.predict(&current)?.ok_or("quantized prediction")?;
        assert!(prediction.charging && prediction.exact_context);
        assert!((prediction.hours_remaining - 5.0 / rate).abs() < 0.01);
        current.battery = Percent::new(99.0)?;
        let prediction = store
            .predict(&current)?
            .ok_or("quantized tail prediction")?;
        assert!((prediction.percent_per_hour - rate).abs() < 0.01);
    }
    Ok(())
}

#[test]
fn continuity_reference_vectors() -> Result {
    let (_directory, mut store) = fixture()?;
    assert!(store.record(&sample("breaks", 0, 90.0))?);
    assert!(store.record(&sample("breaks", 1, 89.0))?);
    store.gap(Some(&serial("breaks")));
    assert!(store.record(&sample("breaks", 2, 88.0))?);
    let mut current = sample("breaks", 3, 87.0);
    current.monotonic -= Duration::from_secs(50);
    assert!(store.record(&current)?);
    assert!(store.record(&sample("breaks", 20, 80.0))?);
    assert!(store.record(&sample("breaks", 19, 79.0))?);
    current = sample("breaks", 21, 78.0);
    current.poll_rate = 8000;
    assert!(store.record(&current)?);
    current = sample("breaks", 22, 77.0);
    current.usb.product += 1;
    assert!(store.record(&current)?);
    current = sample("breaks", 23, 78.0);
    current.charging = true;
    assert!(store.record(&current)?);
    // Typed Percent rejects NaN before the recorder; the telemetry caller marks a gap.
    assert!(Percent::new(f64::NAN).is_err());
    store.gap(Some(&serial("breaks")));
    current = sample("breaks", 25, 76.0);
    assert!(store.record(&current)?);
    let points = store.history(&serial("breaks"), EPOCH, 64)?;
    assert_eq!(points.len(), 10);
    assert_eq!(points[0].session, points[1].session);
    for pair in points[1..].windows(2) {
        assert_ne!(pair[0].session, pair[1].session);
    }
    assert!(store.predict(&current)?.is_none());
    assert!(Percent::new(-1.0).is_err());
    assert!(Percent::new(101.0).is_err());
    current.timestamp = 0;
    assert!(store.record(&current).is_err());
    Ok(())
}

#[test]
fn persistence_restart_pruning_and_usage_reference_vectors() -> Result {
    let (directory, mut store) = fixture()?;
    discharge_vectors(&mut store)?;
    let path = directory.path().join("history.sqlite3");
    let mut reader = Store::open_reader(&path)?;
    let mut current = sample("mouse", 182, 37.0);
    current.poll_rate = 8000;
    assert!(reader.predict(&current)?.is_some());
    assert!(reader.record(&current).is_err());
    assert!(reader.clear(None).is_err());
    assert!(reader.prune(current.timestamp).is_err());
    let before = reader.usage()?;
    assert!(before.samples > 0);
    let mut bytes = 0;
    for suffix in ["", "-wal", "-shm"] {
        bytes += fs::metadata(directory.path().join(format!("history.sqlite3{suffix}")))?.len();
    }
    assert_eq!(before.bytes, bytes);
    assert_eq!(reader.history(&serial("live"), EPOCH, 8)?.len(), 0);
    assert!(store.record(&sample("live", 0, 50.0))?);
    assert_eq!(reader.history(&serial("live"), EPOCH, 8)?.len(), 1);
    assert_eq!(reader.usage()?.samples, before.samples + 1);
    store.clear(Some(&serial("live")))?;
    assert_eq!(reader.history(&serial("live"), EPOCH, 8)?.len(), 0);
    assert_eq!(reader.usage()?.samples, before.samples);
    drop(reader);
    drop(store);
    assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
    let mut store = Store::open_writer(&path)?;
    let previous_session = store.history(&serial("mouse"), EPOCH, 2)?[1].session;
    assert!(store.record(&current)?);
    let points = store.history(&serial("mouse"), current.timestamp, 2)?;
    assert_eq!(points.len(), 1);
    assert_ne!(points[0].session, previous_session);
    assert!(store.predict(&current)?.is_some());
    store.configure(60, 1)?;
    store.prune(EPOCH + 2 * 86400)?;
    assert_eq!(store.history(&serial("mouse"), EPOCH, 2)?.len(), 0);
    assert!(store.predict(&current)?.is_none());
    assert!(store.record(&sample("after_prune", 3000, 50.0))?);
    store.clear(None)?;
    assert_eq!(store.history(&serial("after_prune"), 0, 2)?.len(), 0);
    let usage = store.usage()?;
    assert_eq!(usage.samples, 0);
    assert!(usage.bytes > 0);
    Ok(())
}

#[test]
fn missing_readonly_corrupt_and_newer_databases_are_preserved() -> Result {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("missing.sqlite3");
    assert!(Store::open_reader(&path).is_err());
    assert!(!path.exists());
    assert!(Store::open_writer(Path::new("relative.sqlite3")).is_err());
    fs::write(&path, "corrupt database, not SQLite")?;
    assert!(Store::open_writer(&path).is_err());
    assert_eq!(fs::read_to_string(&path)?, "corrupt database, not SQLite");
    fs::remove_file(&path)?;
    let mut connection = SqliteConnection::establish(path.to_str().ok_or("path")?)?;
    assert!(matches!(
        Store::open_reader(&path),
        Err(razerr_metrics::Error::NeedsMigration)
    ));
    // A read-only open must not even create Diesel's migration metadata table.
    connection.batch_execute("CREATE TABLE __diesel_schema_migrations (version VARCHAR(50) PRIMARY KEY NOT NULL, run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP); INSERT INTO __diesel_schema_migrations (version) VALUES ('9999');")?;
    assert!(matches!(
        Store::open_writer(&path),
        Err(razerr_metrics::Error::TooNew(_))
    ));
    assert!(matches!(
        Store::open_reader(&path),
        Err(razerr_metrics::Error::TooNew(_))
    ));
    Ok(())
}

#[test]
fn simultaneous_writers_apply_migrations_once() -> Result {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("concurrent.sqlite3");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut store = Store::open_writer(&path)?;
                store.usage()
            })
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().expect("writer thread")?.samples, 0);
    }
    assert_eq!(Store::open_reader(&path)?.usage()?.samples, 0);
    Ok(())
}

#[test]
fn database_paths_with_uri_reserved_characters_roundtrip() -> Result {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("history?# %ü.sqlite3");
    let mut writer = Store::open_writer(&path)?;
    writer.record(&sample("path", 0, 50.0))?;
    let mut reader = Store::open_reader(&path)?;
    assert_eq!(reader.usage()?.samples, 1);
    assert!(path.exists());
    Ok(())
}
