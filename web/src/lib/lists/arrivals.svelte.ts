/** How long a line that just appeared is marked, which its entrance takes */
export const FRESH_MS = 600;

/**
 * Tells lines that appeared while a list was on screen, by a new row arriving or a folder or
 * section opening, from lines that were there before, so a new line can make an entrance while
 * one that only scrolled into a virtualized list's window doesn't. The first keys seen are the
 * list as it opened. Each change's new lines are one set, dropped together when their entrance
 * is over, so opening a folder of thousands costs one set and one timer
 */
export class Arrivals {
  #previous: Set<string> | null = null;
  #fresh = $state.raw<Set<string>[]>([]);

  /** Notes the list's keys now; call it with every new list */
  see(keys: Iterable<string>) {
    const now = new Set(keys);
    const before = this.#previous;
    this.#previous = now;
    if (!before) return;
    const fresh = new Set<string>();
    for (const key of now) if (!before.has(key)) fresh.add(key);
    if (!fresh.size) return;
    this.#fresh = [...this.#fresh, fresh];
    setTimeout(() => (this.#fresh = this.#fresh.filter((set) => set !== fresh)), FRESH_MS);
  }

  /** Whether the line appeared a moment ago */
  isFresh = (key: string) => this.#fresh.some((set) => set.has(key));
}
