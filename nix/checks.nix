# the flake's checks besides the package's build and the Home Manager module's; `just ci` runs the
# rest, the Rust, the UI and the API contract
{
  lib,
  runCommand,
  nixfmt,
}:
let
  fs = lib.fileset;
in
{
  fmt =
    runCommand "reseek-fmt"
      {
        src = fs.toSource {
          root = ../.;
          fileset = fs.unions [
            ../flake.nix
            ../nix
          ];
        };
        nativeBuildInputs = [ nixfmt ];
      }
      ''
        cd $src
        nixfmt --check flake.nix $(find nix -name '*.nix')
        touch $out
      '';
}
