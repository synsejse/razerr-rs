//! Native D-Bus dictionaries and validated application interfaces.

mod client;
mod codec;
mod control;
mod devices;

pub use client::Client;
pub use codec::{State, decode, encode};
pub use control::{CONTROL_PATH, ControlProxy};
pub use devices::{DEVICES_PATH, Devices, DevicesProxy, emit_error, publish};

pub const SERVICE_NAME: &str = "org.razerr_rs.Service";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Bus(#[from] zbus::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    #[error("invalid service state: {0}")]
    Invalid(String),
}
