import type { Schemas } from "#lib/api/client.js";

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
