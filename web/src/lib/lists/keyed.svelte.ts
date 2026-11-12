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
 * answer goes through `reconcile`. A reload takes a turn from `reading` before it asks and hands
 * it to `reconcile` with the answer, or to `abandon`: the answer may have been read before frames
 * that arrive while it is on its way, so the rows those frames changed keep their change, and an
 * answer overtaken by a later reload is not taken. A read of one row works the same way with
 * `readingRow` and `takeRow` or `assignRow`. While the user interacts with a list
 * (`interaction.frozen`), removals wait, marked in `removed`, and happen when it ends
 */
export class Keyed<T extends object> {
  #rows = new SvelteMap<string, T>();
  #key: (row: T) => string;
  /** rows that are gone but stay until the interaction ends */
  removed = new SvelteSet<string>();
  /** the rows in the order they were first added */
  rows = $derived([...this.#rows.values()]);
  /** the turn of the last reload that began */
  #turn = 0;
  /** the keys added, patched or removed since that reload began, until its answer is taken */
  #changed: Set<string> | null = null;
  /** counts the reads of one row begun and the changes made while any is under way */
  #rowTurn = 0;
  /** reads of one row under way */
  #rowReads = 0;
  /** while a read of one row is under way: the turn each key last changed at */
  #touched = new Map<string, number>();
  /** the last turn begun before the list was emptied, whose reads are older than it */
  #cleared = 0;

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
    this.#touch(key);
    return this.#put(key, row);
  }

  #put(key: string, row: T): T {
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

  /** Writes `fields` onto the row with `key`, if the list has it, as a frame changing part of it */
  assign(key: string, fields: Partial<T>) {
    const row = this.#rows.get(key);
    if (!row) return;
    this.#touch(key);
    Object.assign(row, fields);
  }

  /** Removes a row, or marks it removed until the interaction ends */
  remove(key: string) {
    this.#touch(key);
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

  /** A change to the row with `key`, which an answer read before it must not undo */
  #touch(key: string) {
    this.#changed?.add(key);
    if (this.#rowReads) this.#touched.set(key, ++this.#rowTurn);
  }

  /** Begins a read of one row and gives back its turn, for `takeRow` or `assignRow`, or `dropRow` */
  readingRow(): number {
    this.#rowReads += 1;
    return ++this.#rowTurn;
  }

  /**
   * Ends the read of one row with `turn`; gives back whether its answer for `key` is the newest:
   * no change to that row since it began, and no answer of a read begun after it taken
   */
  #answered(key: string, turn: number): boolean {
    const newest = turn > this.#cleared && (this.#touched.get(key) ?? 0) <= turn;
    this.#rowReads -= 1;
    if (!this.#rowReads) this.#touched.clear();
    else if (newest) this.#touched.set(key, turn);
    return newest;
  }

  /** Takes `row` as the answer of the read with `turn`, when it is the newest; gives back whether */
  takeRow(row: T, turn: number): boolean {
    const key = this.#key(row);
    if (!this.#answered(key, turn)) return false;
    this.#changed?.add(key);
    this.#put(key, row);
    return true;
  }

  /** Writes `fields` onto the row with `key` as the answer of the read with `turn`, likewise */
  assignRow(key: string, fields: Partial<T>, turn: number): boolean {
    const row = this.#rows.get(key);
    if (!this.#answered(key, turn) || !row) return false;
    this.#changed?.add(key);
    Object.assign(row, fields);
    return true;
  }

  /** Ends a read of one row without an answer, as when it failed */
  dropRow() {
    this.#rowReads -= 1;
    if (!this.#rowReads) this.#touched.clear();
  }

  /** Begins a reload of the whole list and gives back its turn */
  reading(): number {
    this.#changed = new Set();
    return ++this.#turn;
  }

  /** Whether no reload began after the one with `turn`, so its answer is the newest coming */
  latest(turn: number) {
    return turn === this.#turn;
  }

  /**
   * Ends the reload with `turn` without taking an answer, as when its ask failed or its answer is
   * outdated; gives back whether it was the latest
   */
  abandon(turn: number) {
    if (!this.latest(turn)) return false;
    this.#changed = null;
    return true;
  }

  /**
   * Makes the list hold exactly `rows`: patches what changed, adds what is new, removes the rest.
   * With a `turn`, `rows` is that reload's answer: it is taken only while no later reload began,
   * and leaves a row added, patched or removed since the reload began as it is. Gives back whether
   * the rows were taken
   */
  reconcile(rows: T[], turn?: number): boolean {
    if (turn !== undefined && !this.latest(turn)) return false;
    const since = turn === undefined ? null : this.#changed;
    if (turn !== undefined) this.#changed = null;
    const keep = new Set<string>();
    for (const row of rows) {
      const key = this.#key(row);
      keep.add(key);
      if (!since?.has(key)) this.upsert(row);
    }
    for (const key of this.#rows.keys()) {
      if (!keep.has(key) && !since?.has(key)) this.remove(key);
    }
    return true;
  }

  /** Empties the list; the answer of a reload that began before is not taken */
  clear() {
    this.#rows.clear();
    this.removed.clear();
    this.#changed = null;
    this.#turn += 1;
    this.#cleared = this.#rowTurn;
  }
}
