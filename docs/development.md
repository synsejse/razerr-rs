# Development

Use `shell.nix` for the Rust toolchain, SQLite/Diesel tooling and GUI runtime
libraries. OpenRazer must run on the same desktop session bus.

## Build and run

`crates/iced_t3` is a git submodule. Clone with `--recurse-submodules`, or run
`git submodule update --init` in an existing clone.

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
cargo run --bin razerr-rs -- --demo
cargo run --bin razerr-rs -- quit
```

The probe only reads device state. `--demo` runs on the made-up devices in
`crates/ui/fixtures`, with no service, D-Bus or files, and simulates writes. Closing the window or running
`razerr-rs quit` leaves the service running. Use `RAZERR_RS_LOG=debug` for logs.

For headless monitoring, run `cargo run --bin razerr-rs-service` in one terminal.
In another shell, use `cargo run --bin razerr-rs-service -- status`, `refresh`,
`clear-history`, `stop` or `quit`. The service command `stop` leaves an open GUI
running; its explicit `quit` command closes the GUI too, as stopping the service
any other way does, for example `systemctl --user restart razerr-rs`.

## Local checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check
```

These include `iced_t3`, which is a workspace member. To change it, commit and
push in its own repository, then bump the submodule here.

For a focused change, select a crate with `-p`, for example
`cargo test -p razerr-metrics`. See [migrations](migrations.md) for Diesel schema
updates, [dependency maintenance](dependencies.md) for advisory exceptions,
and [verification](verification.md) for actual hardware coverage.

## UI captures

The render tests in `crates/ui` build every page and state without a window. Set
`RAZERR_UI_CAPTURE_DIR` to save them as PNGs, and `RAZERR_UI_SNAPSHOT` to a saved
`razerr-rs-service status` to add a real device's readings:

```sh
RAZERR_UI_CAPTURE_DIR=/tmp/captures cargo test -p razerr-ui every_page
```

These use the software renderer, which has no MSAA and handles translucent layers
differently from the GPU. Check anything visual on the real card too.

## Write path test

`crates/worker/tests/fake_daemon.rs` runs the real worker against a fake
`org.razer` that applies writes late, never, or fails. It stops unless it is on a
private bus:

```sh
dbus-run-session -- cargo test -p razerr-worker --test fake_daemon -- --ignored
```

## Nix

GitHub tarballs leave submodules out, so a `github:` flake input arrives with
`crates/iced_t3` empty and fails to build. Use a git input that asks for them:

```nix
razerr-rs = {
  url = "git+https://github.com/synsejse/razerr-rs?ref=main&submodules=1";
  flake = false;
};
```

## Release builds

```sh
cargo build --release --locked
```

Run `target/release/razerr-rs` from `nix-shell` so runtime libraries are
available. `packaging/` retains desktop, D-Bus and systemd templates for future
packaging; their `@PREFIX@` placeholders need substitution. There is currently
no installer, Nix package or automatic service registration.
