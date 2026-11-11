import type { Schemas } from "#lib/api/client.js";
import { listText } from "#lib/format.js";
import { VISIBLE_TO, type VisibleTo, withAudience } from "#lib/users/groups.js";

/** The option listing the shared and excluded folders */
export const DIRECTORIES = "shares.directories";

/** The entry of `shares.directories` for a folder: the alias is optional, `!` excludes it */
export const shareEntry = (path: string, alias: string, exclude: boolean) =>
  `${exclude ? "!" : ""}${alias.trim() ? `[${alias.trim()}]` : ""}${path}`;

/**
 * The alias an entry of `shares.directories` shares or excludes its folder under, as the daemon
 * reads it: the one in brackets, else the folder's name
 */
export const entryAlias = (entry: string) => {
  const rest = entry.replace(/[/\\]+$/, "").replace(/^[!-]/, "");
  const bracketed = /^\[([^\]]*)\]/.exec(rest);
  if (bracketed) return bracketed[1];
  return rest.slice(Math.max(rest.lastIndexOf("/"), rest.lastIndexOf("\\")) + 1);
};

/** The alias a share of the folder `path` goes by: `alias` when given, else the folder's name */
export const aliasOf = (path: string, alias: string) => entryAlias(shareEntry(path, alias, false));

/**
 * The aliases of the shares and exclusions `entries` lists, lower-cased as the daemon compares
 * them, with that of the downloads directory when `shareDownloads` shares it and no entry does.
 * `downloads` is that directory, null for the default one, `downloads` in the data directory
 */
export const takenAliases = (
  entries: string[],
  shareDownloads: boolean,
  downloads: string | null
) => {
  const aliases = entries.map(entryAlias);
  const listed =
    downloads !== null && entries.some((entry) => entryPath(entry) === entryPath(downloads));
  if (shareDownloads && !listed)
    aliases.push(downloads === null ? "downloads" : entryAlias(downloads));
  return new Set(aliases.map((alias) => alias.toLowerCase()));
};

/**
 * What adding `entry` to the shared folders `entries` writes, for `writeFromPage`: who sees its
 * alias first when `audience` changes it, then the folders. `writeFromPage` writes in that
 * order, so when the two live in different files and the second write fails, what is left is an
 * audience for an alias no share has yet, never a share shown to more than was asked
 */
export const addition = (
  entries: string[],
  entry: string,
  visibleTo: VisibleTo,
  audience: string[] | null | undefined
): Record<string, unknown> => ({
  ...(audience !== undefined && {
    [VISIBLE_TO]: withAudience(visibleTo, entryAlias(entry), audience)
  }),
  [DIRECTORIES]: [...entries, entry]
});

/** The folder an entry of `shares.directories` is about */
export const entryPath = (entry: string) =>
  entry
    .replace(/^[!-]/, "")
    .replace(/^\[[^\]]*\]/, "")
    .replace(/[/\\]+$/, "");

/** A path with its leading `from` swapped for `to`, when it starts with it */
const swapStart = (path: string, from: string, to: string) =>
  path.startsWith(from) ? `${to}${path.slice(from.length)}` : path;

/**
 * A share's contents as the text admins paste into reports: each directory, named by its local
 * path, then its files one a line, indented, sorted by name, without the share's remote prefix
 */
export const contentsText = (share: Schemas["Share"], directories: Schemas["Directory"][]) =>
  directories
    .map((dir) => {
      const files = dir.files
        .map((f) => f.filename)
        .toSorted()
        .map((name) => `\t${swapStart(name, `${share.remotePath}\\`, "")}`);
      return [swapStart(dir.name, share.remotePath, share.localPath), ...files, ""].join("\n");
    })
    .join("\n");

/** Who sees a share, in words: the groups that do, everyone for null, no one for none */
export const audienceText = (groups: string[] | null) =>
  groups === null ? "Everyone" : groups.length ? listText(groups) : "No one";
