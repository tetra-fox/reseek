# The Home Manager module: services.reseek runs the daemon as the user's own service, a
# systemd user unit on Linux and a launchd agent on macOS. The configuration is rendered to
# TOML in the store, linked into the user's configuration directory and marked managed by Home
# Manager, so the daemon shows it read-only.
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (lib) mkOption types literalExpression;
  cfg = config.services.reseek;
  settingsFormat = pkgs.formats.toml { };
  configDir = "${config.xdg.configHome}/reseek";
  configFile = settingsFormat.generate "reseek.toml" cfg.settings;
  arguments = [
    (lib.getExe cfg.package)
    "--config-file"
    "${configDir}/config.toml"
    "--app-dir"
    cfg.dataDir
  ];
  environmentFiles = lib.optional (cfg.environmentFile != null) cfg.environmentFile;
  inherit (import ./module-lib.nix { inherit lib; })
    refusesDefaultLogin
    refusedDefaultLogin
    uncheckedDefaultLogin
    ;
  # the settings keep a default login others reach, and no environment file sets a password
  refuses = cfg.environmentFile == null && refusesDefaultLogin cfg.settings;
in
{
  options.services.reseek = {
    enable = lib.mkEnableOption "the reseek Soulseek daemon as a user service";

    package = mkOption {
      type = types.package;
      default = pkgs.reseek or self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = literalExpression "pkgs.reseek";
      description = "The reseek package to run.";
    };

    dataDir = mkOption {
      type = types.str;
      default = "${config.xdg.dataHome}/reseek";
      defaultText = literalExpression ''"''${config.xdg.dataHome}/reseek"'';
      description = "Where the databases, logs, runtime overrides, downloads and incomplete files live.";
    };

    settings = mkOption {
      type = settingsFormat.type;
      default = { };
      example = literalExpression ''
        {
          soulseek.username = "me";
          shares.directories = [ "''${config.home.homeDirectory}/Music" ];
        }
      '';
      description = ''
        The configuration, rendered to TOML with the daemon's own key paths. It lands in the
        world-readable store: put secrets in `environmentFile` or `extraConfigFiles` instead.
      '';
    };

    environmentFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = ''
        A file of `NAME=value` lines for secrets, such as `RESEEK_SLSK_PASSWORD` and
        `RESEEK_PASSWORD`, read when the daemon starts. Not copied to the store.
      '';
    };

    extraConfigFiles = mkOption {
      type = types.listOf types.path;
      default = [ ];
      description = "TOML files laid over the settings in order, for secrets rendered elsewhere.";
    };
  };

  config = lib.mkIf cfg.enable {
    # without an environment file or a laid-over configuration file, only the settings can set a
    # password or move a listener
    assertions = [
      {
        assertion = !(refuses && cfg.extraConfigFiles == [ ]);
        message = refusedDefaultLogin "reseek";
      }
    ];
    warnings = lib.optional (refuses && cfg.extraConfigFiles != [ ]) (
      uncheckedDefaultLogin "reseek" "extraConfigFiles"
    );

    xdg.configFile = {
      "reseek/config.toml".source = configFile;
    }
    // lib.listToAttrs (
      lib.imap1 (
        index: file:
        lib.nameValuePair "reseek/config.d/${lib.fixedWidthNumber 2 index}-${baseNameOf (toString file)}" {
          source = file;
        }
      ) cfg.extraConfigFiles
    );

    systemd.user.services.reseek = lib.mkIf pkgs.stdenv.hostPlatform.isLinux {
      Unit = {
        Description = "reseek Soulseek daemon";
        X-Restart-Triggers = [ configFile ];
      };
      Service = {
        ExecStart = lib.escapeShellArgs arguments;
        ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
        Environment = [ "RESEEK_CONFIG_MANAGED_BY=home-manager" ];
        EnvironmentFile = environmentFiles;
        Restart = "on-failure";
        # the daemon exits 75 when a restart is asked for through the API
        RestartForceExitStatus = 75;
        # the daemon exits 3 when it refuses its configuration, which another start does not change
        RestartPreventExitStatus = 3;
      };
      Install.WantedBy = [ "default.target" ];
    };

    launchd.agents.reseek = lib.mkIf pkgs.stdenv.hostPlatform.isDarwin {
      enable = true;
      config = {
        # launchd reads no environment file, so a shell exports it before starting the daemon
        ProgramArguments =
          if cfg.environmentFile == null then
            arguments
          else
            [
              "/bin/sh"
              "-c"
              ''set -a; . ${lib.escapeShellArg cfg.environmentFile}; set +a; exec "$@"''
              "reseek"
            ]
            ++ arguments;
        EnvironmentVariables.RESEEK_CONFIG_MANAGED_BY = "home-manager";
        KeepAlive = true;
        RunAtLoad = true;
      };
    };
  };
}
