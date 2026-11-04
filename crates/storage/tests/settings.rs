use std::error::Error;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;

use razerr_storage::{Paths, Settings, SettingsStore};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

fn fixture() -> Result<(TempDir, Paths), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let paths = Paths::from_roots(
        directory.path().join("config"),
        directory.path().join("state"),
        directory.path().join("cache"),
    )?;
    Ok((directory, paths))
}

#[test]
fn missing_file_uses_defaults_without_writing() -> TestResult {
    let (_directory, paths) = fixture()?;
    let settings = Settings::load(&paths)?;
    assert_eq!(settings, Settings::default());
    assert!(settings.notifications.connections);
    assert!(settings.notifications.battery);
    assert!(settings.history.enabled);
    assert_eq!(settings.history.days, 90);
    assert_eq!(settings.history.sample_seconds, 60);
    assert!(!paths.config_dir().exists());
    Ok(())
}

#[test]
fn missing_fields_use_defaults() -> TestResult {
    let (_directory, paths) = fixture()?;
    paths.create_config_dir()?;
    fs::write(paths.settings_file(), "[notifications]\nbattery = false\n")?;
    let settings = Settings::load(&paths)?;
    assert!(!settings.notifications.battery);
    assert!(settings.notifications.connections);
    assert_eq!(settings.history.days, 90);
    Ok(())
}

#[test]
fn saves_preserve_comments_unknown_keys_and_external_edits() -> TestResult {
    let (_directory, paths) = fixture()?;
    paths.create_config_dir()?;
    fs::write(
        paths.settings_file(),
        "# Preferences\n[notifications] # Alerts\n# Leave this note\nbattery = true # Nighttime\ncustom = 'keep me'\n[future]\noption = 42\n",
    )?;
    let mut settings = Settings::load(&paths)?;
    settings.notifications.battery = false;
    settings.history.days = 7;
    settings.history.sample_seconds = 300;
    // The application must reread the document before saving, not use its old copy.
    let mut contents = fs::read_to_string(paths.settings_file())?;
    contents.push_str("added_later = 'external edit'\n");
    fs::write(paths.settings_file(), contents)?;
    settings.save(&paths)?;
    assert_eq!(Settings::load(&paths)?, settings);
    let saved = fs::read_to_string(paths.settings_file())?;
    for retained in [
        "# Preferences",
        "# Alerts",
        "# Leave this note",
        "battery = false # Nighttime",
        "custom = 'keep me'",
        "option = 42",
        "added_later = 'external edit'",
    ] {
        assert!(saved.contains(retained), "missing {retained:?} in {saved}");
    }
    Ok(())
}

#[test]
fn inline_and_dotted_tables_keep_unknown_fields() -> TestResult {
    let (_directory, paths) = fixture()?;
    paths.create_config_dir()?;
    for original in [
        "notifications = { battery = true, future = 1 } # Inline\n",
        "notifications.battery = true # Dotted\nnotifications.future = 1\n",
    ] {
        fs::write(paths.settings_file(), original)?;
        let mut settings = Settings::load(&paths)?;
        settings.notifications.battery = false;
        settings.save(&paths)?;
        assert_eq!(Settings::load(&paths)?, settings);
        let text = fs::read_to_string(paths.settings_file())?;
        assert!(text.contains("future = 1"));
        assert!(text.contains(if original.contains("Inline") {
            "# Inline"
        } else {
            "# Dotted"
        }));
    }
    Ok(())
}

#[test]
fn atomic_replacement_keeps_existing_reader_on_complete_old_file() -> TestResult {
    let (_directory, paths) = fixture()?;
    paths.create_config_dir()?;
    let original = "[notifications]\nbattery = false\n";
    fs::write(paths.settings_file(), original)?;
    fs::set_permissions(paths.settings_file(), fs::Permissions::from_mode(0o644))?;
    let mut old_reader = File::open(paths.settings_file())?;
    Settings::default().save(&paths)?;
    let mut observed = String::new();
    old_reader.read_to_string(&mut observed)?;
    assert_eq!(observed, original);
    assert_eq!(Settings::load(&paths)?, Settings::default());
    assert_eq!(
        fs::metadata(paths.settings_file())?.permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_dir(paths.config_dir())?.count(), 1);
    Ok(())
}

#[test]
fn malformed_or_invalid_existing_file_is_never_overwritten() -> TestResult {
    let (_directory, paths) = fixture()?;
    paths.create_config_dir()?;
    for original in [
        "[notifications\nbattery = true\n",
        "[notifications]\nconnections = 2\n",
        "[history]\ndays = 6\n",
        "[history]\ndays = 366\n",
        "[history]\ndays = -1\n",
        "[history]\ndays = 'many'\n",
        "[history]\nsample_seconds = 19\n",
        "[history]\nsample_seconds = 301\n",
        "[history]\nsample_seconds = 999999999999999999999\n",
        "[notifications]\nconnections = true\nbattery = 'broken'\n",
    ] {
        fs::write(paths.settings_file(), original)?;
        assert!(Settings::load(&paths).is_err(), "accepted {original}");
        assert!(Settings::default().save(&paths).is_err());
        assert_eq!(fs::read_to_string(paths.settings_file())?, original);
    }
    Ok(())
}

#[test]
fn invalid_new_settings_are_rejected_before_any_writes() -> TestResult {
    let (_directory, paths) = fixture()?;
    let mut invalid = Settings::default();
    invalid.history.days = 0;
    assert!(invalid.save(&paths).is_err());
    assert!(!paths.config_dir().exists());
    Ok(())
}

#[test]
fn validation_accepts_range_endpoints() -> TestResult {
    let (_directory, paths) = fixture()?;
    for (days, sample_seconds) in [(7, 20), (365, 300)] {
        let mut settings = Settings::default();
        settings.history.days = days;
        settings.history.sample_seconds = sample_seconds;
        settings.save(&paths)?;
        assert_eq!(Settings::load(&paths)?, settings);
    }
    Ok(())
}

#[test]
fn failed_reload_keeps_entire_last_valid_settings() -> TestResult {
    let (_directory, paths) = fixture()?;
    let mut store = SettingsStore::new(paths.clone());
    let mut valid = Settings::default();
    valid.notifications.battery = false;
    valid.history.days = 365;
    valid.save(&paths)?;
    assert!(store.reload()?);
    assert!(!store.reload()?);
    fs::write(
        paths.settings_file(),
        "[notifications]\nbattery = true\n[history]\ndays = 1\n",
    )?;
    assert!(store.reload().is_err());
    assert_eq!(store.current(), &valid);
    fs::remove_file(paths.settings_file())?;
    assert!(store.reload()?);
    assert_eq!(store.current(), &Settings::default());
    Ok(())
}

#[test]
fn a_retired_setting_is_ignored_and_left_in_the_file() -> TestResult {
    let (_directory, paths) = fixture()?;
    paths.create_config_dir()?;
    fs::write(paths.settings_file(), "[appearance]\ndark = true\n")?;
    assert_eq!(Settings::load(&paths)?, Settings::default());
    Settings::default().save(&paths)?;
    assert!(fs::read_to_string(paths.settings_file())?.contains("dark = true"));
    Ok(())
}

#[test]
fn settings_path_that_is_a_directory_is_an_io_error() -> TestResult {
    let (_directory, paths) = fixture()?;
    paths.create_config_dir()?;
    fs::create_dir(paths.settings_file())?;
    assert!(Settings::load(&paths).is_err());
    assert!(Settings::default().save(&paths).is_err());
    Ok(())
}

#[test]
fn paths_use_fresh_names_and_create_private_directories() -> TestResult {
    let (directory, paths) = fixture()?;
    let config_root = directory.path().join("config");
    fs::create_dir(&config_root)?;
    fs::set_permissions(&config_root, fs::Permissions::from_mode(0o755))?;
    assert_eq!(
        paths.settings_file(),
        config_root.join("razerr-rs/settings.toml")
    );
    assert_eq!(
        paths.history_file(),
        directory.path().join("state/razerr-rs/history.sqlite3")
    );
    assert_eq!(
        paths.images_dir(),
        directory.path().join("cache/razerr-rs/images")
    );
    paths.create_config_dir()?;
    paths.create_state_dir()?;
    for path in [paths.config_dir(), paths.state_dir()] {
        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o700);
    }
    assert_eq!(
        fs::metadata(config_root)?.permissions().mode() & 0o777,
        0o755
    );
    Ok(())
}

#[test]
fn explicit_roots_must_be_absolute() {
    for roots in [
        ("relative", "/tmp/state", "/tmp/cache"),
        ("/tmp/config", "", "/tmp/cache"),
        ("/tmp/config", "/tmp/state", "../cache"),
    ] {
        assert!(Paths::from_roots(roots.0, roots.1, roots.2).is_err());
    }
}

#[test]
fn saving_does_not_change_a_symlinked_directory_target() -> TestResult {
    let (directory, paths) = fixture()?;
    let shared = directory.path().join("shared");
    fs::create_dir(&shared)?;
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o755))?;
    fs::create_dir(directory.path().join("config"))?;
    std::os::unix::fs::symlink(&shared, paths.config_dir())?;

    // Reading a linked configuration is fine; writes must not chmod its target.
    assert_eq!(Settings::load(&paths)?, Settings::default());
    assert!(Settings::default().save(&paths).is_err());
    assert_eq!(fs::metadata(&shared)?.permissions().mode() & 0o777, 0o755);
    assert!(!shared.join("settings.toml").exists());
    Ok(())
}
