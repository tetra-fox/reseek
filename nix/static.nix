# linked statically against musl, so the binaries run on any Linux of their architecture without a
# Nix store. zig compiles and links the dependencies' C for the target
{
  lib,
  craneLib,
  cargo-zigbuild,
  zig,
  perl,
  writableTmpDirAsHomeHook,
  reseek,
  web,
  revision ? null,
  # the Rust target, such as `aarch64-unknown-linux-musl`
  target,
}:
let
  targetCraneLib = craneLib.overrideToolchain (p: p.reseek-rust.override { targets = [ target ]; });
  # what the target's dependencies share with its build, as in package.nix
  args = {
    pname = "reseek-${target}";
    inherit (reseek) version src;
    strictDeps = true;
    nativeBuildInputs = [
      cargo-zigbuild
      zig
      perl
      # zig keeps its cache under the home directory
      writableTmpDirAsHomeHook
    ];
    doCheck = false;
    buildPhaseCargoCommand = "cargo zigbuild --locked --profile dist --package reseek-bin --target ${target}";
  };
in
targetCraneLib.mkCargoDerivation (
  args
  // {
    cargoArtifacts = targetCraneLib.buildDepsOnly args;
    doInstallCargoArtifacts = false;

    env = {
      WEBUI_DIR = web;
    }
    // lib.optionalAttrs (revision != null) { RESEEK_GIT_REVISION = revision; };

    installPhaseCommand = ''
      install -Dm755 target/${target}/dist/reseek $out/bin/reseek
      install -Dm755 target/${target}/dist/reseek-agent $out/bin/reseek-agent
    '';

    meta = reseek.meta // {
      description = "The reseek release binaries for ${target}, linked statically";
      platforms = lib.platforms.linux;
    };
  }
)
