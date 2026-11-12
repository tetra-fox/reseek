import { api } from "#lib/api/client.js";
import { Keyed } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";
import { stored } from "#lib/stored.svelte.js";

import { type Job, ordered, type Schedule } from "./jobs";

/** When the Jobs page was last open in this browser, which clears the bottom bar's failure mark */
export const jobsSeen = stored<{ at: string | null }>("reseek.jobs.seen", (saved) => {
  const at = (saved as { at?: unknown } | null)?.at;
  return { at: typeof at === "string" ? at : null };
});

/**
 * The jobs and the schedules, read while a screen shows them and patched in place by the
 * `jobs.*` frames, which the app follows on every screen
 */
class Jobs {
  rows = new Keyed<Job>((j) => j.id);
  schedules = $state.raw<Schedule[]>([]);
  loaded = $state(false);
  error = $state<string | null>(null);
  /** why the last read of the schedules failed; the card says it while it has none to show */
  schedulesError = $state<string | null>(null);
  /** counts the schedules taken from frames, which are newer than a read already on its way */
  #scheduleFrames = 0;

  constructor() {
    live.on("jobs.updated", (job) => void this.rows.upsert(job));
    live.on("jobs.removed", ({ ids }) => {
      for (const id of ids) this.rows.remove(id);
    });
    live.on("jobs.schedules", (schedules) => {
      this.#scheduleFrames += 1;
      this.schedules = schedules;
      this.schedulesError = null;
    });
  }

  /** Every job in the list's order */
  get list(): Job[] {
    return ordered(this.rows.rows);
  }

  /** Reads the jobs and the schedules again */
  async load() {
    const turn = this.rows.reading();
    const frames = this.#scheduleFrames;
    const [jobs, schedules] = await Promise.all([
      call(() => api.GET("/api/v0/jobs")),
      call(() => api.GET("/api/v0/jobs/schedules"))
    ]);
    // a later read, or the end of the session, came meanwhile
    if (!this.rows.latest(turn)) return;
    if (jobs.ok) {
      this.rows.reconcile(jobs.data.jobs, turn);
      this.error = null;
    } else {
      this.rows.abandon(turn);
      this.error = jobs.error;
    }
    // schedules a frame brought while they were read are newer than the read's
    if (frames === this.#scheduleFrames) {
      if (schedules.ok) {
        this.schedules = schedules.data;
        this.schedulesError = null;
      } else {
        // the schedules shown before stay, kept current by the frames
        this.schedulesError = schedules.error;
      }
    }
    this.loaded = true;
  }

  /** Forgets the jobs, as the session ends; a read on its way is not taken */
  clear() {
    this.rows.clear();
    this.#scheduleFrames += 1;
    this.schedules = [];
    this.loaded = false;
    this.error = null;
    this.schedulesError = null;
  }

  /** One of a job's controls; the frames that follow bring its new state */
  control(id: string, what: "cancel" | "pause" | "resume"): Promise<Outcome<unknown>> {
    const path = { params: { path: { id } } };
    return call(() =>
      what === "cancel"
        ? api.POST("/api/v0/jobs/{id}/cancel", path)
        : what === "pause"
          ? api.POST("/api/v0/jobs/{id}/pause", path)
          : api.POST("/api/v0/jobs/{id}/resume", path)
    );
  }

  runAgain(id: string) {
    return call(() => api.POST("/api/v0/jobs/{id}/run-again", { params: { path: { id } } }));
  }

  remove(id: string) {
    return call(() => api.DELETE("/api/v0/jobs/{id}", { params: { path: { id } } }));
  }

  clearEnded() {
    return call(() => api.DELETE("/api/v0/jobs"));
  }

  runSchedule(name: string) {
    return call(() =>
      api.POST("/api/v0/jobs/schedules/{name}/run", { params: { path: { name } } })
    );
  }

  /** A job with its parts */
  detail(id: string) {
    return call(() => api.GET("/api/v0/jobs/{id}", { params: { path: { id } } }));
  }
}

export const jobs = new Jobs();
