# soulseek-rs, an independent Soulseek client whose CLI is the other user in `just e2e`. It is a
# test fixture only; nothing in reseek links to it.
{
  lib,
  rustPlatform,
  fetchFromGitHub,
}:
rustPlatform.buildRustPackage {
  pname = "soulseek-rs";
  version = "0-unstable-2026-10-22";

  src = fetchFromGitHub {
    owner = "michel";
    repo = "soulseek-rs";
    rev = "68bc3067c315da856dae894c4c610e0360c538b5";
    hash = "sha256-TwH62BhmOVixJ2HeRzLCZv4GK2pMhHq3OSav98o+VIU=";
  };

  cargoHash = "sha256-k780jwKdWYaK+5TekK+WniLNQL0jDEtRRFIzMUuoXf8=";

  cargoBuildFlags = [
    "--package"
    "soulseek-rs"
  ];

  # its own suite talks to a server; the e2e recipe is what exercises it here
  doCheck = false;

  meta = {
    description = "Soulseek client whose CLI is the interop peer of the end-to-end tests";
    homepage = "https://github.com/michel/soulseek-rs";
    license = lib.licenses.mit;
    mainProgram = "soulseek-rs";
  };
}
