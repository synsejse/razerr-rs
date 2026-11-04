//! Typed settings with atomic saves that retain comments and unknown keys.

use std::fs::{self, File};
use std::io::Write;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use toml_edit::{DocumentMut, Item};

use crate::{Error, Paths};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub notifications: Notifications,
    pub history: History,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub connections: bool,
    pub battery: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            connections: true,
            battery: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub enabled: bool,
    pub days: u16,
    pub sample_seconds: u16,
}

impl Default for History {
    fn default() -> Self {
        Self {
            enabled: true,
            days: 90,
            sample_seconds: 60,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), Error> {
        for (field, value, minimum, maximum) in [
            ("history.days", self.history.days, 7, 365),
            (
                "history.sample_seconds",
                self.history.sample_seconds,
                20,
                300,
            ),
        ] {
            if !(minimum..=maximum).contains(&value) {
                return Err(Error::OutOfRange {
                    field,
                    minimum,
                    maximum,
                    value,
                });
            }
        }
        Ok(())
    }

    /// Missing files and missing keys use defaults; reads never create files.
    pub fn load(paths: &Paths) -> Result<Self, Error> {
        read_document(paths).map(|(_, settings)| settings)
    }

    /// Replace the file atomically, preserving user comments and unknown keys.
    /// Existing malformed files are left intact for the user to repair.
    pub fn save(&self, paths: &Paths) -> Result<(), Error> {
        self.validate()?;
        // Read again at save time so changes from external editors are retained.
        let (mut document, _) = read_document(paths)?;
        let replacement = toml_edit::ser::to_document(self)?;
        merge(document.as_item_mut(), replacement.into_item());

        paths.create_config_dir()?;
        let path = paths.settings_file();
        let failed = |source| Error::io(&path, source);
        let mut temporary = NamedTempFile::new_in(paths.config_dir()).map_err(failed)?;
        temporary
            .write_all(document.to_string().as_bytes())
            .map_err(failed)?;
        temporary.as_file().sync_all().map_err(failed)?;
        temporary
            .persist(&path)
            .map_err(|error| failed(error.error))?;
        // Persist only renames; fsync the directory to make that rename durable.
        File::open(paths.config_dir())
            .and_then(|directory| directory.sync_all())
            .map_err(|source| Error::io(paths.config_dir(), source))
    }
}

/// Own the last valid settings. A failed reload never replaces them.
#[derive(Debug)]
pub struct SettingsStore {
    paths: Paths,
    current: Settings,
}

impl SettingsStore {
    /// Start with defaults; call `reload` to read the file and report errors.
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            current: Settings::default(),
        }
    }

    pub fn current(&self) -> &Settings {
        &self.current
    }

    /// Return whether a valid reload changed any known setting.
    pub fn reload(&mut self) -> Result<bool, Error> {
        let next = Settings::load(&self.paths)?;
        let changed = self.current != next;
        self.current = next;
        Ok(changed)
    }
}

fn read_document(paths: &Paths) -> Result<(DocumentMut, Settings), Error> {
    let path = paths.settings_file();
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(Error::io(path, error)),
    };
    let document: DocumentMut = contents.parse()?;
    let settings: Settings = toml_edit::de::from_document(document.clone())?;
    settings.validate()?;
    Ok((document, settings))
}

/// Update known leaves in place, including inline and dotted TOML tables.
fn merge(destination: &mut Item, mut source: Item) {
    if let (Some(destination), Some(source)) =
        (destination.as_table_like_mut(), source.as_table_like())
    {
        for (key, value) in source.iter() {
            if let Some(existing) = destination.get_mut(key) {
                merge(existing, value.clone());
            } else {
                destination.insert(key, value.clone());
            }
        }
    } else {
        if let (Some(previous), Some(next)) = (destination.as_value(), source.as_value_mut()) {
            *next.decor_mut() = previous.decor().clone();
        }
        *destination = source;
    }
}
