import { createSubscriber } from "svelte/reactivity";

let ticking = false;

// one timer for every reader of the second: it runs while something on the page shows one, and
// skips its updates while the tab is hidden
const second = createSubscriber((update) => {
  const timer = setInterval(() => {
    if (!document.hidden) update();
  }, 1000);
  return () => clearInterval(timer);
});

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

  /**
   * The time for a figure that counts seconds, such as an uptime: reading it makes the reader run
   * again a second later, while the tab is visible
   */
  get second(): number {
    second();
    return Date.now();
  }
}

export const clock = new Clock();
