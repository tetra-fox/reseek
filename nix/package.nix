# the UI comes from the `web` derivation through WEBUI_DIR, so this build needs no network and no
# pnpm. the dependencies build in a derivation of their own that sees only the lockfile and the
# manifests, so a change to reseek's code reuses them from the store or the binary cache
{
  lib,
  craneLib,
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
  cargoArgs = "--locked --package reseek-bin";
  # what the dependencies' build shares with the package's: cargo compiles a dependency again when
  # the environment its build script reads differs. crane leaves out nixpkgs' cargo setup hook,
  # whose switches would make aws-lc and SQLite link the system's instead of their own
  args = {
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
    strictDeps = true;
    nativeBuildInputs = [
      pkg-config
      cmake
      perl
    ];
    # `just test` runs the test suite
    doCheck = false;
  };
in
craneLib.buildPackage (
  args
  // {
    cargoArtifacts = craneLib.buildDepsOnly (
      args // { buildPhaseCargoCommand = "cargoWithProfile build ${cargoArgs}"; }
    );
    cargoExtraArgs = cargoArgs;

    env = {
      WEBUI_DIR = web;
    }
    // lib.optionalAttrs (revision != null) { RESEEK_GIT_REVISION = revision; };

    nativeBuildInputs = args.nativeBuildInputs ++ [ installShellFiles ];

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
)
