{
  description = "reseek, a Soulseek daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          stable = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "clippy"
              "rustfmt"
            ];
          };
          # cargo-fuzz builds with -Zsanitizer, which only nightly accepts
          nightly = pkgs.rust-bin.selectLatestNightlyWith (
            toolchain: toolchain.minimal.override { extensions = [ "rust-src" ]; }
          );
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              stable
              cargo-nextest
              cargo-deny
              cargo-insta
              bacon
              watchexec
              just
              typos
              sqlite

              nodejs_24
              pnpm

              nixfmt
              git
            ];

            env.RUST_BACKTRACE = "1";
          };

          fuzz = pkgs.mkShell {
            packages = [
              nightly
              pkgs.cargo-fuzz
              pkgs.just
            ];
          };
        }
      );
    };
}
