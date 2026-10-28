# Builds a home with the Home Manager module enabled and checks what it generates: the user unit
# runs the package with the linked configuration, marked managed by Home Manager, and the
# configuration holds the settings.
{
  self,
  home-manager,
  pkgs,
  runCommand,
}:
let
  home = home-manager.lib.homeManagerConfiguration {
    inherit pkgs;
    modules = [
      self.homeManagerModules.default
      {
        home = {
          username = "someone";
          homeDirectory = "/home/someone";
          stateVersion = "25.05";
        };
        services.reseek = {
          enable = true;
          environmentFile = "/run/user/1000/reseek.env";
          settings.soulseek.username = "someone";
        };
      }
    ];
  };
  files = home.config.home-files;
  unit = "${files}/.config/systemd/user/reseek.service";
in
runCommand "reseek-home-manager" { } ''
  grep -q '^ExecStart=.*/bin/reseek --config-file /home/someone/.config/reseek/config.toml --app-dir /home/someone/.local/share/reseek' ${unit}
  grep -q '^Environment=RESEEK_CONFIG_MANAGED_BY=home-manager' ${unit}
  grep -q '^EnvironmentFile=/run/user/1000/reseek.env' ${unit}
  grep -q '^RestartForceExitStatus=75' ${unit}
  grep -q 'username = "someone"' ${files}/.config/reseek/config.toml
  touch $out
''
