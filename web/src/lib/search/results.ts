import type { Schemas } from "#lib/api/client.js";
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
  /** users the directory last saw offline */
  offline?: (username: string) => boolean;
  /** the longest queue a user may have and stay; none when null */
  maxQueue?: number | null;
  /** null keeps the order the responses came in, while a search still runs */
  sort: ResultSort | null;
};

/**
 * The results as the page shows them: hidden users dropped, locked files emptied when they are
 * hidden, the filter applied, responses left without files dropped, responses without a free
 * slot, from users seen offline or with a long queue dropped when asked, then sorted, ties in the
 * order they came
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
    if (options.offline?.(response.username)) continue;
    if (options.maxQueue !== null && options.maxQueue !== undefined) {
      if (filtered.queueLength > options.maxQueue) continue;
    }
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

/** A response's files and then its locked ones, marked; a file listed twice is one file */
export const resultFiles = (response: SearchResponse): ResultFile[] => {
  const seen = new Set<string>();
  const files: ResultFile[] = [];
  const add = (file: SearchFile, locked: boolean) => {
    if (seen.has(file.filename)) return;
    seen.add(file.filename);
    files.push({ ...file, locked });
  };
  for (const file of response.files) add(file, false);
  for (const file of response.lockedFiles) add(file, true);
  return files;
};

/** The separator a remote path uses: `/` when it has one, else `\` */
export const separatorOf = (path: string) => (path.includes("/") ? "/" : "\\");
