{
  description = "A NixOS machine running reseek";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    reseek = {
      url = "github:tetra-fox/reseek";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, reseek, ... }:
    {
      nixosConfigurations.machine = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          reseek.nixosModules.default
          ./configuration.nix
          {
            services.reseek = {
              enable = true;
              openFirewall = true;
              # RESEEK_SLSK_PASSWORD and RESEEK_PASSWORD, kept out of the store
              environmentFile = "/var/lib/secrets/reseek.env";
              settings = {
                soulseek.username = "your-soulseek-name";
                shares.directories = [ "/srv/music" ];
              };
            };
          }
        ];
      };
    };
}
