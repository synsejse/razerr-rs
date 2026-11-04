//! Desktop integration for the background service; no GUI toolkit dependency.

pub mod launch;
pub mod notifications;
pub mod tray;

pub use launch::launch_gui;
pub use notifications::{Notification, Notifications};
pub use tray::Tray;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    Open,
    Settings,
    Refresh,
    Stop,
    Quit,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("desktop operation failed: {0}")]
    Bus(#[from] zbus::Error),
    #[error("tray could not start: {0}")]
    Tray(#[from] ksni::Error),
    #[error("tray service has stopped")]
    TrayStopped,
    #[error("{0} timed out")]
    Timeout(&'static str),
    #[error("cannot launch {path}: {source}")]
    Launch {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sibling executable name must be a single filename")]
    InvalidProgram,
}
