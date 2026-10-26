import { SvelteMap, SvelteSet } from "svelte/reactivity";

import { interaction } from "./interaction.svelte";

/** Whether two values from the API are the same, looking inside arrays and objects */
export const same = (a: unknown, b: unknown): boolean => {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return ka.every(
    (k) => k in b && same((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k])
  );
};

/** Writes onto `target` only the fields of `source` that differ, so unchanged fields don't re-render */
export const patch = <T extends object>(target: T, source: T) => {
  for (const key of Object.keys(source) as (keyof T)[]) {
    if (!same(target[key], source[key])) target[key] = source[key];
  }
  for (const key of Object.keys(target) as (keyof T)[]) {
    if (!(key in source)) delete target[key];
  }
};

/**
 * Rows kept by a stable id, for lists rendered with `{#each list.rows as row (key)}`. A row that
 * changes is patched in place: the same object, with only the fields that differ written, so its
 * DOM nodes stay and only the changed text updates. A push frame upserts one row; a poll's whole
 * answer goes through `reconcile`. While the user interacts with a list (`interaction.frozen`),
 * removals wait, marked in `removed`, and happen when it ends
 */
export class Keyed<T extends object> {
  #rows = new SvelteMap<string, T>();
  #key: (row: T) => string;
  /** rows that are gone but stay until the interaction ends */
  removed = new SvelteSet<string>();
  /** the rows in the order they were first added */
  rows = $derived([...this.#rows.values()]);

  constructor(key: (row: T) => string) {
    this.#key = key;
  }

  get size() {
    return this.#rows.size;
  }

  get(key: string): T | undefined {
    return this.#rows.get(key);
  }

  /** Adds a row, or patches the one with its key; gives back the row the list keeps */
  upsert(row: T): T {
    const key = this.#key(row);
    const kept = this.#rows.get(key);
    if (kept) {
      patch(kept, row);
      this.removed.delete(key);
      return kept;
    }
    const fresh = $state(row);
    this.#rows.set(key, fresh);
    return fresh;
  }

  /** Removes a row, or marks it removed until the interaction ends */
  remove(key: string) {
    if (!this.#rows.has(key)) return;
    if (!interaction.frozen) {
      this.#rows.delete(key);
      return;
    }
    this.removed.add(key);
    interaction.whenThawed(() => {
      if (!this.removed.has(key)) return;
      this.removed.delete(key);
      this.#rows.delete(key);
    });
  }

  /** Makes the list hold exactly `rows`: patches what changed, adds what is new, removes the rest */
  reconcile(rows: T[]) {
    const keep = new Set<string>();
    for (const row of rows) keep.add(this.#key(this.upsert(row)));
    for (const key of this.#rows.keys()) if (!keep.has(key)) this.remove(key);
  }

  clear() {
    this.#rows.clear();
    this.removed.clear();
  }
}
