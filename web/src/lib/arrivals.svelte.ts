import { SvelteSet } from "svelte/reactivity";

/** How long a row that just arrived is marked, which its entrance takes */
export const FRESH_MS = 600;

/**
 * Tells rows that arrived while a list was on screen from rows that were there all along, so a
 * new row can make an entrance while one that only scrolled into a virtualised list's window
 * doesn't. The first keys seen are the list as it opened and count as there all along
 */
export class Arrivals {
  #known = new Set<string>();
  #fresh = new SvelteSet<string>();
  #primed = false;

  /** Notes the list's keys now; call it with every new list */
  see(keys: Iterable<string>) {
    for (const key of keys) {
      if (this.#known.has(key)) continue;
      this.#known.add(key);
      if (!this.#primed) continue;
      this.#fresh.add(key);
      setTimeout(() => this.#fresh.delete(key), FRESH_MS);
    }
    this.#primed = true;
  }

  /** Whether the row arrived a moment ago */
  isFresh = (key: string) => this.#fresh.has(key);
}
