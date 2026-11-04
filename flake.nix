{
  description = "reseek, a Soulseek daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # for the check that builds a home with the Home Manager module
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      home-manager,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        # nixpkgs dropped x86_64-darwin in 26.11
        "aarch64-darwin"
      ];
      forAllSystems = lib.genAttrs systems;
      version = (lib.importTOML ./Cargo.toml).workspace.package.version;
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
            self.overlays.default
          ];
        };
      revision = self.shortRev or self.dirtyShortRev or null;
      # the architectures a static release binary is built for, by the name Docker gives them
      releaseTargets = {
        amd64 = "x86_64-unknown-linux-musl";
        arm64 = "aarch64-unknown-linux-musl";
      };
      # test fixtures only; nothing in reseek links to them
      fixtures = pkgs: {
        soulfind = pkgs.callPackage ./nix/soulfind.nix { };
        soulseek-rs = pkgs.callPackage ./nix/soulseek-rs.nix { };
      };
    in
    {
      overlays.default = final: _prev: {
        reseek-web = final.callPackage ./nix/web.nix { inherit version; };
        reseek = final.callPackage ./nix/package.nix {
          inherit version;
          web = final.reseek-web;
          inherit revision;
          tests = lib.optionalAttrs final.stdenv.hostPlatform.isLinux {
            nixos-vm = self.checks.${final.stdenv.hostPlatform.system}.nixos-vm;
          };
        };
      };

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.reseek;
          web = pkgs.reseek-web;
        }
        # the static release binaries, each built from any Linux machine
        // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (
          lib.mapAttrs' (
            arch: target:
            lib.nameValuePair "release-${arch}" (
              pkgs.callPackage ./nix/static.nix {
                inherit target revision;
                inherit (pkgs) reseek;
                web = pkgs.reseek-web;
              }
            )
          ) releaseTargets
        )
      );

      nixosModules = {
        default = import ./nix/module.nix { inherit self; };
        reseek = self.nixosModules.default;
      };

      homeManagerModules.default = import ./nix/home-manager.nix { inherit self; };

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          package = pkgs.reseek;
        }
        // pkgs.callPackages ./nix/checks.nix { }
        // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          nixos-vm = pkgs.callPackage ./nix/vm-test.nix (
            fixtures pkgs
            // {
              inherit self;
              reseek-smoke = pkgs.callPackage ./nix/smoke.nix { };
            }
          );
          home-manager = pkgs.callPackage ./nix/home-manager-test.nix {
            inherit self home-manager;
          };
        }
      );

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
            packages =
              with pkgs;
              [
                stable
                cargo-nextest
                cargo-deny
                cargo-insta
                bacon
                watchexec
                just
                typos
                sqlite
                # the Soulseek server the client and daemon test suites start, and the other
                # user in `just e2e`
                (fixtures pkgs).soulfind
                (fixtures pkgs).soulseek-rs
                # the completions test reads each script in its shell
                fish
                zsh

                nodejs_26
                pnpm

                nixfmt
                git
              ]
              # the browser `just web-smoke` drives, and the linker .cargo/config.toml names
              ++ lib.optionals stdenv.hostPlatform.isLinux [
                chromium
                mold
              ];

            env.RUST_BACKTRACE = "1";
          };

          fuzz = pkgs.mkShell {
            packages = [
              nightly
              pkgs.cargo-fuzz
              pkgs.just
            ]
            # the linker .cargo/config.toml names, which `just fuzz-seed` links with
            ++ lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.mold;
          };
        }
      );

      templates.default = {
        path = ./nix/template;
        description = "A NixOS machine running reseek through its module";
      };

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
