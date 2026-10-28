# The reseek daemon with the web UI built in, its shell completions and its man page. The UI
# comes from the `web` derivation through WEBUI_DIR, so this build needs no network and no pnpm.
# The test suite runs in the check phase; the suites that need soulfind skip without it.
{
  lib,
  rustPlatform,
  pkg-config,
  cmake,
  perl,
  installShellFiles,
  web,
  version,
  # the commit the build is of, shown beside the version; none outside a git checkout
  revision ? null,
  # the tests of the package beyond its own suite, such as the NixOS VM test
  tests ? { },
  nix-update-script,
}:
let
  fs = lib.fileset;
in
rustPlatform.buildRustPackage {
  pname = "reseek";
  inherit version;

  src = fs.toSource {
    root = ../.;
    fileset = fs.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../rustfmt.toml
      ../crates
      ../migrations
      ../api/openapi.json
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = [
    "--package"
    "reseek"
  ];

  env = {
    WEBUI_DIR = web;
  }
  // lib.optionalAttrs (revision != null) { RESEEK_GIT_REVISION = revision; };

  nativeBuildInputs = [
    pkg-config
    cmake
    perl
    installShellFiles
  ];

  useNextest = true;
  cargoTestFlags = [ "--workspace" ];

  postInstall = ''
    installShellCompletion --cmd reseek \
      --bash <($out/bin/reseek completions bash) \
      --zsh <($out/bin/reseek completions zsh) \
      --fish <($out/bin/reseek completions fish)
    $out/bin/reseek man > reseek.1
    installManPage reseek.1
  '';

  passthru = {
    inherit web tests;
    updateScript = nix-update-script { };
  };

  meta = {
    description = "A Soulseek client daemon with a web UI";
    license = lib.licenses.agpl3Only;
    mainProgram = "reseek";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
