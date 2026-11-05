import { SvelteSet } from "svelte/reactivity";

import { api, type Schemas } from "#lib/api/client.js";
import { plural } from "#lib/format.js";
import { keepCurrent } from "#lib/live/keep.svelte.js";
import { call } from "#lib/request.js";
import { downloads, fileKey } from "#lib/transfers/transfers.svelte.js";
import { commonFolder, type Separator } from "#lib/tree/folders.js";

export type EnqueueItem = Schemas["EnqueueDownloadFile"];

/** What an enqueue came to, in words for the status next to the button and its toast */
export type Enqueued =
  | { kind: "done"; text: string }
  | { kind: "partial" | "none" | "failed"; text: string };

/** Files from one user to download together, one group of a download job */
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
 * Enqueues everything picked in one go as one download job, a group per request, which the
 * daemon tells of once when all of it has finished. The outcome is read from the files each
 * group refused and from the groups refused whole
 */
export const enqueueJob = async (requests: Request[]): Promise<Enqueued> => {
  const keys = requests.flatMap(({ username, files }) =>
    files.map((f) => fileKey(username, f.filename))
  );
  for (const key of keys) starting.add(key);
  const outcome = await call(() =>
    api.POST("/api/v0/transfers/downloads/jobs", {
      body: {
        groups: requests.map(({ username, files, searchId, destination }) => ({
          username,
          files,
          searchId: searchId ?? null,
          destination: destination || null
        }))
      }
    })
  );
  const answers = outcome.ok ? outcome.data.groups : [];
  const refused = new Set<string>();
  const why: string[] = [];
  // a group refused whole, its user offline say, has the same reason for each of its files
  const wholly: string[] = [];
  requests.forEach(({ username, files }, i) => {
    const answer = answers[i];
    if (!answer || answer.error) {
      for (const f of files) refused.add(fileKey(username, f.filename));
      if (!answer?.error) return;
      wholly.push(answer.error.error);
      why.push(`${plural(files.length, "file", "files")} from ${username}: ${answer.error.error}`);
      return;
    }
    for (const f of answer.failures) {
      refused.add(fileKey(username, f.filename));
      why.push(`${f.filename}: ${f.message}`);
    }
  });
  for (const key of keys) {
    if (refused.has(key)) starting.delete(key);
    else setTimeout(() => starting.delete(key), STARTING_MS);
  }
  if (!outcome.ok) return { kind: "failed", text: outcome.error };
  const total = keys.length;
  const reasons = [...new Set(why)].join("; ");
  if (answers.every((a) => a.error))
    return { kind: "failed", text: [...new Set(wholly)].join("; ") };
  if (refused.size === 0) return { kind: "done", text: `${plural(total, "file", "files")} queued` };
  if (refused.size >= total) return { kind: "none", text: `Could not queue any files: ${reasons}` };
  return {
    kind: "partial",
    text: `Queued ${total - refused.size} of ${plural(total, "file", "files")}. Could not queue ${reasons}`
  };
};

/** Enqueues files from one user as a download job of one group */
export const enqueue = (request: Request): Promise<Enqueued> => enqueueJob([request]);

/** A file in a listing as an enqueue asks for it */
export const asItem = (file: Schemas["File"], filename = file.filename): EnqueueItem => ({
  filename,
  size: file.size
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

/**
 * Selected files split into the groups that download them. The files under each top folder,
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
    downloadGroups(
      commonFolder(
        grouped.map((f) => f.filename),
        separator
      ),
      grouped,
      separator
    )
  );
};
