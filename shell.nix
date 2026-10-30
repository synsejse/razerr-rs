{ pkgs ? import <nixpkgs> { } }:

let
  # winit/wgpu load these libraries at runtime.
  runtimeLibs = with pkgs; [
    wayland
    libxkbcommon
    vulkan-loader
    libGL
    libx11
    libxcursor
    libxi
    libxrandr
  ];
in
pkgs.mkShell {
  name = "razerr-rs";

  nativeBuildInputs = with pkgs; [
    cargo
    rustc
    rustfmt
    clippy
    rust-analyzer
    cargo-deny
    diesel-cli
    desktop-file-utils
    pkg-config
  ];

  buildInputs = [ pkgs.sqlite ] ++ runtimeLibs;
  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

  shellHook = ''
    export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  '';
}
