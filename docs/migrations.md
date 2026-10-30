# Battery history migrations

`razerr-metrics` owns `$XDG_STATE_HOME/razerr-rs/history.sqlite3`. It uses a fresh
Diesel schema and never imports Chromaven databases or reads `user_version`.

SQL migrations live in `crates/metrics/migrations/NNNN_lowercase_name/` and are
embedded in the Rust library. Their numbers must be consecutive starting at
`0001`; a unit test checks the naming rule. `down.sql` is empty because the
application supports forward migration only. Never run `diesel migration redo`
or `revert` against user data: downgrade is deliberately unsupported.

The service's writer obtains `BEGIN IMMEDIATE`, checks the applied migration
versions, and applies every pending migration in that transaction. Per-migration
transactions become savepoints. Any failure rolls the entire batch back,
including Diesel's metadata updates. Concurrent writers recheck versions after
acquiring the lock, so initialization runs once.

An applied migration absent from this build is rejected as a newer/unknown
database. GUI readers use a percent-encoded SQLite `file:` URI with `mode=ro`
and set `query_only`. They inspect existing migration metadata using SELECTs,
never the migration harness, and reject pending or unknown versions. A missing
read-only database stays missing.

Connections enable foreign keys and a two-second busy timeout. Writers enable
WAL and normal synchronous mode; readers leave the database's journal mode
unchanged. The database is created privately with mode `0600`. App directory
creation belongs to `razerr-storage` and the service.

## Adding a migration

1. Add the next directory with `up.sql` and an empty `down.sql`.
2. Keep transaction management, PRAGMA connection setup and VACUUM outside the
   migration. Change schema and backfill data using normal SQLite SQL.
3. Add tests covering both fresh creation and upgrades. Until the first release,
   `0001_initial` may be edited; never edit a migration after it ships.
4. Regenerate the private Diesel schema against a scratch database, then run the metrics tests.

From the workspace root, inside `nix-shell`:

```sh
diesel migration run \
  --migration-dir crates/metrics/migrations \
  --database-url target/metrics-schema.sqlite3
diesel print-schema \
  --config-file crates/metrics/diesel.toml \
  --database-url target/metrics-schema.sqlite3 \
  > crates/metrics/src/schema.rs
cargo test -p razerr-metrics
```

When editing the initial migration before release, use a new scratch database;
an already migrated scratch file does not replay a changed `up.sql`.

`schema.rs` is generated and checked in. Database row types and Diesel's
connection never leave the metrics crate. Tests use real temporary SQLite files
to cover rollback, read-only safety, concurrent initialization, recording,
sessions, retention, usage and the literal numeric prediction scenarios from
Chromaven. The serial labels used for those scenarios are test identifiers,
not simulated hardware.
