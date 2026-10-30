# Bundled assets

## Icon

`icons/razerr-rs.svg` is the application icon, drawn for razerr-rs. The
embedded `icons/razerr-rs.png` is rendered from it at 512 × 512:

```sh
nix-shell -p librsvg --run 'rsvg-convert -w 512 -h 512 assets/icons/razerr-rs.svg -o assets/icons/razerr-rs.png'
```

## Fonts

- `fonts/inter/Inter-Regular.ttf` was copied from Chromaven (Inter 4.001).
- `fonts/inter/Inter-SemiBold.ttf` is the SemiBold face of nixpkgs `inter`
  4.1 (`Inter.ttc`), subset to Latin, general punctuation and arrows.
- `fonts/lucide/Lucide.ttf` is a subset of `font/lucide.ttf` from the npm
  package `lucide-static` 1.64.0 (integrity
  `sha512-UxWmMteOXbKHI5E4f2qVtxCAAsI9gsInYK1lNw0nrOA0w4FkJeOVZ+UOV6aGA2377FZe3qyiFYq3lqjBKZ6OuA==`).
  It keeps only the glyphs named by `Icon` in `crates/ui/src/style.rs`; the
  codepoints are Lucide's own, from the package's `font/codepoints.json`.

To add an icon, look up its codepoint in `codepoints.json`, add it to `Icon`,
and subset again with every codepoint `Icon::glyph` uses:

```sh
nix-shell -p 'python3.withPackages(p: [p.fonttools])' --run \
  'pyftsubset lucide.ttf --unicodes="U+e145,U+e154,…" \
     --output-file=assets/fonts/lucide/Lucide.ttf --name-IDs="*" --layout-features="*"'
```

Font licenses are bundled beside each font: Inter uses the SIL Open Font
License, and the Lucide subset retains its ISC license. Include these notices
when distributing the assets. No generated shaders or C icon renderer are used.
