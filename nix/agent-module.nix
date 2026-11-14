# The relay agent's NixOS module: services.reseek-agent runs `reseek-agent` as a hardened system
# service with a user of its own, sharing this machine's files through a reseek daemon elsewhere,
# its controller. It only dials out, so no port is opened. The configuration is rendered to TOML
# in the store with the agent's keys, a subset of the daemon's; a change to it restarts the
# agent, which reconnects and sends its shares again.
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (lib)
    mkOption
    mkEnableOption
    types
    literalExpression
    ;
  cfg = config.services.reseek-agent;
  settingsFormat = pkgs.formats.toml { };
  inherit (import ./module-lib.nix { inherit lib; })
    sharePath
    readOnlyShares
    etcFiles
    ;
  unit = "reseek-agent";

  settings = lib.filterAttrsRecursive (_: value: value != null) cfg.settings;
  configFile = settingsFormat.generate "${unit}.toml" settings;
  writable = [
    cfg.dataDir
  ]
  ++ lib.optional (cfg.settings.directories.downloads != null) cfg.settings.directories.downloads;
  stateName = lib.removePrefix "/var/lib/" cfg.dataDir;
  inStateDirectory = lib.hasPrefix "/var/lib/" cfg.dataDir;
in
{
  options.services.reseek-agent = {
    enable = mkEnableOption "the reseek relay agent";

    package = mkOption {
      type = types.package;
      default = pkgs.reseek or self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = literalExpression "pkgs.reseek";
      description = "The reseek package whose `reseek-agent` program runs.";
    };

    user = mkOption {
      type = types.str;
      default = unit;
      description = ''
        The user the agent runs as; created when left at its default. A dedicated system user,
        so share directories can be made readable to a stable uid.
      '';
    };

    group = mkOption {
      type = types.str;
      default = unit;
      description = "The agent's primary group; created when left at its default.";
    };

    supplementaryGroups = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "media" ];
      description = "More groups the agent runs in, such as one that may read the shares.";
    };

    dataDir = mkOption {
      type = types.path;
      default = "/var/lib/${unit}";
      description = ''
        The agent's application directory: the share index lives in its `cache` directory, its
        device id in `data/device-id` and, unless the settings say otherwise, the downloads
        copied from the controller in `data/downloads`.
      '';
    };

    settings = mkOption {
      default = { };
      description = ''
        The configuration, rendered to TOML with the agent's key paths, the same as the daemon's
        for the same options. It lands in the world-readable store: put the controller's API key
        and this agent's secret in `environmentFile` instead.
      '';
      type = types.submodule {
        freeformType = settingsFormat.type;
        options = {
          instance_name = mkOption {
            type = types.str;
            example = "nas";
            description = "The name the controller lists this agent under.";
          };
          relay = mkOption {
            default = { };
            description = "The controller this agent connects to.";
            type = types.submodule {
              freeformType = settingsFormat.type;
              options.controller = mkOption {
                default = { };
                description = "The controller this agent connects to.";
                type = types.submodule {
                  freeformType = settingsFormat.type;
                  options.address = mkOption {
                    type = types.str;
                    example = "https://reseek.example.net";
                    description = ''
                      The controller's base URL. Its API key goes in `environmentFile` as
                      `RESEEK_CONTROLLER_API_KEY`, and the secret it holds for this agent as
                      `RESEEK_CONTROLLER_SECRET`.
                    '';
                  };
                };
              };
            };
          };
          shares = mkOption {
            default = { };
            description = "What the agent shares.";
            type = types.submodule {
              freeformType = settingsFormat.type;
              options.directories = mkOption {
                type = types.listOf types.str;
                default = [ ];
                example = [
                  "/srv/music"
                  "!/srv/music/private"
                ];
                description = ''
                  Shared directories, each absolute, optionally with `[alias]` before it, or `!`
                  before one to leave out.
                '';
              };
            };
          };
          directories = mkOption {
            default = { };
            description = "Where downloads go.";
            type = types.submodule {
              freeformType = settingsFormat.type;
              options.downloads = mkOption {
                type = types.nullOr types.str;
                default = null;
                description = ''
                  Where the controller's finished downloads are copied when
                  `relay.controller.downloads` is on; `data/downloads` in `dataDir` when not
                  set.
                '';
              };
            };
          };
        };
      };
    };

    environmentFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      example = "/run/secrets/reseek-agent.env";
      description = ''
        A file of `NAME=value` lines for secrets: `RESEEK_CONTROLLER_API_KEY` and
        `RESEEK_CONTROLLER_SECRET`. Not copied to the store. A variable ranks below the
        configuration files, so an option set in `settings` or `extraConfigFiles` wins over it.
      '';
    };

    extraConfigFiles = mkOption {
      type = types.listOf types.path;
      default = [ ];
      description = "TOML files laid over the settings in order, for secrets rendered by agenix or sops-nix.";
    };

    extraArgs = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Appended to the agent's command line.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = lib.all (entry: lib.hasPrefix "/" (sharePath entry)) cfg.settings.shares.directories;
        message = "services.${unit}: every entry of settings.shares.directories is an absolute path.";
      }
    ];
    warnings =
      lib.optional (cfg.settings.relay.controller ? api_key || cfg.settings.relay.controller ? secret)
        "services.${unit}: the controller's API key and secret in settings land in the world-readable store; put RESEEK_CONTROLLER_API_KEY and RESEEK_CONTROLLER_SECRET in environmentFile.";

    environment.etc = lib.listToAttrs (etcFiles unit configFile cfg.extraConfigFiles);

    systemd.services.${unit} = {
      description = "reseek relay agent";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      restartTriggers = [ configFile ] ++ cfg.extraConfigFiles;
      serviceConfig = {
        ExecStart = lib.escapeShellArgs (
          [
            (lib.getExe' cfg.package "reseek-agent")
            "--config"
            "/etc/${unit}/config.toml"
            "--app-dir"
            cfg.dataDir
          ]
          ++ cfg.extraArgs
        );
        EnvironmentFile = lib.optional (cfg.environmentFile != null) cfg.environmentFile;
        User = cfg.user;
        Group = cfg.group;
        SupplementaryGroups = cfg.supplementaryGroups;
        StateDirectory = lib.mkIf inStateDirectory stateName;
        StateDirectoryMode = lib.mkIf inStateDirectory "0750";
        UMask = "0027";
        Restart = "on-failure";
        RestartSec = 5;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        PrivateTmp = true;
        NoNewPrivileges = true;
        ReadWritePaths = map (path: "-${path}") writable;
        ReadOnlyPaths = map (path: "-${path}") (readOnlyShares writable cfg.settings.shares.directories);
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        SystemCallFilter = [ "@system-service" ];
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
      };
    };

    systemd.tmpfiles.rules = lib.optional (
      !inStateDirectory
    ) "d ${cfg.dataDir} 0750 ${cfg.user} ${cfg.group} -";

    users.users = lib.mkIf (cfg.user == unit) {
      ${unit} = {
        isSystemUser = true;
        inherit (cfg) group;
        home = cfg.dataDir;
      };
    };
    users.groups = lib.mkIf (cfg.group == unit) { ${unit} = { }; };
  };
}
