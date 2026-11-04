import { api } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

import { type Job, names, type Target } from "./analysis";

/** How often the queue is read while something runs in it, and while it is idle */
const BUSY_MS = 1000;
const IDLE_MS = 5000;

/**
 * The analysis queue, read while a screen that shows analyses is open: often while files wait or
 * run, seldom otherwise, and at once when an analysis finishes
 */
class Jobs {
  list = $state.raw<Job[]>([]);
  /** how many files of batches wait for room in the queue, which they join as it drains */
  backlog = $state(0);
  /** bumped whenever an analysis finishes, for views to read their report again */
  finished = $state(0);
  #holders = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #stops: (() => void)[] = [];

  /** Keeps the queue current while the returned function has not been called */
  retain(): () => void {
    if (this.#holders++ === 0) {
      this.#stops = [
        live.retain("transfers"),
        live.on("files.analysis", () => {
          this.finished++;
          void this.refresh();
        })
      ];
      void this.refresh();
    }
    return () => {
      if (--this.#holders > 0) return;
      clearTimeout(this.#timer);
      for (const stop of this.#stops) stop();
      this.#stops = [];
    };
  }

  /** Reads the queue now, and again after a while */
  async refresh() {
    clearTimeout(this.#timer);
    const outcome = await call(() => api.GET("/api/v0/analysis/jobs"));
    if (outcome.ok) {
      this.list = outcome.data.jobs;
      this.backlog = outcome.data.backlog;
    }
    if (this.#holders > 0) {
      this.#timer = setTimeout(() => void this.refresh(), this.count ? BUSY_MS : IDLE_MS);
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
