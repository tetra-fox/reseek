# linked statically against musl, so the binaries run on any Linux of their architecture without a
# Nix store. zig compiles and links the dependencies' C for the target
{
  lib,
  reseek-rust,
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
  toolchain = reseek-rust.override { targets = [ target ]; };
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "reseek-${target}";
  inherit (reseek)
    version
    src
    cargoDeps
    dontCargoUseSystemLibs
    ;

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
    cargo zigbuild --offline --profile dist --package reseek-bin --target ${target}
    runHook postBuild
  '';

  doCheck = false;

  installPhase = ''
    runHook preInstall
    install -Dm755 target/${target}/dist/reseek $out/bin/reseek
    install -Dm755 target/${target}/dist/reseek-agent $out/bin/reseek-agent
    runHook postInstall
  '';

  meta = reseek.meta // {
    description = "The reseek release binaries for ${target}, linked statically";
    platforms = lib.platforms.linux;
  };
}
