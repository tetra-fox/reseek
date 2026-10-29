import { SvelteSet } from "svelte/reactivity";

import { api, type Schemas } from "#lib/api/client.js";
import { plural } from "#lib/format.js";
import { getDirectoryName } from "#lib/format.js";
import { keepCurrent } from "#lib/live/keep.svelte.js";
import { call } from "#lib/request.js";
import { downloads, fileKey } from "#lib/transfers/transfers.svelte.js";
import { parentOf, type Separator } from "#lib/tree/folders.js";

export type EnqueueItem = Schemas["EnqueueDownloadBatchItem"];

/** What an enqueue came to, in words for the status next to the button and its toast */
export type Enqueued =
  | { kind: "done"; text: string }
  | { kind: "partial" | "none" | "failed"; text: string };

type Request = {
  username: string;
  files: EnqueueItem[];
  /** the search the files were found by */
  searchId?: string;
  /** a relative folder under the downloads directory that every file goes to */
  destination?: string;
};

/** How long a file asked for stays marked as starting while its download hasn't shown yet */
const STARTING_MS = 10_000;

/**
 * The files asked for whose downloads haven't shown yet, by `fileKey`, so a file's row shows its
 * download at once
 */
export const starting = new SvelteSet<string>();

/** The download of one user's file, the newest when there were several */
export const downloadOf = (username: string, path: string) =>
  downloads.byFile.get(fileKey(username, path));

/**
 * Keeps the downloads current while a screen that lists remote files is mounted, so each file's
 * row shows how its download is going. Call it while the component is being set up
 */
export const followDownloads = () => {
  keepCurrent(
    () => void downloads.load(),
    () => "transfers"
  );
  if (!downloads.loaded) void downloads.load();
};

/**
 * Enqueues files from one user as one batch. The batch exists even when files were refused, so
 * the outcome is read from its failures: none, some or every file
 */
export const enqueue = async ({
  username,
  files,
  searchId,
  destination
}: Request): Promise<Enqueued> => {
  const keys = files.map((f) => fileKey(username, f.filename));
  for (const key of keys) starting.add(key);
  const outcome = await call(() =>
    api.POST("/api/v0/transfers/downloads/batches", {
      body: {
        username,
        files,
        searchId: searchId ?? null,
        options: destination ? { destination } : undefined
      }
    })
  );
  const refused = new Set(
    outcome.ok ? outcome.data.failures.map((f) => fileKey(username, f.filename)) : keys
  );
  for (const key of keys) {
    if (refused.has(key)) starting.delete(key);
    else setTimeout(() => starting.delete(key), STARTING_MS);
  }
  if (!outcome.ok) {
    const text = outcome.status === 404 ? `${username} appears to be offline` : outcome.error;
    return { kind: "failed", text };
  }
  const { failures } = outcome.data;
  const why = failures.map((f) => `${f.filename}: ${f.message}`).join("; ");
  if (failures.length === 0) {
    return {
      kind: "done",
      text: `${plural(files.length, "file", "files")} queued`
    };
  }
  if (failures.length >= files.length)
    return { kind: "none", text: `Could not queue any files: ${why}` };
  return {
    kind: "partial",
    text: `Queued ${files.length - failures.length} of ${plural(files.length, "file", "files")}. Could not queue ${why}`
  };
};

/** The attributes of a file in a listing, as an enqueue keeps them to show with the download */
export const asItem = (file: Schemas["File"], filename = file.filename): EnqueueItem => ({
  filename,
  size: file.size,
  attributes: file.attributes
});

export type DownloadGroup = {
  files: Schemas["File"][];
  /** where the files go under the downloads directory, null for where the pattern puts them */
  destination: string | null;
};

/**
 * Selected files of the folder `name` split for downloading: those directly in it go where the
 * download pattern puts them, and those in each of its subfolders go to that subfolder's path
 * from the folder's parent, with `/` between, so `Music\Album\Disc 1` lands in `Album/Disc 1`
 */
const downloadGroups = (
  name: string,
  files: Schemas["File"][],
  separator: Separator
): DownloadGroup[] => {
  const parent = name.split(separator).slice(0, -1).join(separator);
  const prefix = name + separator;
  const groups = new Map<string, Schemas["File"][]>();
  for (const file of files) {
    const relative = file.filename.startsWith(prefix)
      ? file.filename.slice(prefix.length)
      : file.filename;
    const parts = relative.split(separator).filter(Boolean);
    const group = parts.length > 1 ? parts[0] : "";
    const grouped = groups.get(group);
    if (grouped) grouped.push(file);
    else groups.set(group, [file]);
  }
  return [...groups.entries()]
    .toSorted(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : 0))
    .map(([group, grouped]) => ({
      files: grouped,
      destination:
        group === ""
          ? null
          : (prefix + group)
              .slice(parent.length > 0 ? parent.length + 1 : 0)
              .split(separator)
              .join("/")
    }));
};

/** The deepest folder that holds every one of the files, by their paths */
const commonFolder = (files: Schemas["File"][], separator: Separator) => {
  let common = getDirectoryName(files[0].filename);
  for (const file of files) {
    const folder = getDirectoryName(file.filename);
    while (common && folder !== common && !folder.startsWith(common + separator)) {
      common = parentOf(common, separator);
    }
  }
  return common;
};

/**
 * Selected files split into the batches that download them. The files under each top folder,
 * which `topOf` gives a path's, are split from the deepest folder holding all of them: those
 * directly in it go where the download pattern puts them, and each subfolder's go to that
 * subfolder's path from the folder's parent, so a selection keeps its folders
 */
export const downloadPlan = (
  files: Schemas["File"][],
  topOf: (path: string) => string,
  separator: Separator
): DownloadGroup[] => {
  const byTop = new Map<string, Schemas["File"][]>();
  for (const file of files) {
    const top = topOf(file.filename);
    const listed = byTop.get(top);
    if (listed) listed.push(file);
    else byTop.set(top, [file]);
  }
  return [...byTop.values()].flatMap((grouped) =>
    downloadGroups(commonFolder(grouped, separator), grouped, separator)
  );
};
