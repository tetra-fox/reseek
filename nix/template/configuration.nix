# The rest of the machine. This is a stand-in so the flake evaluates: replace it with your own
# configuration.nix and the hardware-configuration.nix that nixos-generate-config writes.
{
  boot.loader.grub.device = "/dev/sda";
  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
  };
  # the NixOS release the machine was first installed with
  system.stateVersion = "26.11";
}
