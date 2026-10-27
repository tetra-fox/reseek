let ticking = false;

/**
 * The time for relative labels ("3 min ago"), moved on once a minute, so every label on the page
 * updates its text together without its element being made again
 */
class Clock {
  #now = $state(Date.now());

  get now(): number {
    if (!ticking) {
      ticking = true;
      setInterval(() => (this.#now = Date.now()), 60_000);
    }
    return this.#now;
  }
}

export const clock = new Clock();
