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
 * The responses of one search: read once the page opens and whenever a stream starts following
 * the `search:<id>` topic, since the daemon streams a response only to a stream that follows its
 * search when it comes; appended as the topic streams them while the search runs; then replaced
 * once by the stored responses when it completes, which are the ones that count. Each response
 * keeps its key throughout, so its card stays where it is. Streamed responses are added at most
 * every `STREAM_MS`, since every change to `results` runs the page's filters and counts over all
 * of them
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
  /** the reads of a running search's responses started, and the last one whose answer was taken */
  #reads = 0;
  #caught = 0;

  constructor(id: string) {
    this.id = id;
    const stops = [
      live.on("search.response", ({ searchId, response }) => {
        if (searchId === this.id && !this.stored) this.add(response);
      }),
      live.on("hello", ({ topics }) => {
        if (topics.includes(`search:${this.id}`)) void this.catchUp();
      })
    ];
    this.#stop = () => {
      for (const stop of stops) stop();
    };
  }

  /**
   * Adds one streamed response at the end, with the others that come within `STREAM_MS`. A search
   * takes one response from each user, so one from a user already here came with a read
   */
  add(response: SearchResponse) {
    if (this.#keys.has(response.username)) return;
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
   * Reads the responses the running search took so far, which take the order the search took
   * them in, with the ones streamed since after them. A read answered after a later one, or
   * after the stored responses, is dropped
   */
  catchUp = async () => {
    if (this.stored) return;
    const turn = ++this.#reads;
    const outcome = await this.#get();
    // a failed read leaves what is shown; the next greeting or the stored responses read again
    if (!outcome.ok || this.stored || turn < this.#caught) return;
    this.#caught = turn;
    this.#flush();
    const read = new Set(outcome.data.map((response) => response.username));
    const keys = new ResultKeys();
    this.results = [
      ...outcome.data.map((response) => keys.next(readResponse(response))),
      ...this.results
        .filter((r) => !read.has(r.response.username))
        .map((r) => keys.next(r.response))
    ];
    this.#keys = keys;
  };

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
