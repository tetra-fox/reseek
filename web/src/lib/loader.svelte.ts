import type { Outcome } from "./request";

/**
 * Data a region shows, from `fetch`. One load runs at a time: asking while one runs loads once
 * more after it, and each answer is taken as it comes, since it is newer than what is shown. A
 * failed load keeps the last data and sets `error`, so what was shown stays on screen. `kept` is
 * handed each answer as it becomes `data`, for rows patched in place rather than replaced
 */
export class Loader<T> {
  data = $state.raw<T | undefined>();
  error = $state("");
  /** until the first load is done */
  loading = $state(true);
  #fetch: () => Promise<Outcome<T>>;
  #kept: ((data: T) => void) | undefined;
  #running: Promise<void> | null = null;
  #again = false;
  /** counts the times the data was forgotten, so a load from before leaves it so */
  #cleared = 0;

  constructor(fetch: () => Promise<Outcome<T>>, kept?: (data: T) => void) {
    this.#fetch = fetch;
    this.#kept = kept;
  }

  /** Resolves once the data is current, after the load asked for while one ran too */
  load = (): Promise<void> => {
    if (this.#running) {
      this.#again = true;
      return this.#running;
    }
    this.#running = this.#run().finally(() => (this.#running = null));
    return this.#running;
  };

  /** Forgets the data, back to before the first load; a load on its way is not taken */
  clear() {
    this.#cleared += 1;
    this.data = undefined;
    this.error = "";
    this.loading = true;
  }

  async #run() {
    do {
      this.#again = false;
      const cleared = this.#cleared;
      const outcome = await this.#fetch();
      if (cleared !== this.#cleared) continue;
      if (outcome.ok) {
        this.#kept?.(outcome.data);
        this.data = outcome.data;
        this.error = "";
      } else {
        this.error = outcome.error;
      }
      this.loading = false;
    } while (this.#again);
  }
}
