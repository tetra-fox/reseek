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
  /**
   * the remote folder the files were picked in, which the daemon keeps their folders below,
   * wherever the download pattern puts the folder
   */
  folder?: string;
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
        groups: requests.map(({ username, files, searchId, folder }) => ({
          username,
          files,
          searchId: searchId ?? null,
          folder: folder || null
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
  /** the remote folder the files were picked in, null when they are in none */
  folder: string | null;
};

/** What a path below the folder `name` starts with: the folder and a separator, a root as it is */
const prefixOf = (name: string, separator: Separator) =>
  name.endsWith(separator) ? name : name + separator;

/**
 * The folder `name` when every file is below it, else null: a file at a peer's root, such as
 * `\x.flac`, is in no folder below the root
 */
const folderOf = (name: string, files: Schemas["File"][], separator: Separator) => {
  const prefix = prefixOf(name, separator);
  const below = (path: string) => path.length > prefix.length && path.startsWith(prefix);
  return name && files.every((f) => below(f.filename)) ? name : null;
};

/**
 * Files under the folder `name` in the order they download: those directly in it first, then
 * each folder's below it together
 */
const inOrder = (
  name: string,
  files: Schemas["File"][],
  separator: Separator
): Schemas["File"][] => {
  const prefix = prefixOf(name, separator);
  const groups = new Map<string, Schemas["File"][]>();
  for (const file of files) {
    const relative = file.filename.startsWith(prefix)
      ? file.filename.slice(prefix.length)
      : file.filename;
    const group = relative.split(separator).filter(Boolean).slice(0, -1).join(separator);
    const grouped = groups.get(group);
    if (grouped) grouped.push(file);
    else groups.set(group, [file]);
  }
  return [...groups.entries()]
    .toSorted(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : 0))
    .flatMap(([, grouped]) => grouped);
};

/**
 * Files under the folder `name` as one group in it. The daemon puts each file's folders from
 * `name` down where the download pattern names the file's folder: `${SOURCE_DIRECTORY}` gives
 * `Album/Disc 1` for `Music\Album\Disc 1\01.flac` in `Music\Album`, so a folder keeps its tree
 */
export const downloadFolderPlan = (
  name: string,
  files: Schemas["File"][],
  separator: Separator
): DownloadGroup[] => [
  { files: inOrder(name, files, separator), folder: folderOf(name, files, separator) }
];

/**
 * Selected files as the groups that download them, one per top folder, which `topOf` gives a
 * path's. When anything in the selection is in a folder below the deepest one holding its top's
 * files, or that folder is one `whole` says was ticked whole, folders were picked, and every
 * group is sent in that deepest folder so the selection keeps its tree, as `downloadFolderPlan`
 * keeps a folder's. When neither is, the files are loose and each goes where the download pattern
 * puts it
 */
export const downloadPlan = (
  files: Schemas["File"][],
  topOf: (path: string) => string,
  separator: Separator,
  whole: (folder: string) => boolean = () => false
): DownloadGroup[] => {
  const byTop = new Map<string, Schemas["File"][]>();
  for (const file of files) {
    const top = topOf(file.filename);
    const listed = byTop.get(top);
    if (listed) listed.push(file);
    else byTop.set(top, [file]);
  }
  const tops = [...byTop.values()].map((grouped) => ({
    name: commonFolder(
      grouped.map((f) => f.filename),
      separator
    ),
    grouped
  }));
  const picked = ({ name, grouped }: (typeof tops)[number]) => {
    const prefix = prefixOf(name, separator);
    return (
      name !== "" &&
      (whole(name) ||
        grouped.some(
          (f) =>
            f.filename.startsWith(prefix) && f.filename.slice(prefix.length).includes(separator)
        ))
    );
  };
  const inFolders = tops.some(picked);
  return tops.map(({ name, grouped }) => ({
    files: inOrder(name, grouped, separator),
    folder: inFolders ? folderOf(name, grouped, separator) : null
  }));
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
      folder: group.folder ?? undefined
    }))
  );

/**
 * Enqueues loose files of several users, such as search results, as one download job, a group
 * per user: each file goes where the download pattern puts it
 */
export const enqueueSelection = (
  picked: { username: string; file: Schemas["File"] }[],
  searchId?: string
): Promise<Enqueued> => {
  const byUser = new Map<string, Schemas["File"][]>();
  for (const { username, file } of picked) {
    const listed = byUser.get(username);
    if (listed) listed.push(file);
    else byUser.set(username, [file]);
  }
  return enqueueJob(
    [...byUser].map(([username, files]) => ({
      username,
      files: files.map((f) => asItem(f)),
      searchId
    }))
  );
};
