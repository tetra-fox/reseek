# The smoke program the NixOS VM test runs beside the service: it drives the running daemon
# through its API against a soulfind in the VM, with the soulseek-rs CLI as the other user.
{
  lib,
  rustPlatform,
  pkg-config,
  cmake,
  perl,
  reseek,
}:
rustPlatform.buildRustPackage {
  pname = "reseek-smoke";
  inherit (reseek) version src cargoDeps;

  buildType = "debug";
  cargoBuildFlags = [
    "--package"
    "reseek-bin"
    "--example"
    "smoke"
  ];
  nativeBuildInputs = [
    pkg-config
    cmake
    perl
  ];
  doCheck = false;

  installPhase = ''
    runHook preInstall
    install -Dm755 "$(find target -path '*/debug/examples/smoke' -type f)" $out/bin/smoke
    runHook postInstall
  '';

  meta = {
    description = "The reseek daemon's smoke run, for the NixOS VM test";
    license = lib.licenses.agpl3Only;
    mainProgram = "smoke";
  };
}
