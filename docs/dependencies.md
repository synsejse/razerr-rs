# Dependency maintenance

Direct dependencies are pinned in the workspace manifest; `Cargo.lock` is
checked in. Run `cargo deny --locked check` inside `nix-shell` to inspect
sources, licenses, advisories and duplicate versions.

The `openrazer` D-Bus bindings come from a pinned commit of
[synsejse/openrazer-rs](https://github.com/synsejse/openrazer-rs), the one git
source `deny.toml` allows. Its manifest entry keeps a `version` as well as the
`rev`, because `cargo deny` treats an unversioned git dependency as a wildcard.
Updating the pin also changes the Nix hash described in
[development](development.md#nix).

Five maintenance notices have narrow, documented exceptions in `deny.toml`:

- iced 0.14's graphics dependencies still use the discontinued `paste` macro
  crate. This is an unmaintained-package advisory with no patched release.
  [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html).
- iced's text dependencies still require `ttf-parser`, also reported as
  unmaintained with no patched release.
  [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html).
- ksni 0.3.7 uses zbus 5 and the formerly standalone `zvariant`, `zbus_names`
  and `zvariant_derive` crates. Those crates were merged into zbus 6; this app's
  own D-Bus code already uses zbus 6. The tray dependency must migrate upstream.
  [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html),
  [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html),
  [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html).

These exceptions cover those maintenance notices only; other advisories still
fail the dependency check. Revisit them when updating iced or ksni. Duplicate versions are reported
as warnings, including the temporary zbus 5/6 split.

iced image widgets use `image-without-codecs`. Decoding is confined to
`razerr-images`, which enables PNG/JPEG only and enforces download/allocation
limits. The interface is always dark and does not enable iced's platform theme
detection. SQLite links to the system library supplied by
`shell.nix`; no copy of SQLite is bundled.

The Cargo license allowlist covers the selected dependency graph. Fonts come from
`iced_t3`, whose Inter and Lucide license notices are kept beside its assets; they
are not Cargo packages. Include these notices when distributing the assets.

`iced_t3` and `openrazer` are Cargo Git dependencies pinned by full revision and
exact package version in the workspace manifest. Their repositories are explicitly
allowed in `deny.toml`. Their tests and examples run in their own checkouts; they
are not workspace members. Update the pin, lockfile and Nix source hash together.
Bundled font license copies for distribution are in `packaging/licenses/`.

`iced_aw` 0.14.1 shares iced 0.14 with the app. Default features are disabled:
number inputs are enabled explicitly. Its release version is
pinned; upstream development branches may target a newer iced.
