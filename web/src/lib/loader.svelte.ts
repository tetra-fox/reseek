import type { Outcome } from "./request";

/**
 * Data a region shows, from `fetch`. One load runs at a time: asking while one runs loads once
 * more after it, and the answer of the load that was overtaken is dropped. A failed load keeps
 * the last data and sets `error`, so what was shown stays on screen
 */
export class Loader<T> {
  data = $state.raw<T | undefined>();
  error = $state("");
  /** until the first load is done */
  loading = $state(true);
  #fetch: () => Promise<Outcome<T>>;
  #running: Promise<void> | null = null;
  #again = false;

  constructor(fetch: () => Promise<Outcome<T>>) {
    this.#fetch = fetch;
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

  async #run() {
    do {
      this.#again = false;
      const outcome = await this.#fetch();
      if (this.#again) continue;
      if (outcome.ok) {
        this.data = outcome.data;
        this.error = "";
      } else {
        this.error = outcome.error;
      }
      this.loading = false;
    } while (this.#again);
  }
}
