# Settings

`razerr-storage` reads and writes
`$XDG_CONFIG_HOME/razerr-rs/settings.toml` (normally
`~/.config/razerr-rs/settings.toml`). The service loads it at startup and watches
for edits; the GUI's Preferences page saves it. The read-only probe does not
read or create this file.

These are the defaults:

```toml
[appearance]
dark = false

[notifications]
connections = true
battery = true

[history]
enabled = true
days = 90
sample_seconds = 60
```

The interface is always dark now, so `appearance.dark` has no effect. It stays
valid in the file, and saving keeps it.

Missing files and keys use defaults. History retention accepts 7–365 days;
the sampling interval accepts 20–300 seconds. Boolean settings require TOML
booleans. Parsing and range validation apply to the complete document before
the in-memory settings change.

`SettingsStore::reload` reports invalid files and retains the last valid
settings. Removing the file restores defaults on the next reload. The service
watches the containing directory, debounces changes, and reloads only after
changes to this file. Watching the directory also supports editors that save
by renaming a temporary file. Read/access events are ignored to avoid a
reload feedback loop.

Saving validates the requested settings and rereads the existing document to
preserve comments and unknown keys. An existing invalid document is left
untouched. Saves write a private temporary file in the config directory,
synchronize it, rename it over the settings file, and synchronize the directory.
The settings file is mode `0600`; app directories are mode `0700`. XDG root
directory permissions are not changed.

Reading through a symlinked app directory is supported. Creating or saving
inside one returns an error, so the library cannot accidentally change the
permissions of the link's target.

There is no importer for Chromaven's `settings.ini`.
