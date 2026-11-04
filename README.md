# razerr-rs

OpenRazer device controls, a system tray, and battery history for Linux.

The interface is built on [iced_t3](https://github.com/synsejse/iced_t3), a git
submodule, so clone with submodules:

```sh
git clone --recurse-submodules https://github.com/synsejse/razerr-rs.git
```

In an existing clone, run `git submodule update --init`. GitHub's tarballs leave
submodules out, so `github:` Nix inputs no longer work; see
[Nix](docs/development.md#nix).

With OpenRazer running in your desktop session:

```sh
nix-shell
cargo build --locked
cargo run --bin razerr-rs-service
```

In another `nix-shell`, run `cargo run --bin razerr-rs`. The GUI only connects;
packaging is responsible for starting the service in installed builds.

To look around without a device, service or OpenRazer, run
`cargo run --bin razerr-rs -- --demo`. It shows made-up devices and touches no
files.

[Architecture](docs/architecture.md) · [Development](docs/development.md) · [Settings](docs/settings.md)


Device controls are drafts: DPI stage activation, polling, DPI values, lighting,
and power settings reach the device only when you press **Apply** for that page.
**Revert** restores the reported values. Switching pages and background readings
preserve unapplied drafts. Preferences retain their separate explicit **Save**
action; selecting a stage with a keyboard shortcut also only changes its draft.
