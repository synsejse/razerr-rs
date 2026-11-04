# Bundled assets

## Icon

`icons/razerr-rs.svg` is the application icon, drawn for razerr-rs. The
embedded `icons/razerr-rs.png` is rendered from it at 512 × 512:

```sh
nix-shell -p librsvg --run 'rsvg-convert -w 512 -h 512 assets/icons/razerr-rs.svg -o assets/icons/razerr-rs.png'
```

## Fonts

Fonts and icon glyphs come from [iced_t3](../crates/iced_t3), with their licenses
in its `assets/fonts`.
