# The NixOS module: services.reseek runs the daemon as a hardened system service, and
# services.reseek.instances.<name> runs more of them, each with its own user, state directory,
# ports and Soulseek account. The configuration is rendered to TOML in the store and marked
# managed by NixOS, so the daemon shows it read-only; a change to it reloads the running
# daemon with SIGHUP, or restarts it with restartOnChange. services.reseek-agent, the relay
# agent, is in agent-module.nix.
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
  cfg = config.services.reseek;
  settingsFormat = pkgs.formats.toml { };
  inherit (import ./module-lib.nix { inherit lib; })
    sharePath
    readOnlyShares
    etcFiles
    refusesDefaultLogin
    refusedDefaultLogin
    uncheckedDefaultLogin
    ;

  # the options of one daemon; `name` is its unit, user and directory name, `enabled` whether
  # it runs unless `enable` says otherwise
  instanceOptions =
    {
      name,
      enabled ? false,
    }:
    {
      enable = mkEnableOption "the reseek Soulseek daemon" // {
        default = enabled;
      };

      package = mkOption {
        type = types.package;
        default = pkgs.reseek or self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        defaultText = literalExpression "pkgs.reseek";
        description = "The reseek package to run.";
      };

      user = mkOption {
        type = types.str;
        default = name;
        description = ''
          The user the daemon runs as; created when left at its default. A dedicated system
          user, not a dynamic one, so share directories can be made readable to a stable uid.
        '';
      };

      group = mkOption {
        type = types.str;
        default = name;
        description = "The daemon's primary group; created when left at its default.";
      };

      supplementaryGroups = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [ "media" ];
        description = "More groups the daemon runs in, such as one that may read the shares.";
      };

      dataDir = mkOption {
        type = types.path;
        default = "/var/lib/${name}";
        description = ''
          Where the databases, logs, runtime overrides and, unless the settings say otherwise,
          the downloads and incomplete files live.
        '';
      };

      settings = mkOption {
        default = { };
        description = ''
          The configuration, rendered to TOML with the daemon's own key paths. It lands in the
          world-readable store: put secrets in `environmentFile` or `extraConfigFiles` instead.
        '';
        type = types.submodule {
          freeformType = settingsFormat.type;
          options = {
            remote_configuration = mkOption {
              type = types.bool;
              default = true;
              description = "Lets administrators change settings from the web UI and the API.";
            };
            soulseek = mkOption {
              default = { };
              description = "The Soulseek account and connection.";
              type = types.submodule {
                freeformType = settingsFormat.type;
                options = {
                  username = mkOption {
                    type = types.nullOr types.str;
                    default = null;
                    description = "The Soulseek account; its password goes in `environmentFile` as `RESEEK_SLSK_PASSWORD`.";
                  };
                  listen_port = mkOption {
                    type = types.port;
                    default = 50300;
                    description = "The port other users connect to.";
                  };
                };
              };
            };
            web = mkOption {
              default = { };
              description = "The HTTP API and web UI.";
              type = types.submodule {
                freeformType = settingsFormat.type;
                options = {
                  port = mkOption {
                    type = types.port;
                    default = 5030;
                    description = "The HTTP port.";
                  };
                  url_base = mkOption {
                    type = types.str;
                    default = "/";
                    example = "/reseek";
                    description = "The path the daemon is served under behind a reverse proxy.";
                  };
                  https = mkOption {
                    default = { };
                    description = "The HTTPS listener.";
                    type = types.submodule {
                      freeformType = settingsFormat.type;
                      options = {
                        disabled = mkOption {
                          type = types.bool;
                          default = false;
                          description = "Does not listen for HTTPS.";
                        };
                        port = mkOption {
                          type = types.port;
                          default = 5031;
                          description = "The HTTPS port.";
                        };
                      };
                    };
                  };
                  authentication = mkOption {
                    default = { };
                    description = ''
                      Who may sign in. The administrator's password goes in `environmentFile` as
                      `RESEEK_PASSWORD`, or as `password_hash` from `reseek hash-password`.
                    '';
                    type = types.submodule {
                      freeformType = settingsFormat.type;
                      options.password_hash = mkOption {
                        type = types.nullOr types.str;
                        default = null;
                        description = "The administrator's password as an argon2id hash.";
                      };
                    };
                  };
                };
              };
            };
            shares = mkOption {
              default = { };
              description = "What the daemon shares.";
              type = types.submodule {
                freeformType = settingsFormat.type;
                options.directories = mkOption {
                  type = types.listOf types.str;
                  default = [ ];
                  example = [
                    "/srv/music"
                    "[vinyl]/srv/rips"
                    "!/srv/music/private"
                  ];
                  description = ''
                    Shared directories, each absolute, optionally with `[alias]` before it, or
                    `!` before one to leave out.
                  '';
                };
              };
            };
            directories = mkOption {
              default = { };
              description = "Where downloads go.";
              type = types.submodule {
                freeformType = settingsFormat.type;
                options = {
                  downloads = mkOption {
                    type = types.nullOr types.str;
                    default = null;
                    description = "Where finished downloads are moved; `downloads` in the data directory when not set.";
                  };
                  incomplete = mkOption {
                    type = types.nullOr types.str;
                    default = null;
                    description = "Where downloads are written while they run; `incomplete` in the data directory when not set.";
                  };
                };
              };
            };
          };
        };
      };

      environmentFile = mkOption {
        type = types.nullOr types.path;
        default = null;
        example = "/run/secrets/reseek.env";
        description = ''
          A file of `NAME=value` lines for secrets, such as `RESEEK_SLSK_PASSWORD`,
          `RESEEK_PASSWORD` and `RESEEK_API_KEY`. Not copied to the store.
        '';
      };

      environmentFiles = mkOption {
        type = types.listOf types.path;
        default = [ ];
        description = "More files like `environmentFile`.";
      };

      extraConfigFiles = mkOption {
        type = types.listOf types.path;
        default = [ ];
        example = literalExpression "[ config.age.secrets.reseek-oidc.path ]";
        description = ''
          TOML files laid over the settings in order, for secrets rendered by agenix or
          sops-nix and owned by the service user. A table keyed by name, such as
          `web.authentication.api_keys`, set in such a file replaces the whole table, so the
          file holds all of it.
        '';
      };

      openFirewall = mkOption {
        type = types.bool;
        default = false;
        description = "Opens the Soulseek listen port.";
      };

      openWebFirewall = mkOption {
        type = types.bool;
        default = false;
        description = "Opens the HTTP port, and the HTTPS port unless HTTPS is off.";
      };

      restartOnChange = mkOption {
        type = types.bool;
        default = false;
        description = ''
          Restarts the daemon when its configuration changes; otherwise the running daemon
          reloads it, and keys that need a restart show as pending.
        '';
      };

      extraArgs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Appended to the daemon's command line.";
      };

      reverseProxy = mkOption {
        default = null;
        description = ''
          A virtual host in front of the daemon, served over HTTPS with ACME. The daemon then
          trusts the proxy on loopback and takes `https://<host><url_base>` as its public URL.
        '';
        type = types.nullOr (
          types.submodule {
            options = {
              host = mkOption {
                type = types.str;
                example = "reseek.example.net";
                description = "The virtual host's name.";
              };
              server = mkOption {
                type = types.enum [
                  "nginx"
                  "caddy"
                ];
                default = "nginx";
                description = "The web server that proxies.";
              };
            };
          }
        );
      };
    };

  # every enabled daemon, with its unit name, its /etc directory and its default user
  instances =
    lib.optional cfg.enable (
      cfg
      // {
        unit = "reseek";
      }
    )
    ++ lib.mapAttrsToList (name: instance: instance // { unit = "reseek-${name}"; }) (
      lib.filterAttrs (_: instance: instance.enable) cfg.instances
    );

  # the settings with the module's own additions and nothing null, which TOML cannot hold
  settingsOf =
    instance:
    lib.recursiveUpdate (lib.filterAttrsRecursive (_: value: value != null) instance.settings) (
      lib.optionalAttrs (instance.reverseProxy != null) {
        web.trusted_proxies = [
          "127.0.0.1/32"
          "::1/128"
        ];
        web.external_url = "https://${instance.reverseProxy.host}${lib.removeSuffix "/" instance.settings.web.url_base}";
      }
    );

  configFileOf = instance: settingsFormat.generate "${instance.unit}.toml" (settingsOf instance);

  # the directories the daemon writes outside its data directory
  writablePaths =
    instance:
    [ instance.dataDir ]
    ++ lib.filter (path: path != null) [
      instance.settings.directories.downloads
      instance.settings.directories.incomplete
    ];

  serviceOf =
    instance:
    let
      configFile = configFileOf instance;
      writable = writablePaths instance;
      readOnly = readOnlyShares writable instance.settings.shares.directories;
      stateName = lib.removePrefix "/var/lib/" instance.dataDir;
      inStateDirectory = lib.hasPrefix "/var/lib/" instance.dataDir;
    in
    {
      description = "reseek Soulseek daemon${
        lib.optionalString (instance.unit != "reseek") " (${instance.unit})"
      }";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      reloadTriggers = lib.optionals (!instance.restartOnChange) (
        [ configFile ] ++ instance.extraConfigFiles
      );
      restartTriggers = lib.optionals instance.restartOnChange (
        [ configFile ] ++ instance.extraConfigFiles
      );
      environment.RESEEK_CONFIG_MANAGED_BY = "Nix";
      serviceConfig = {
        ExecStart = lib.escapeShellArgs (
          [
            (lib.getExe instance.package)
            "--config-file"
            "/etc/${instance.unit}/config.toml"
            "--app-dir"
            instance.dataDir
          ]
          ++ instance.extraArgs
        );
        ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
        EnvironmentFile =
          lib.optional (instance.environmentFile != null) instance.environmentFile
          ++ instance.environmentFiles;
        User = instance.user;
        Group = instance.group;
        SupplementaryGroups = instance.supplementaryGroups;
        StateDirectory = lib.mkIf inStateDirectory stateName;
        StateDirectoryMode = lib.mkIf inStateDirectory "0750";
        UMask = "0027";
        Restart = "on-failure";
        RestartSec = 5;
        # the daemon exits 75 when a restart is asked for through the API
        RestartForceExitStatus = 75;
        # the daemon exits 3 when it refuses its configuration, which another start does not change
        RestartPreventExitStatus = 3;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        PrivateTmp = true;
        NoNewPrivileges = true;
        # a path that does not exist yet, such as a disk not mounted, is left out rather than
        # failing the unit, so the daemon starts and says what is missing
        ReadWritePaths = map (path: "-${path}") writable;
        ReadOnlyPaths = map (path: "-${path}") readOnly;
        # netlink is how the daemon finds the default gateway to ask it for a port mapping
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
          "AF_NETLINK"
        ];
        SystemCallFilter = [ "@system-service" ];
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        LimitNOFILE = 65536;
      };
    };

  # the assertions and warnings of one daemon
  checksOf =
    instance:
    let
      inherit (instance) settings unit;
      noSecrets = instance.environmentFile == null && instance.environmentFiles == [ ];
      # nothing but the settings can set a password or move a listener
      settingsAlone = noSecrets && instance.extraConfigFiles == [ ] && instance.extraArgs == [ ];
      refuses = refusesDefaultLogin (settingsOf instance);
    in
    {
      assertions = [
        {
          assertion = lib.all (entry: lib.hasPrefix "/" (sharePath entry)) settings.shares.directories;
          message = "services.${unit}: every entry of settings.shares.directories is an absolute path.";
        }
        {
          assertion = settings.web.https.disabled || settings.web.port != settings.web.https.port;
          message = "services.${unit}: settings.web.port and settings.web.https.port are the same port.";
        }
        {
          assertion = !(settingsAlone && refuses);
          message = refusedDefaultLogin unit;
        }
      ];
      # what an environment file holds is not known here, so a missing account only warns
      warnings =
        lib.optional (settings.soulseek.username == null && noSecrets)
          "services.${unit}: no Soulseek account: set settings.soulseek.username, or RESEEK_SLSK_USERNAME and RESEEK_SLSK_PASSWORD in environmentFile."
        ++
          lib.optional (settings.soulseek ? password)
            "services.${unit}: settings.soulseek.password lands in the world-readable store; put RESEEK_SLSK_PASSWORD in environmentFile."
        ++
          lib.optional
            (settings.remote_configuration && settings.web.authentication.password_hash == null && noSecrets)
            "services.${unit}: remote_configuration is on and the administrator still has the default password; set web.authentication.password_hash or RESEEK_PASSWORD in environmentFile."
        # an environment file is where the password is meant to go, so a service with one is not
        # warned about
        ++ lib.optional (noSecrets && !settingsAlone && refuses) (
          uncheckedDefaultLogin unit "extraArgs or extraConfigFiles"
        );
    };

  ports =
    instance:
    lib.optional instance.openFirewall instance.settings.soulseek.listen_port
    ++ lib.optionals instance.openWebFirewall (
      [ instance.settings.web.port ]
      ++ lib.optional (!instance.settings.web.https.disabled) instance.settings.web.https.port
    );

  proxied =
    server:
    lib.filter (
      instance: instance.reverseProxy != null && instance.reverseProxy.server == server
    ) instances;
  upstream = instance: "http://127.0.0.1:${toString instance.settings.web.port}";
in
{
  imports = [ (import ./agent-module.nix { inherit self; }) ];

  options.services.reseek = instanceOptions { name = "reseek"; } // {
    instances = mkOption {
      default = { };
      description = ''
        More daemons, each with its own unit `reseek-<name>`, user, state directory, ports and
        Soulseek account; enabled unless `enable` says otherwise.
      '';
      type = types.attrsOf (
        types.submodule (
          { name, ... }:
          {
            options = instanceOptions {
              name = "reseek-${name}";
              enabled = true;
            };
          }
        )
      );
    };
  };

  config = lib.mkIf (instances != [ ]) {
    assertions = lib.concatMap (instance: (checksOf instance).assertions) instances;
    warnings = lib.concatMap (instance: (checksOf instance).warnings) instances;

    environment.etc = lib.listToAttrs (
      lib.concatMap (
        instance: etcFiles instance.unit (configFileOf instance) instance.extraConfigFiles
      ) instances
    );

    systemd.services = lib.listToAttrs (
      map (instance: lib.nameValuePair instance.unit (serviceOf instance)) instances
    );

    systemd.tmpfiles.rules = lib.concatMap (
      instance:
      lib.optional (
        !lib.hasPrefix "/var/lib/" instance.dataDir
      ) "d ${instance.dataDir} 0750 ${instance.user} ${instance.group} -"
    ) instances;

    users.users = lib.listToAttrs (
      map (
        instance:
        lib.nameValuePair instance.user {
          isSystemUser = true;
          group = instance.group;
          home = instance.dataDir;
        }
      ) (lib.filter (instance: instance.user == instance.unit) instances)
    );
    users.groups = lib.listToAttrs (
      map (instance: lib.nameValuePair instance.group { }) (
        lib.filter (instance: instance.group == instance.unit) instances
      )
    );

    networking.firewall.allowedTCPPorts = lib.concatMap ports instances;

    services.nginx = lib.mkIf (proxied "nginx" != [ ]) {
      enable = true;
      virtualHosts = lib.listToAttrs (
        map (
          instance:
          lib.nameValuePair instance.reverseProxy.host {
            forceSSL = true;
            enableACME = true;
            locations.${instance.settings.web.url_base} = {
              proxyPass = upstream instance;
              proxyWebsockets = true;
              # the client's address and host for the daemon, which trusts them from loopback
              recommendedProxySettings = true;
              # the live event stream is read as it is written
              extraConfig = "proxy_buffering off;";
            };
          }
        ) (proxied "nginx")
      );
    };

    services.caddy = lib.mkIf (proxied "caddy" != [ ]) {
      enable = true;
      virtualHosts = lib.listToAttrs (
        map (
          instance:
          lib.nameValuePair instance.reverseProxy.host {
            extraConfig = "reverse_proxy ${lib.removePrefix "http://" (upstream instance)}";
          }
        ) (proxied "caddy")
      );
    };
  };
}
