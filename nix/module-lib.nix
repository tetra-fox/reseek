# What the daemon's and the relay agent's modules share: share entries, configuration files laid
# in config.d, and the daemon's refusal of a default login.
{ lib }:
rec {
  # a shared directory as the options write it, `[alias]` and an excluding `!` taken off
  sharePath =
    entry:
    let
      bare = lib.removePrefix "!" entry;
      aliased = builtins.match "\\[[^]]*](.*)" bare;
    in
    if aliased == null then bare else builtins.head aliased;
  isExclusion = entry: lib.hasPrefix "!" entry;

  # the shared directories of `directories`, the excluded ones left out
  sharePaths = directories: map sharePath (lib.filter (entry: !isExclusion entry) directories);

  inside = parent: path: path == parent || lib.hasPrefix "${lib.removeSuffix "/" parent}/" path;

  # the shares a service only reads: those outside every directory it writes
  readOnlyShares =
    writable: directories:
    lib.filter (share: !(lib.any (parent: inside parent share) writable)) (sharePaths directories);

  # the name a drop-in file is laid in config.d under: its own, without the hash of a file in the
  # store. An attribute name cannot hold the path's store reference, which `source` keeps
  dropInName =
    file:
    let
      path = toString file;
      base = builtins.unsafeDiscardStringContext (baseNameOf path);
    in
    if lib.isStorePath path then builtins.substring 33 (builtins.stringLength base) base else base;

  # the /etc entries of a service's configuration: `config.toml` from `configFile`, then each of
  # `extraConfigFiles` in config.d, in order
  etcFiles =
    unit: configFile: extraConfigFiles:
    [ (lib.nameValuePair "${unit}/config.toml" { source = configFile; }) ]
    ++ lib.imap1 (
      index: file:
      lib.nameValuePair "${unit}/config.d/${lib.fixedWidthNumber 2 index}-${dropInName file}" {
        source = file;
      }
    ) extraConfigFiles;

  # whether the daemon refuses to start because its administrator keeps the default password while
  # other machines can reach the web server. It reads only `settings`, so it answers only for a
  # service whose password and listeners nothing else can set. It mirrors check_default_login in
  # crates/reseek/src/app/startup.rs; change both
  refusesDefaultLogin =
    settings:
    let
      web = settings.web or { };
      auth = web.authentication or { };
      https = web.https or { };
      # "reseek" is the daemon's built-in administrator password
      defaultLogin =
        !(auth.disabled or false)
        && (auth.local.enabled or true)
        && (auth.password_hash or null) == null
        && (auth.password or "reseek") == "reseek";
      # only the usual spellings (127.x, ::1, ::ffff:127.x) count, so a long form such as
      # 0:0:0:0:0:0:0:1 is refused here though the daemon takes it
      isLoopback =
        address:
        let
          bare = lib.toLower (lib.removeSuffix "]" (lib.removePrefix "[" (lib.trim address)));
        in
        lib.hasPrefix "127." bare || bare == "::1" || lib.hasPrefix "::ffff:127." bare;
      # a listener without addresses binds every address
      reachable = list: list == null || !lib.all isLoopback (lib.splitString "," list);
    in
    defaultLogin
    && (
      reachable (web.ip_address or null)
      || !(https.disabled or false) && reachable (https.ip_address or null)
      || web ? external_url
      || (web.trusted_proxies or [ ]) != [ ]
    );

  # how to keep the daemon from refusing a default login
  defaultLoginAdvice = "Set settings.web.authentication.password_hash from `reseek hash-password`, or RESEEK_PASSWORD in environmentFile; or keep settings.web.ip_address on 127.0.0.1 and either settings.web.https.ip_address on 127.0.0.1 or settings.web.https.disabled = true, with no reverse proxy.";

  refusedDefaultLogin =
    unit:
    "services.${unit}: the administrator still has the default password and other machines can reach the web server, so the daemon refuses to start. ${defaultLoginAdvice}";

  # for a service whose `others` may set a password or move a listener, which the module cannot read
  uncheckedDefaultLogin =
    unit: others:
    "services.${unit}: the settings keep the default password where other machines can reach the web server. The module cannot read ${others}, so it leaves this check to the daemon, which refuses to start unless they change that. ${defaultLoginAdvice}";
}
