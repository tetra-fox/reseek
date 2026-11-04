# What the daemon's and the relay agent's NixOS modules both need of a share entry and of a
# configuration file laid in config.d.
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
}
