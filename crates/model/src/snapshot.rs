use serde::{Deserialize, Serialize};

use crate::{Device, ValidationError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    Full,
    Live,
    #[default]
    Ack,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: u64,
    /// Changes whenever a potentially missing observation must break history.
    /// Survives watch-channel coalescing of intermediate snapshots.
    #[serde(default)]
    pub continuity: u64,
    /// Changes after a daemon outage or ownership change, establishing a quiet
    /// notification baseline even when the unavailable snapshot was coalesced.
    #[serde(default)]
    pub daemon_generation: u64,
    pub kind: SnapshotKind,
    pub devices: Vec<Device>,
    pub refreshing: bool,
    pub daemon_available: bool,
    /// False when enumeration or an identity read failed. Never interpret this
    /// as physical removal. Telemetry may break sessions to avoid training
    /// across unreliable observations.
    pub reliable: bool,
    pub error: Option<String>,
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut serials = std::collections::HashSet::new();
        if self
            .devices
            .iter()
            .any(|device| !serials.insert(&device.serial))
        {
            return Err(ValidationError("duplicate device serial in snapshot"));
        }
        Ok(())
    }
}
