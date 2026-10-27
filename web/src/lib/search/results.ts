import type { Schemas } from "#lib/api/client.js";
import { getDirectoryName } from "#lib/format.js";
import type { ResultSort } from "#lib/settings.svelte.js";

import { filterResponse, type Filters } from "./filters";

export type SearchResponse = Schemas["SearchResponse"];
export type SearchFile = Schemas["File"];

/** One response with the key its card is rendered by */
export type Result = { key: string; response: SearchResponse };

/**
 * Keys responses in the order they came: the username, then how many responses that user gave
 * before, since a peer can answer twice. The streamed responses and the stored ones come in the
 * same order, so a card keeps its key when the stored list replaces the streamed one
 */
export class ResultKeys {
  #seen = new Map<string, number>();

  next(response: SearchResponse): Result {
    const n = this.#seen.get(response.username) ?? 0;
    this.#seen.set(response.username, n + 1);
    return { key: `${n}:${response.username}`, response };
  }
}

export type ResultOptions = {
  /** users whose cards were dismissed, and users who are blocked unless shown */
  hidden: (username: string) => boolean;
  filters: Filters;
  hideLocked: boolean;
  hideNoFreeSlots: boolean;
  /** null keeps the order the responses came in, while a search still runs */
  sort: ResultSort | null;
};

/**
 * The results as the page shows them: hidden users dropped, locked files emptied when they are
 * hidden, the filter applied, responses left without files dropped, responses without a free
 * slot dropped when asked, then sorted, ties in the order they came
 */
export const pipeline = (results: Result[], options: ResultOptions): Result[] => {
  const shown: Result[] = [];
  for (const { key, response } of results) {
    if (options.hidden(response.username)) continue;
    const unlocked = options.hideLocked
      ? { ...response, lockedFiles: [], lockedFileCount: 0 }
      : response;
    const filtered = filterResponse(options.filters, unlocked);
    if (filtered.fileCount + filtered.lockedFileCount === 0) continue;
    if (options.hideNoFreeSlots && !filtered.hasFreeUploadSlot) continue;
    shown.push({ key, response: filtered });
  }
  if (options.sort === "uploadSpeed") {
    shown.sort((a, b) => b.response.uploadSpeed - a.response.uploadSpeed);
  } else if (options.sort === "queueLength") {
    shown.sort((a, b) => a.response.queueLength - b.response.queueLength);
  }
  return shown;
};

export type ResultFile = SearchFile & { locked: boolean };

/** One folder of a response: its path and its files, locked ones marked */
export type ResultFolder = { name: string; files: ResultFile[]; locked: boolean };

/**
 * A response's files by folder, in the order the folders first appear. A file listed twice is
 * one file
 */
export const folders = (response: SearchResponse): ResultFolder[] => {
  const byName = new Map<string, ResultFolder>();
  const seen = new Set<string>();
  const add = (file: SearchFile, locked: boolean) => {
    if (seen.has(file.filename)) return;
    seen.add(file.filename);
    const name = getDirectoryName(file.filename);
    const folder = byName.get(name) ?? { name, files: [], locked: false };
    byName.set(name, folder);
    folder.files.push({ ...file, locked });
    folder.locked ||= locked;
  };
  for (const file of response.files) add(file, false);
  for (const file of response.lockedFiles) add(file, true);
  return [...byName.values()];
};

/** The separator a remote path uses: `/` when it has one, else `\` */
export const separatorOf = (path: string) => (path.includes("/") ? "/" : "\\");
