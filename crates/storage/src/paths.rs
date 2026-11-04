//! Resolve paths without creating files. Creation is always explicit.

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::Error;

const APP_NAME: &str = "razerr-rs";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    config: PathBuf,
    state: PathBuf,
    cache: PathBuf,
}

impl Paths {
    /// Read the XDG environment once; pass the result to components explicitly.
    pub fn discover() -> Result<Self, Error> {
        let dirs = ProjectDirs::from("org", "razerr_rs", APP_NAME)
            .ok_or(Error::UserDirectoriesUnavailable)?;
        Ok(Self {
            config: dirs.config_dir().to_path_buf(),
            state: dirs
                .state_dir()
                .ok_or(Error::UserDirectoriesUnavailable)?
                .to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
        })
    }

    /// Use explicit XDG roots, adding the `razerr-rs` directory to each.
    /// This does not inspect or change the process environment.
    pub fn from_roots(
        config: impl AsRef<Path>,
        state: impl AsRef<Path>,
        cache: impl AsRef<Path>,
    ) -> Result<Self, Error> {
        for root in [config.as_ref(), state.as_ref(), cache.as_ref()] {
            if !root.is_absolute() {
                return Err(Error::RelativeRoot(root.to_path_buf()));
            }
        }
        Ok(Self {
            config: config.as_ref().join(APP_NAME),
            state: state.as_ref().join(APP_NAME),
            cache: cache.as_ref().join(APP_NAME),
        })
    }

    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    pub fn history_file(&self) -> PathBuf {
        self.state.join("history.sqlite3")
    }

    pub fn images_dir(&self) -> PathBuf {
        self.cache.join("images")
    }

    pub fn create_config_dir(&self) -> Result<(), Error> {
        create_private_dir(&self.config)
    }

    pub fn create_state_dir(&self) -> Result<(), Error> {
        create_private_dir(&self.state)
    }
}

fn create_private_dir(path: &Path) -> Result<(), Error> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|source| Error::io(path, source))?;
    if fs::symlink_metadata(path)
        .map_err(|source| Error::io(path, source))?
        .file_type()
        .is_symlink()
    {
        return Err(Error::io(
            path,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "application directory is a symbolic link; refusing to change its target permissions",
            ),
        ));
    }
    // Only restrict our app's own directory, never the user's XDG root.
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|source| Error::io(path, source))
}
