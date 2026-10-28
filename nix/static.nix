# The release binary for one Linux architecture: the daemon with the web UI built in, linked
# statically against musl, so it runs on any Linux of that architecture without a Nix store. zig
# compiles and links the C the dependencies carry, for whichever architecture is asked for.
{
  lib,
  rust-bin,
  makeRustPlatform,
  cargo-zigbuild,
  zig,
  perl,
  reseek,
  web,
  revision ? null,
  # the Rust target, such as `aarch64-unknown-linux-musl`
  target,
}:
let
  toolchain = rust-bin.stable.latest.minimal.override { targets = [ target ]; };
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "reseek-${target}";
  inherit (reseek) version src cargoDeps;

  env = {
    WEBUI_DIR = web;
  }
  // lib.optionalAttrs (revision != null) { RESEEK_GIT_REVISION = revision; };

  nativeBuildInputs = [
    cargo-zigbuild
    zig
    perl
  ];

  buildPhase = ''
    runHook preBuild
    # zig keeps its cache under the home directory
    export HOME=$TMPDIR
    cargo zigbuild --offline --release --package reseek --target ${target}
    runHook postBuild
  '';

  doCheck = false;

  installPhase = ''
    runHook preInstall
    install -Dm755 target/${target}/release/reseek $out/bin/reseek
    runHook postInstall
  '';

  meta = reseek.meta // {
    description = "The reseek release binary for ${target}, linked statically";
    platforms = lib.platforms.linux;
  };
}
