import { api } from "#lib/api/client.js";
import { sleep } from "#lib/format.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

import { readResponse, type Result, ResultKeys, type SearchResponse } from "./results";

/** How long to wait before asking once more for the responses of a search that had some */
export const RETRY_EMPTY_MS = 1_000;

/** How long streamed responses gather before they are added to the results together */
export const STREAM_MS = 250;

/**
 * The responses of one search: appended as the `search:<id>` topic streams them while the
 * search runs, then replaced once by the stored responses when it completes, which are the
 * ones that count. Each response keeps its key across both, so its card stays where it is.
 * Streamed responses are added at most every `STREAM_MS`, since every change to `results` runs
 * the page's filters and counts over all of them
 */
export class Results {
  readonly id: string;
  results = $state.raw<Result[]>([]);
  /** the stored responses arrived */
  stored = $state(false);
  loading = $state(false);
  error = $state("");
  #keys = new ResultKeys();
  #stop: () => void;
  #waiting: Result[] = [];
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(id: string) {
    this.id = id;
    this.#stop = live.on("search.response", ({ searchId, response }) => {
      if (searchId === this.id && !this.stored) this.add(response);
    });
  }

  /** Adds one streamed response at the end, with the others that come within `STREAM_MS` */
  add(response: SearchResponse) {
    this.#waiting.push(this.#keys.next(readResponse(response)));
    this.#timer ??= setTimeout(this.#flush, STREAM_MS);
  }

  #flush = () => {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    if (!this.#waiting.length) return;
    this.results = [...this.results, ...this.#waiting];
    this.#waiting = [];
  };

  /** Drops the streamed responses not yet added, which the stored ones replace */
  #drop() {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#waiting = [];
  }

  /**
   * Fetches the stored responses once the search completed. The search says it is complete
   * only once they are stored, but an empty answer for a search that had responses is asked
   * again once, in case it came too early
   */
  fetch = async (responseCount: number) => {
    if (this.stored || this.loading) return;
    // the streamed responses show while the stored ones load, and stay if they fail to
    this.#flush();
    this.loading = true;
    let outcome = await this.#get();
    if (outcome.ok && outcome.data.length === 0 && responseCount > 0) {
      await sleep(RETRY_EMPTY_MS);
      outcome = await this.#get();
    }
    this.loading = false;
    if (!outcome.ok) {
      this.error = outcome.error;
      return;
    }
    this.#drop();
    const keys = new ResultKeys();
    this.results = outcome.data.map((response) => keys.next(readResponse(response)));
    this.#keys = keys;
    this.stored = true;
    this.error = "";
  };

  /** Stops following the stream */
  close() {
    this.#stop();
    this.#drop();
  }

  #get() {
    return call(() =>
      api.GET("/api/v0/searches/{id}/responses", { params: { path: { id: this.id } } })
    );
  }
}
