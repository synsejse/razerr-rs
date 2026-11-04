//! Local battery observations and empirical predictions, independent of D-Bus/UI.
//!
//! A `Store` is synchronous and belongs to one thread. The service owns its writer
//! on a dedicated thread; GUI readers run in blocking tasks and never migrate.

mod migrations;
pub mod predict;
mod schema;
mod store;

use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use razerr_model::{Percent, Serial, UsbId};
use serde::Serialize;

pub use predict::{Confidence, Prediction, RateInterval};
pub use store::Store;

/// A fresh observation. Callers must obtain battery from `Reading::fresh()` and
/// skip samples when charging status is unknown; cached display values do not train.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub serial: Serial,
    pub timestamp: i64,
    pub monotonic: Duration,
    pub battery: Percent,
    pub charging: bool,
    /// Zero means unknown. No inferred polling multiplier is ever applied.
    pub poll_rate: u16,
    pub usb: UsbId,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Point {
    pub timestamp: i64,
    pub session: i64,
    pub battery: f64,
    pub charging: bool,
    pub poll_rate: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub samples: u64,
    pub bytes: u64,
}

/// Seconds since the Unix epoch, as history timestamps count them.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_secs()).ok())
        .unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("history path must be absolute: {0}")]
    RelativePath(PathBuf),
    #[error("history path is a symbolic link: {0}")]
    Symlink(PathBuf),
    #[error("history I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot open history database: {0}")]
    Connection(#[from] diesel::ConnectionError),
    #[error("history database error: {0}")]
    Database(#[from] diesel::result::Error),
    #[error("database migration failed: {0}")]
    Migration(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("history database contains unknown migration {0}; a newer app may have created it")]
    TooNew(String),
    #[error("history database needs migration by the service")]
    NeedsMigration,
    #[error("operation requires a writable history database")]
    ReadOnly,
    #[error("invalid history input: {0}")]
    Invalid(&'static str),
}

impl Error {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
