use std::{
    collections::HashMap,
    fs,
    fs::OpenOptions,
    os::unix::{
        ffi::OsStrExt,
        fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::Duration,
};

use diesel::{
    dsl::{max, min},
    prelude::*,
    sql_types::{BigInt, Bool, Double, Integer, Text},
};
use razerr_model::Serial;

use crate::{
    Error, Point, Prediction, RateInterval, Sample, Usage, migrations, predict,
    schema::{intervals, samples},
};

#[derive(Clone)]
struct Tracking {
    last: Sample,
    anchor: Sample,
    session: i64,
    stored_at: i64,
    active: bool,
}

pub struct Store {
    connection: SqliteConnection,
    path: PathBuf,
    writable: bool,
    interval_seconds: u16,
    retention_days: u16,
    tracking: HashMap<Serial, Tracking>,
}

impl Store {
    pub fn open_writer(path: &Path) -> Result<Self, Error> {
        Self::open(path, true)
    }

    pub fn open_reader(path: &Path) -> Result<Self, Error> {
        Self::open(path, false)
    }

    fn open(path: &Path, writable: bool) -> Result<Self, Error> {
        if !path.is_absolute() {
            return Err(Error::RelativePath(path.to_owned()));
        }
        if writable {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
            {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if fs::symlink_metadata(path)
                        .map_err(|source| Error::io(path, source))?
                        .file_type()
                        .is_symlink()
                    {
                        return Err(Error::Symlink(path.to_owned()));
                    }
                }
                Err(source) => return Err(Error::io(path, source)),
            }
        }
        // Diesel 2.3 opens with SQLITE_OPEN_URI. mode=ro prevents creation even
        // if the file disappears between a caller's existence check and open.
        let mut connection = SqliteConnection::establish(&database_uri(path, writable))?;
        migrations::configure(&mut connection, writable)?;
        if writable {
            migrations::migrate(&mut connection, migrations::MIGRATIONS)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|source| Error::io(path, source))?;
        } else {
            migrations::check_reader(&mut connection)?;
        }
        Ok(Self {
            connection,
            path: path.to_owned(),
            writable,
            interval_seconds: 60,
            retention_days: 90,
            tracking: HashMap::new(),
        })
    }

    /// The engine supports wide sampling ranges for numeric experiments;
    /// application preferences enforce 20–300 seconds and 7–365 days.
    pub fn configure(&mut self, interval_seconds: u16, retention_days: u16) -> Result<(), Error> {
        if !(10..=3600).contains(&interval_seconds) || !(1..=365).contains(&retention_days) {
            return Err(Error::Invalid("sampling or retention out of range"));
        }
        if interval_seconds != self.interval_seconds {
            self.gap(None);
        }
        self.interval_seconds = interval_seconds;
        self.retention_days = retention_days;
        Ok(())
    }

    pub fn gap(&mut self, serial: Option<&Serial>) {
        for (key, state) in &mut self.tracking {
            if serial.is_none_or(|serial| serial == key) {
                state.active = false;
            }
        }
    }

    pub fn record(&mut self, sample: &Sample) -> Result<bool, Error> {
        self.require_writer()?;
        if sample.timestamp <= 0 {
            self.gap(Some(&sample.serial));
            return Err(Error::Invalid("sample timestamp must be positive"));
        }
        let previous = self.tracking.get(&sample.serial).cloned();
        if previous
            .as_ref()
            .is_some_and(|state| state.active && duplicate_observation(&state.last, sample))
        {
            return Ok(false);
        }
        let restart = previous
            .as_ref()
            .is_none_or(|state| !state.active || !continuous(&state.last, sample));
        let learn = !restart
            && previous
                .as_ref()
                .is_some_and(|state| enough_change(&state.anchor, sample));
        let changed = previous
            .as_ref()
            .is_none_or(|state| !state.active || state.last.battery != sample.battery);
        if !restart
            && !changed
            && !learn
            && previous.as_ref().is_some_and(|state| {
                sample.timestamp - state.stored_at < i64::from(self.interval_seconds)
            })
        {
            if let Some(state) = self.tracking.get_mut(&sample.serial) {
                state.last = sample.clone();
            }
            return Ok(false);
        }
        let written = self
            .connection
            .immediate_transaction::<_, Error, _>(|connection| {
                let id = next_sample_id(connection)?;
                let session = if restart {
                    id
                } else {
                    previous
                        .as_ref()
                        .ok_or(Error::Invalid("missing observation state"))?
                        .session
                };
                diesel::insert_into(samples::table)
                    .values(SampleRow::new(sample, id, session))
                    .execute(connection)?;
                if learn {
                    let state = previous
                        .as_ref()
                        .ok_or(Error::Invalid("missing interval state"))?;
                    let id = next_interval_id(connection)?;
                    diesel::insert_into(intervals::table)
                        .values(IntervalRow::new(&state.anchor, sample, id, session)?)
                        .execute(connection)?;
                }
                Ok(session)
            });
        let session = match written {
            Ok(session) => session,
            Err(error) => {
                self.gap(Some(&sample.serial));
                return Err(error);
            }
        };
        let anchor = if restart || learn {
            sample.clone()
        } else {
            previous
                .ok_or(Error::Invalid("missing observation anchor"))?
                .anchor
        };
        self.tracking.insert(
            sample.serial.clone(),
            Tracking {
                last: sample.clone(),
                anchor,
                session,
                stored_at: sample.timestamp,
                active: true,
            },
        );
        Ok(true)
    }

    pub fn predict(&mut self, current: &Sample) -> Result<Option<Prediction>, Error> {
        if current.timestamp <= 0 {
            return Err(Error::Invalid("prediction timestamp must be positive"));
        }
        let cutoff = current
            .timestamp
            .saturating_sub(i64::from(self.retention_days) * 86400);
        let rows: Vec<RateRow> = diesel::sql_query(
            "SELECT seconds,first,last,poll_rate FROM intervals WHERE serial=? AND charging=? AND ended BETWEEN ? AND ? AND vid=? AND pid=? ORDER BY (poll_rate=?) DESC,ended DESC LIMIT 1024"
        ).bind::<Text,_>(current.serial.as_str()).bind::<Bool,_>(current.charging)
            .bind::<BigInt,_>(cutoff).bind::<BigInt,_>(current.timestamp)
            .bind::<Integer,_>(i32::from(current.usb.vendor)).bind::<Integer,_>(i32::from(current.usb.product))
            .bind::<Integer,_>(i32::from(current.poll_rate)).load(&mut self.connection)?;
        let source = rows
            .into_iter()
            .map(|row| {
                Ok(RateInterval {
                    seconds: row.seconds,
                    first: row.first,
                    last: row.last,
                    poll_rate: u16::try_from(row.poll_rate)
                        .map_err(|_| Error::Invalid("stored polling rate out of range"))?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(predict::predict(current, &source))
    }

    /// Returns real observations in chronological time buckets, keeping session
    /// identifiers so consumers never connect chart lines across a gap.
    pub fn history(
        &mut self,
        serial: &Serial,
        since: i64,
        capacity: usize,
    ) -> Result<Vec<Point>, Error> {
        if since < 0 || !(1..=4096).contains(&capacity) {
            return Err(Error::Invalid(
                "history requires nonnegative time and capacity 1–4096",
            ));
        }
        let extent: (Option<i64>, Option<i64>) = samples::table
            .filter(samples::serial.eq(serial.as_str()))
            .filter(samples::timestamp.ge(since))
            .select((min(samples::timestamp), max(samples::timestamp)))
            .first(&mut self.connection)?;
        let (Some(first), Some(last)) = extent else {
            return Ok(Vec::new());
        };
        let capacity =
            i64::try_from(capacity).map_err(|_| Error::Invalid("history capacity out of range"))?;
        let bucket = (last - first) / capacity + 1;
        let rows: Vec<PointRow> = diesel::sql_query(
            "SELECT timestamp,session,battery,charging,poll_rate FROM samples WHERE id IN (SELECT max(id) FROM samples WHERE serial=? AND timestamp>=? GROUP BY (timestamp-?)/?) ORDER BY timestamp,id LIMIT ?"
        ).bind::<Text,_>(serial.as_str()).bind::<BigInt,_>(since).bind::<BigInt,_>(first).bind::<BigInt,_>(bucket).bind::<BigInt,_>(capacity)
            .load(&mut self.connection)?;
        rows.into_iter()
            .map(|row| {
                Ok(Point {
                    timestamp: row.timestamp,
                    session: row.session,
                    battery: row.battery,
                    charging: row.charging,
                    poll_rate: u16::try_from(row.poll_rate)
                        .map_err(|_| Error::Invalid("stored polling rate out of range"))?,
                })
            })
            .collect()
    }

    pub fn usage(&mut self) -> Result<Usage, Error> {
        let count: i64 = samples::table.count().get_result(&mut self.connection)?;
        let samples =
            u64::try_from(count).map_err(|_| Error::Invalid("negative history row count"))?;
        let mut bytes = 0_u64;
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.as_os_str().to_os_string();
            path.push(suffix);
            match fs::metadata(&path) {
                Ok(metadata) => bytes = bytes.saturating_add(metadata.len()),
                Err(error)
                    if !suffix.is_empty() && error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(Error::io(PathBuf::from(path), source)),
            }
        }
        Ok(Usage { samples, bytes })
    }

    pub fn clear(&mut self, serial: Option<&Serial>) -> Result<(), Error> {
        self.require_writer()?;
        self.connection
            .immediate_transaction::<_, Error, _>(|connection| {
                if let Some(serial) = serial {
                    diesel::delete(samples::table.filter(samples::serial.eq(serial.as_str())))
                        .execute(connection)?;
                    diesel::delete(intervals::table.filter(intervals::serial.eq(serial.as_str())))
                        .execute(connection)?;
                } else {
                    diesel::delete(samples::table).execute(connection)?;
                    diesel::delete(intervals::table).execute(connection)?;
                }
                Ok(())
            })?;
        self.gap(serial);
        Ok(())
    }

    pub fn prune(&mut self, now: i64) -> Result<(), Error> {
        self.require_writer()?;
        if now <= 0 {
            return Err(Error::Invalid("prune timestamp must be positive"));
        }
        let cutoff = now.saturating_sub(i64::from(self.retention_days) * 86400);
        self.connection
            .immediate_transaction::<_, Error, _>(|connection| {
                diesel::delete(samples::table.filter(samples::timestamp.lt(cutoff)))
                    .execute(connection)?;
                diesel::delete(intervals::table.filter(intervals::ended.lt(cutoff)))
                    .execute(connection)?;
                Ok(())
            })?;
        for state in self.tracking.values_mut() {
            if state.anchor.timestamp < cutoff {
                state.active = false;
            }
        }
        Ok(())
    }

    fn require_writer(&self) -> Result<(), Error> {
        if self.writable {
            Ok(())
        } else {
            Err(Error::ReadOnly)
        }
    }
}

fn duplicate_observation(previous: &Sample, current: &Sample) -> bool {
    // Device writes trigger fresh readbacks. The wall clock has second precision,
    // so repeated unchanged reads within that second must not restart learning.
    previous.serial == current.serial
        && previous.timestamp == current.timestamp
        && previous.battery == current.battery
        && previous.charging == current.charging
        && previous.poll_rate == current.poll_rate
        && previous.usb == current.usb
        && current
            .monotonic
            .checked_sub(previous.monotonic)
            .is_some_and(|elapsed| elapsed < Duration::from_secs(1))
}

fn continuous(previous: &Sample, current: &Sample) -> bool {
    let Some(wall) = current.timestamp.checked_sub(previous.timestamp) else {
        return false;
    };
    let Ok(wall) = u32::try_from(wall) else {
        return false;
    };
    let Some(monotonic) = current.monotonic.checked_sub(previous.monotonic) else {
        return false;
    };
    let before = previous.battery.get();
    let after = current.battery.get();
    wall > 0
        && wall <= 300
        && !monotonic.is_zero()
        && (f64::from(wall) - monotonic.as_secs_f64()).abs() <= 5.0
        && previous.charging == current.charging
        && previous.poll_rate == current.poll_rate
        && previous.usb == current.usb
        && (after - before).abs() <= 10.0
        && if current.charging {
            after >= before - 0.5
        } else {
            after <= before + 0.5
        }
}

fn enough_change(anchor: &Sample, sample: &Sample) -> bool {
    let Some(seconds) = sample.monotonic.checked_sub(anchor.monotonic) else {
        return false;
    };
    let delta = if sample.charging {
        sample.battery.get() - anchor.battery.get()
    } else {
        anchor.battery.get() - sample.battery.get()
    };
    let minimum_change = if sample.charging && sample.battery.get() >= 100.0 {
        0.25
    } else {
        2.0
    };
    delta >= minimum_change
        && seconds >= Duration::from_secs(if sample.charging { 300 } else { 600 })
}

fn next_sample_id(connection: &mut SqliteConnection) -> Result<i64, Error> {
    let id: Option<i64> = samples::table.select(max(samples::id)).first(connection)?;
    id.unwrap_or(0)
        .checked_add(1)
        .ok_or(Error::Invalid("sample ID exhausted"))
}

fn next_interval_id(connection: &mut SqliteConnection) -> Result<i64, Error> {
    let id: Option<i64> = intervals::table
        .select(max(intervals::id))
        .first(connection)?;
    id.unwrap_or(0)
        .checked_add(1)
        .ok_or(Error::Invalid("interval ID exhausted"))
}

fn database_uri(path: &Path, writable: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut uri = String::from("file:");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push('%');
            uri.push(char::from(HEX[usize::from(byte >> 4)]));
            uri.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    uri.push_str(if writable { "?mode=rw" } else { "?mode=ro" });
    uri
}

#[derive(Insertable)]
#[diesel(table_name = samples)]
struct SampleRow<'a> {
    id: i64,
    serial: &'a str,
    timestamp: i64,
    session: i64,
    battery: f64,
    charging: bool,
    poll_rate: i32,
    vid: i32,
    pid: i32,
}

impl<'a> SampleRow<'a> {
    fn new(sample: &'a Sample, id: i64, session: i64) -> Self {
        Self {
            id,
            serial: sample.serial.as_str(),
            timestamp: sample.timestamp,
            session,
            battery: sample.battery.get(),
            charging: sample.charging,
            poll_rate: i32::from(sample.poll_rate),
            vid: i32::from(sample.usb.vendor),
            pid: i32::from(sample.usb.product),
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = intervals)]
struct IntervalRow<'a> {
    id: i64,
    serial: &'a str,
    started: i64,
    ended: i64,
    seconds: f64,
    first: f64,
    last: f64,
    charging: bool,
    poll_rate: i32,
    vid: i32,
    pid: i32,
    session: i64,
}

impl<'a> IntervalRow<'a> {
    fn new(anchor: &Sample, sample: &'a Sample, id: i64, session: i64) -> Result<Self, Error> {
        Ok(Self {
            id,
            serial: sample.serial.as_str(),
            started: anchor.timestamp,
            ended: sample.timestamp,
            seconds: sample
                .monotonic
                .checked_sub(anchor.monotonic)
                .ok_or(Error::Invalid("negative interval"))?
                .as_secs_f64(),
            first: anchor.battery.get(),
            last: sample.battery.get(),
            charging: sample.charging,
            poll_rate: i32::from(sample.poll_rate),
            vid: i32::from(sample.usb.vendor),
            pid: i32::from(sample.usb.product),
            session,
        })
    }
}

#[derive(QueryableByName)]
struct RateRow {
    #[diesel(sql_type = Double)]
    seconds: f64,
    #[diesel(sql_type = Double)]
    first: f64,
    #[diesel(sql_type = Double)]
    last: f64,
    #[diesel(sql_type = Integer)]
    poll_rate: i32,
}

#[derive(QueryableByName)]
struct PointRow {
    #[diesel(sql_type = BigInt)]
    timestamp: i64,
    #[diesel(sql_type = BigInt)]
    session: i64,
    #[diesel(sql_type = Double)]
    battery: f64,
    #[diesel(sql_type = Bool)]
    charging: bool,
    #[diesel(sql_type = Integer)]
    poll_rate: i32,
}
