# The flake's checks besides the package itself, whose check phase runs the test suite: clippy,
# formatting, the web UI's lint, type check and tests, and the UI's types against the committed
# API document.
{
  lib,
  runCommand,
  rustPlatform,
  pkg-config,
  cmake,
  perl,
  clippy,
  rustfmt,
  nixfmt,
  reseek,
  reseek-web,
}:
let
  fs = lib.fileset;
  # the web build's sources and packages, running `commands` instead of the build
  web =
    name: commands:
    reseek-web.overrideAttrs {
      pname = "reseek-${name}";
      buildPhase = ''
        runHook preBuild
        ${commands}
        runHook postBuild
      '';
      installPhase = "touch $out";
    };
in
{
  clippy = rustPlatform.buildRustPackage {
    pname = "reseek-clippy";
    inherit (reseek) version src cargoDeps;
    nativeBuildInputs = [
      pkg-config
      cmake
      perl
      clippy
    ];
    buildPhase = ''
      runHook preBuild
      cargo clippy --offline --workspace --all-targets --all-features -- -D warnings
      runHook postBuild
    '';
    doCheck = false;
    installPhase = "touch $out";
    env.WEBUI_DIR = reseek-web;
  };

  fmt =
    runCommand "reseek-fmt"
      {
        src = fs.toSource {
          root = ../.;
          fileset = fs.unions [
            ../rustfmt.toml
            ../crates
            ../fuzz
            ../flake.nix
            ../nix
          ];
        };
        nativeBuildInputs = [
          rustfmt
          nixfmt
        ];
      }
      ''
        cd $src
        find crates fuzz -name '*.rs' -print0 | xargs -0 rustfmt --check --edition 2024 --config-path rustfmt.toml
        nixfmt --check flake.nix $(find nix -name '*.nix')
        touch $out
      '';

  web = web "web-checks" ''
    pnpm lint
    pnpm check
    pnpm test
  '';

  api-contract = web "api-contract" "pnpm api-check";
}
