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
 * Files under the folder `name` by the path of their folder below it, `""` for those directly
 * in it, which come first
 */
const byFolder = (
  name: string,
  files: Schemas["File"][],
  separator: Separator
): [string, Schemas["File"][]][] => {
  const prefix = name + separator;
  const groups = new Map<string, Schemas["File"][]>();
  for (const file of files) {
    const relative = file.filename.startsWith(prefix)
      ? file.filename.slice(prefix.length)
      : file.filename;
    const group = relative.split(separator).filter(Boolean).slice(0, -1).join("/");
    const grouped = groups.get(group);
    if (grouped) grouped.push(file);
    else groups.set(group, [file]);
  }
  return [...groups.entries()].toSorted(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : 0));
};

/** Groups of files under the folder `name`, each sent to its folder's path from `name`'s parent */
const named = (
  name: string,
  groups: [string, Schemas["File"][]][],
  separator: Separator
): DownloadGroup[] => {
  const label = name.split(separator).filter(Boolean).pop() ?? "";
  return groups.map(([group, files]) => ({
    files,
    destination: [label, group].filter(Boolean).join("/") || null
  }));
};

/**
 * Files under the folder `name` split for downloading, each folder's to its path from `name`'s
 * parent with `/` between: those directly in `Music\Album` to `Album`, those in
 * `Music\Album\Disc 1\Extras` to `Album/Disc 1/Extras`. Every destination is written, so a
 * folder lands the same whatever the download pattern, which the daemon does not apply to a
 * written one
 */
export const downloadFolderPlan = (
  name: string,
  files: Schemas["File"][],
  separator: Separator
): DownloadGroup[] => named(name, byFolder(name, files, separator), separator);

/**
 * Selected files split into the groups that download them. The files under each top folder,
 * which `topOf` gives a path's, are split from the deepest folder holding all of them. When
 * anything in the selection is below such a folder, every group is named as
 * `downloadFolderPlan` names it, so the selection keeps its folders and lands in one tree; when
 * nothing is, every file goes where the download pattern puts it
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
  const tops = [...byTop.values()].map((grouped) => {
    const name = commonFolder(
      grouped.map((f) => f.filename),
      separator
    );
    return { name, groups: byFolder(name, grouped, separator) };
  });
  const nested = tops.some(({ groups }) => groups.some(([group]) => group !== ""));
  return tops.flatMap(({ name, groups }) =>
    nested
      ? named(name, groups, separator)
      : groups.map(([, grouped]) => ({ files: grouped, destination: null }))
  );
};

/**
 * Enqueues the groups of a plan for one user's files as one download job, found by the search
 * `searchId` when they came from one
 */
export const enqueuePlan = (
  username: string,
  plan: DownloadGroup[],
  searchId?: string
): Promise<Enqueued> =>
  enqueueJob(
    plan.map((group) => ({
      username,
      files: group.files.map((f) => asItem(f)),
      searchId,
      destination: group.destination ?? undefined
    }))
  );
