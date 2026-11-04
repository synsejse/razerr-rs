{
  lib,
  rustPlatform,
  razerrSource ? ../.,
  pkg-config,
  makeWrapper,
  desktop-file-utils,
  sqlite,
  wayland,
  libxkbcommon,
  vulkan-loader,
  libGL,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
}:

rustPlatform.buildRustPackage {
  pname = "razerr-rs";
  version = "0.1.0";
  src = lib.cleanSource razerrSource;
  cargoLock = {
    lockFile = "${razerrSource}/Cargo.lock";
    outputHashes = import "${razerrSource}/packaging/cargo-git-hashes.nix";
  };

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    desktop-file-utils
  ];
  buildInputs = [ sqlite ];

  postInstall = ''
    install -Dm644 packaging/org.razerr_rs.App.desktop $out/share/applications/org.razerr_rs.App.desktop
    install -Dm644 packaging/org.razerr_rs.App.service $out/share/dbus-1/services/org.razerr_rs.App.service
    install -Dm644 packaging/org.razerr_rs.Service.service $out/share/dbus-1/services/org.razerr_rs.Service.service
    install -Dm644 packaging/razerr-rs.service $out/lib/systemd/user/razerr-rs.service
    install -Dm644 assets/icons/razerr-rs.png $out/share/icons/hicolor/512x512/apps/org.razerr_rs.App.png
    substituteInPlace \
      $out/share/applications/org.razerr_rs.App.desktop \
      $out/share/dbus-1/services/*.service \
      $out/lib/systemd/user/razerr-rs.service \
      --replace-fail '@PREFIX@' "$out"
    desktop-file-validate $out/share/applications/org.razerr_rs.App.desktop
    install -Dm644 packaging/licenses/inter/LICENSE.txt $out/share/licenses/razerr-rs/inter/LICENSE.txt
    install -Dm644 packaging/licenses/lucide/LICENSE $out/share/licenses/razerr-rs/lucide/LICENSE
  '';

  postFixup = ''
    wrapProgram $out/bin/razerr-rs \
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath [
          wayland
          libxkbcommon
          vulkan-loader
          libGL
          libx11
          libxcursor
          libxi
          libxrandr
        ]
      }
  '';

  meta = {
    description = "OpenRazer device controls, system tray, and battery history";
    platforms = lib.platforms.linux;
    mainProgram = "razerr-rs";
  };
}
