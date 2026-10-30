# Development

Use `shell.nix` for the Rust toolchain, SQLite/Diesel tooling and GUI runtime
libraries. OpenRazer must run on the same desktop session bus.

## Build and run

```sh
nix-shell
cargo build --locked
cargo run --bin razerr-rs-service
```

Both applications are default workspace members. Leave the service running and
start `cargo run --bin razerr-rs` in another `nix-shell`. The GUI only connects
to an existing service and retries automatically when it is unavailable.
Packaging is responsible for service startup in installed builds.

Useful commands inside the shell:

```sh
cargo run --bin razerr-rs -- probe
cargo run --bin razerr-rs -- --settings
cargo run --bin razerr-rs -- --dark
cargo run --bin razerr-rs -- quit
```

The probe only reads device state. Closing the window or running
`razerr-rs quit` leaves the service running. Use `RAZERR_RS_LOG=debug` for logs.

For headless monitoring, run `cargo run --bin razerr-rs-service` in one terminal.
In another shell, use `cargo run --bin razerr-rs-service -- status`, `refresh`,
`clear-history`, `stop` or `quit`. The service command `stop` leaves an open GUI
running; its explicit `quit` command closes the GUI too.

## Local checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check
```

For a focused change, select a crate with `-p`, for example
`cargo test -p razerr-metrics`. See [migrations](migrations.md) for Diesel schema
updates, [dependency maintenance](dependencies.md) for advisory exceptions,
and [verification](verification.md) for actual hardware coverage.

## Release builds

```sh
cargo build --release --locked
```

Run `target/release/razerr-rs` from `nix-shell` so runtime libraries are
available. `packaging/` retains desktop, D-Bus and systemd templates for future
packaging; their `@PREFIX@` placeholders need substitution. There is currently
no installer, Nix package or automatic service registration.
