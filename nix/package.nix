# the UI comes from the `web` derivation through WEBUI_DIR, so this build needs no network and no
# pnpm
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
  # aws-lc and SQLite are built from the crates' own sources (SQLite with FTS5); nixpkgs' cargo
  # setup hook would otherwise make them link the system's, which the static builds cannot
  dontCargoUseSystemLibs = true;
  cargoBuildFlags = [
    "--package"
    "reseek-bin"
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

  # `just test` runs the test suite
  doCheck = false;

  postInstall = ''
    installShellCompletion --cmd reseek \
      --bash <($out/bin/reseek completions bash) \
      --zsh <($out/bin/reseek completions zsh) \
      --fish <($out/bin/reseek completions fish)
    $out/bin/reseek man > reseek.1
    installManPage reseek.1
  '';

  passthru = {
    inherit web;
    updateScript = nix-update-script { };
  };

  meta = {
    description = "A Soulseek client daemon with a web UI, and its relay agent";
    license = lib.licenses.agpl3Only;
    mainProgram = "reseek";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
