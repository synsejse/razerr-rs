# razerr-rs

OpenRazer device controls, a system tray, and battery history for Linux.

With OpenRazer running in your desktop session:

```sh
nix-shell
cargo build --locked
cargo run --bin razerr-rs-service
```

In another `nix-shell`, run `cargo run --bin razerr-rs`. The GUI only connects;
packaging is responsible for starting the service in installed builds.

[Architecture](docs/architecture.md) · [Development](docs/development.md) · [Settings](docs/settings.md)
