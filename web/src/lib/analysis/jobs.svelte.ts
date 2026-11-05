import { api } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

import { type Job, names, type Target } from "./analysis";

/** How often the queue is read while files wait or run in it */
const BUSY_MS = 1000;

/**
 * The analysis queue, kept while a screen that shows analyses is open: read every second while
 * files wait or run, and at once when an analysis finishes. While nothing does, it is read again
 * when the event stream says a file was queued, or on the stream's polling timer while it is down
 */
class Jobs {
  list = $state.raw<Job[]>([]);
  /** how many files of batches wait for room in the queue, which they join as it drains */
  backlog = $state(0);
  /** bumped whenever an analysis finishes, for views to read their report again */
  finished = $state(0);
  #holders = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #reading = false;
  #stops: (() => void)[] = [];

  /** Keeps the queue current while the returned function has not been called */
  retain(): () => void {
    if (this.#holders++ === 0) {
      this.#stops = [
        live.retain("transfers"),
        live.on("files.analysis", () => {
          this.finished++;
          void this.refresh();
        }),
        live.on("files.analysis_queued", (job) => {
          if (!this.list.some((j) => j.id === job.id)) this.list = [...this.list, job];
          // a batch queues many at once: one read after a while covers them all
          this.#timer ??= setTimeout(() => void this.refresh(), BUSY_MS);
        }),
        live.on("lagged", () => void this.refresh()),
        // a stream that resumed to follow transfers carries no job change from before its resume point
        live.onAdded((topics) => {
          if (topics.includes("transfers")) void this.refresh();
        }),
        // the stream went down while nothing waited: polling starts, as no frame will come
        $effect.root(() => {
          $effect(() => {
            if (live.pollMs !== null && !this.#reading && this.#timer === undefined) {
              void this.refresh();
            }
          });
        })
      ];
      void this.refresh();
    }
    return () => {
      if (--this.#holders > 0) return;
      clearTimeout(this.#timer);
      this.#timer = undefined;
      for (const stop of this.#stops) stop();
      this.#stops = [];
    };
  }

  /** Reads the queue now, and again after a while */
  async refresh() {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    this.#reading = true;
    const outcome = await call(() => api.GET("/api/v0/analysis/jobs"));
    this.#reading = false;
    if (outcome.ok) {
      this.list = outcome.data.jobs;
      this.backlog = outcome.data.backlog;
    }
    // a queued frame that arrived during the read set a timer, which this one replaces
    clearTimeout(this.#timer);
    this.#timer = undefined;
    const wait = this.count ? BUSY_MS : live.pollMs;
    if (this.#holders > 0 && wait !== null) {
      this.#timer = setTimeout(() => void this.refresh(), wait);
    }
  }

  /** The job of the file a target names, if it waits or runs */
  of(target: Target): Job | undefined {
    return this.list.find((j) => names(target, j));
  }

  async cancel(id: string) {
    const outcome = await call(() =>
      api.DELETE("/api/v0/analysis/jobs/{id}", { params: { path: { id } } })
    );
    void this.refresh();
    return outcome;
  }

  /** Takes every waiting file off the queue, with the backlog; the running ones go on */
  async cancelWaiting() {
    const outcome = await call(() => api.DELETE("/api/v0/analysis/jobs"));
    void this.refresh();
    return outcome;
  }

  /** How many files wait or run, the backlog with them */
  get count() {
    return this.list.length + this.backlog;
  }
}

export const jobs = new Jobs();
