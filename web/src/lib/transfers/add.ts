import type { EnqueueItem } from "#lib/files/downloads.js";
import { parseSlsk } from "#lib/files/slsk.js";

/** Files to download, by the user they come from */
export type Wanted = Map<string, EnqueueItem[]>;

const add = (wanted: Wanted, username: string, filename: string, size = 0) => {
  if (!username || !filename) return;
  const files = wanted.get(username) ?? [];
  if (!files.some((f) => f.filename === filename)) files.push({ filename, size });
  wanted.set(username, files);
};

/** One file or one search response as pasted JSON, in whatever of the two shapes it has */
const fromJson = (wanted: Wanted, value: unknown) => {
  if (Array.isArray(value)) {
    for (const v of value) fromJson(wanted, v);
    return;
  }
  if (!value || typeof value !== "object") return;
  const v = value as Record<string, unknown>;
  const username = typeof v.username === "string" ? v.username : "";
  const size = (n: unknown) => (typeof n === "number" && n >= 0 ? n : 0);
  if (typeof v.filename === "string") add(wanted, username, v.filename, size(v.size));
  for (const list of [v.files, v.lockedFiles]) {
    if (!Array.isArray(list)) continue;
    for (const f of list) {
      const file = (f ?? {}) as Record<string, unknown>;
      if (typeof file.filename === "string") add(wanted, username, file.filename, size(file.size));
    }
  }
};

/**
 * What the Add dialog's paste box names: `slsk://user/path/to/file` links, one per line, whose
 * slashes stand for the share's backslashes, or the JSON of files or search responses. Sizes
 * not given are 0, which the daemon takes as unknown
 */
export const parsePaste = (text: string): Wanted => {
  const wanted: Wanted = new Map();
  const trimmed = text.trim();
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
    try {
      fromJson(wanted, JSON.parse(trimmed));
    } catch {
      // not JSON after all: nothing is read from it
    }
    return wanted;
  }
  for (const line of trimmed.split(/\r?\n/)) {
    const link = parseSlsk(line);
    if (link && !link.folder) add(wanted, link.username, link.path);
  }
  return wanted;
};
