{
  lib,
  rustPlatform,
  pkg-config,
  obs-studio,
  openvr,
}:

rustPlatform.buildRustPackage {
  pname = "obs-openvr";
  version = "0.2.0";

  src = lib.cleanSource ../.;
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    rustPlatform.bindgenHook
  ];
  buildInputs = [
    obs-studio
    openvr
  ];

  # The workspace has no tests, and building them would only rebuild the plugin.
  doCheck = false;

  # Only the plugin crate is installed; the rest are its internal bindings.
  cargoBuildFlags = [ "-p" "obs-openvr" ];

  installPhase = ''
    runHook preInstall
    lib=$(find target -name libobs_openvr.so -path '*/release/*' | head -n1)
    install -Dm755 "$lib" $out/lib/obs-plugins/obs-openvr.so
    runHook postInstall
  '';

  meta = {
    description = "OBS source that captures the SteamVR (OpenVR) headset mirror view on Linux";
    homepage = "https://github.com/alisonjenkins/obs-openvr";
    license = lib.licenses.gpl3Only;
    platforms = [ "x86_64-linux" ];
  };
}
