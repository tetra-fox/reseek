import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";

import type { Direction } from "./states";

type Counts = { succeeded: number; failed: number };

const KEY = { download: "downloads", upload: "uploads" } as const;

/**
 * What finished and what failed in each direction since its screen was last open, from the
 * daemon's running totals, for the navigation's badges. Totals from before the page loaded count
 * as seen
 */
class Unseen {
  #seen = $state<Record<Direction, Counts | null>>({ download: null, upload: null });
  #now = $state.raw<Schemas["TransferMetrics"] | null>(null);

  constructor() {
    live.on("metrics.update", (metrics) => {
      this.#now = metrics;
      for (const direction of ["download", "upload"] as const) {
        const seen = this.#seen[direction];
        const now = metrics[KEY[direction]].completed;
        // a daemon that restarted counts from zero again
        if (!seen || now.succeeded < seen.succeeded || now.failed < seen.failed) {
          this.#seen[direction] = { succeeded: now.succeeded, failed: now.failed };
        }
      }
    });
  }

  /** What finished and failed since the screen was last open */
  of(direction: Direction): Counts {
    const seen = this.#seen[direction];
    const now = this.#now?.[KEY[direction]].completed;
    if (!seen || !now) return { succeeded: 0, failed: 0 };
    return { succeeded: now.succeeded - seen.succeeded, failed: now.failed - seen.failed };
  }

  /** The screen is open: everything up to now is seen */
  seen(direction: Direction) {
    const now = this.#now?.[KEY[direction]].completed;
    if (now) this.#seen[direction] = { succeeded: now.succeeded, failed: now.failed };
  }
}

export const unseen = new Unseen();
