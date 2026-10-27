let ticking = false;

/**
 * The time for relative labels ("3 min ago"). Reading it gives the time now and makes the
 * reader run again a minute later, so every label on the page updates its text together
 * without its element being made again
 */
class Clock {
  #minute = $state(0);

  get now(): number {
    if (!ticking) {
      ticking = true;
      setInterval(() => (this.#minute += 1), 60_000);
    }
    void this.#minute;
    return Date.now();
  }
}

export const clock = new Clock();
