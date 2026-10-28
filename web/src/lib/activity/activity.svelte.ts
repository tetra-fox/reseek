import { api, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";
import { fields, stored } from "#lib/stored.svelte.js";

export type ActivityEntry = Schemas["ActivityEntry"];
export type ActivityType = Schemas["ActivityKind"]["type"];

/** The docked activity log's least height, in pixels */
export const DOCK_MIN = 120;
/** The docked activity log's default height, in pixels */
const DOCK_HEIGHT = 288;

/** The docked activity log's greatest height: most of the window, the screen above keeping some */
export const dockMax = () => Math.max(DOCK_MIN, Math.round(window.innerHeight * 0.6));

/** Whether the activity log is open, its height when docked and the kinds it hides, kept in this browser */
export const activityPrefs = stored("reseek.activity", (saved) => {
  const s = fields(saved);
  return {
    open: s.open === true,
    height: typeof s.height === "number" && s.height >= DOCK_MIN ? s.height : DOCK_HEIGHT,
    hidden: Array.isArray(s.hidden) ? s.hidden.filter((t) => typeof t === "string") : []
  };
});

/**
 * The activity log as this page holds it: read whole when the dock opens, then kept by the
 * `activity` topic's frames, an entry that grows (more files queued) patched in place. The topic
 * is followed only while the dock is open
 */
class Activity {
  readonly entries = new Keyed<ActivityEntry>((e) => String(e.id));
  /** entries the reader cleared from the view, by id; the daemon keeps them */
  clearedThrough = $state(0);
  error = $state("");
  #release: (() => void) | null = null;

  constructor() {
    live.on("activity.buffer", (all) => this.entries.reconcile(all));
    live.on("activity.entry", (entry) => void this.entries.upsert(entry));
  }

  /** the entries shown, newest first */
  get shown(): ActivityEntry[] {
    return this.entries.rows
      .filter((e) => e.id > this.clearedThrough)
      .toSorted((a, b) => b.id - a.id);
  }

  /** Follows the log while the dock is open; gives the function that stops following it */
  follow() {
    if (this.#release) return this.#release;
    const release = live.retain("activity");
    void this.load();
    this.#release = () => {
      release();
      this.#release = null;
    };
    return this.#release;
  }

  /** Reads the whole log, which the frames then keep */
  load = async () => {
    const outcome = await call(() => api.GET("/api/v0/activity"));
    if (!outcome.ok) return void (this.error = outcome.error);
    this.error = "";
    for (const entry of outcome.data) this.entries.upsert(entry);
  };

  /** Clears the view up to the newest entry; the log itself is the daemon's */
  clearView() {
    this.clearedThrough = Math.max(0, ...this.entries.rows.map((e) => e.id));
  }
}

export const activity = new Activity();
