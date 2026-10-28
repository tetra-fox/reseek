import { live } from "#lib/live/live.svelte.js";

import { ERRORED, type Direction, type Transfer } from "./states";

type Counts = { succeeded: number; failed: number };

const DIRECTIONS = { Download: "download", Upload: "upload" } as const;

/**
 * What finished and what failed in each direction while its screen was not open, for the
 * navigation's badges: transfers that reach `Completed, Succeeded` or a failure on the
 * `transfers` topic, each transfer counted once per outcome. Opening the screen clears its counts
 */
class Unseen {
  #counts = $state<Record<Direction, Counts>>({
    download: { succeeded: 0, failed: 0 },
    upload: { succeeded: 0, failed: 0 }
  });
  #viewing: Record<Direction, number> = { download: 0, upload: 0 };
  #counted = new Set<string>();

  constructor() {
    live.on("transfers.updated", (t) => this.#updated(t));
    live.on("transfers.removed", (t) => {
      this.#counted.delete(`${t.id}:succeeded`);
      this.#counted.delete(`${t.id}:failed`);
    });
  }

  /** What finished and failed since the screen was last open */
  of(direction: Direction): Counts {
    return this.#counts[direction];
  }

  /**
   * The screen is open from now until the returned function is called: what was counted is seen,
   * and what finishes meanwhile is seen as it happens
   */
  view(direction: Direction): () => void {
    this.#viewing[direction] += 1;
    this.#counts[direction] = { succeeded: 0, failed: 0 };
    return () => {
      this.#viewing[direction] -= 1;
    };
  }

  #updated(t: Transfer) {
    const outcome =
      t.state === "Completed, Succeeded"
        ? "succeeded"
        : ERRORED.includes(t.state)
          ? "failed"
          : null;
    const direction = DIRECTIONS[t.direction as keyof typeof DIRECTIONS];
    if (!outcome || !direction) return;
    const key = `${t.id}:${outcome}`;
    if (this.#counted.has(key)) return;
    this.#counted.add(key);
    if (this.#viewing[direction]) return;
    this.#counts[direction][outcome] += 1;
  }
}

export const unseen = new Unseen();
