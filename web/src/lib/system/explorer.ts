import type { Schemas } from "#lib/api/client.js";

import { joinPath } from "./localfiles";

/** A line of the file explorer: the way up, a directory or a file */
export type Entry = {
  /** the kind and the path, unique in the whole tree */
  key: string;
  kind: "up" | "directory" | "file";
  name: string;
  /** the path under the root */
  path: string;
  modifiedAt: string | null;
  size: number | null;
};

export type SortBy = "name" | "date";

const byName = (a: Entry, b: Entry) =>
  a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
const byDate = (a: Entry, b: Entry) =>
  (a.modifiedAt ? Date.parse(a.modifiedAt) : 0) - (b.modifiedAt ? Date.parse(b.modifiedAt) : 0);

/**
 * A listing's lines: `..` first inside a subdirectory, then the directories, then the files,
 * each part sorted by name or date
 */
export const entries = (
  listing: Schemas["FilesystemDirectory"],
  parts: string[],
  sortBy: SortBy,
  dir: "asc" | "desc"
): Entry[] => {
  const sign = dir === "asc" ? 1 : -1;
  const compare = sortBy === "name" ? byName : byDate;
  const sorted = (list: Entry[]) => list.toSorted((a, b) => sign * compare(a, b) || byName(a, b));
  const directories = (listing.directories ?? []).map((d): Entry => ({
    key: `d:${joinPath([...parts, d.name])}`,
    kind: "directory",
    name: d.name,
    path: joinPath([...parts, d.name]),
    modifiedAt: d.modifiedAt,
    size: null
  }));
  const files = (listing.files ?? []).map((f): Entry => ({
    key: `f:${joinPath([...parts, f.name])}`,
    kind: "file",
    name: f.name,
    path: joinPath([...parts, f.name]),
    modifiedAt: f.modifiedAt,
    size: f.length
  }));
  const up: Entry[] = parts.length
    ? [
        {
          key: "..",
          kind: "up",
          name: "..",
          path: joinPath(parts.slice(0, -1)),
          modifiedAt: null,
          size: null
        }
      ]
    : [];
  return [...up, ...sorted(directories), ...sorted(files)];
};
