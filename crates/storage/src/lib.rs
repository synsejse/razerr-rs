//! Filesystem locations and durable, human-editable settings for razerr-rs.

pub mod paths;
pub mod settings;

pub use paths::Paths;
pub use settings::{Settings, SettingsStore};

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot determine user directories")]
    UserDirectoriesUnavailable,
    #[error("storage root must be an absolute path: {0}")]
    RelativeRoot(PathBuf),
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid settings TOML: {0}")]
    Parse(#[from] toml_edit::TomlError),
    #[error("invalid settings: {0}")]
    Deserialize(#[from] toml_edit::de::Error),
    #[error("cannot serialize settings: {0}")]
    Serialize(#[from] toml_edit::ser::Error),
    #[error("{field} must be between {minimum} and {maximum} (got {value})")]
    OutOfRange {
        field: &'static str,
        minimum: u16,
        maximum: u16,
        value: u16,
    },
}

impl Error {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
