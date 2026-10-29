import { flushSync } from "svelte";
import { vi } from "vitest";

/** Lets pending fetches and zero-delay timers run on real timers, then updates the page */
export const flush = async () => {
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};

/** Lets pending fetches and zero-delay timers run under fake timers, then updates the page */
export const flushFaked = async () => {
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
};
