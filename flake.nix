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
          overlays = [ self.overlays.default ];
        };
      revision = self.shortRev or self.dirtyShortRev or null;
      # the architectures a static release binary is built for, by the name Docker gives them
      releaseTargets = {
        amd64 = "x86_64-unknown-linux-musl";
        arm64 = "aarch64-unknown-linux-musl";
      };
      # the Soulseek server the client and daemon test suites start; nothing in reseek links to it
      soulfind = pkgs: pkgs.callPackage ./nix/soulfind.nix { };
    in
    {
      # reseek's toolchain comes from rust-overlay, so this overlay brings it along
      overlays.default = lib.composeManyExtensions [
        rust-overlay.overlays.default
        (final: _prev: {
          # the Rust in rust-toolchain.toml, and a platform that builds with it
          reseek-rust = final.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          reseek-rust-platform = final.makeRustPlatform {
            cargo = final.reseek-rust;
            rustc = final.reseek-rust;
          };
          reseek-web = final.callPackage ./nix/web.nix { inherit version; };
          reseek = final.callPackage ./nix/package.nix {
            inherit version;
            web = final.reseek-web;
            inherit revision;
            rustPlatform = final.reseek-rust-platform;
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
        // pkgs.callPackages ./nix/checks.nix {
          rustPlatform = pkgs.reseek-rust-platform;
          clippy = pkgs.reseek-rust;
          rustfmt = pkgs.reseek-rust;
        }
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
