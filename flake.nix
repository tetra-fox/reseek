{
  description = "reseek, a Soulseek daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
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
      crane,
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
          overlays = [ self.overlays.default ];
        };
      revision = self.shortRev or self.dirtyShortRev or null;
      # the architectures a static release binary is built for, by the name Docker gives them
      releaseTargets = {
        amd64 = "x86_64-unknown-linux-musl";
        arm64 = "aarch64-unknown-linux-musl";
      };
      soulfind = pkgs: pkgs.callPackage ./nix/soulfind.nix { };
      craneLib = pkgs: (crane.mkLib pkgs).overrideToolchain (p: p.reseek-rust);
    in
    {
      # reseek's toolchain comes from rust-overlay, so this overlay brings it along
      overlays.default = lib.composeManyExtensions [
        rust-overlay.overlays.default
        (final: _prev: {
          reseek-rust = final.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          reseek-web = final.callPackage ./nix/web.nix { inherit version; };
          reseek = final.callPackage ./nix/package.nix {
            inherit version revision;
            craneLib = craneLib final;
            web = final.reseek-web;
          };
        })
      ];

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
                craneLib = craneLib pkgs;
                inherit (pkgs) reseek;
                web = pkgs.reseek-web;
              }
            )
          ) releaseTargets
        )
        # built by `just vm-test` rather than among the checks, so `just flake-check` does not run
        # it a second time
        // lib.optionalAttrs (system == "x86_64-linux") {
          relay-test = pkgs.callPackage ./nix/relay-test.nix {
            inherit self;
            soulfind = soulfind pkgs;
          };
        }
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
          home-manager = pkgs.callPackage ./nix/home-manager-test.nix {
            inherit self home-manager;
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
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
                reseek-rust
                cargo-nextest
                cargo-deny
                cargo-insta
                bacon
                watchexec
                just
                # `just dev` opens zellij_dev_layout.kdl in it
                zellij
                typos
                sqlite
                (soulfind pkgs)
                # the completions test reads each script in its shell
                fish
                zsh

                nodejs_26
                pnpm

                nixfmt
                git
              ]
              # the browser `just web-e2e` drives, and the linker .cargo/config.toml names
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
