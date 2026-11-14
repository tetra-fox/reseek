import { type Freeze, interaction } from "#lib/lists/interaction.svelte.js";

import { live, type Topic } from "./live.svelte";

/** How often a screen re-reads its data in the background while its tab is visible */
export const RECONCILE_MS = 60_000;

/**
 * Keeps a screen current while it is mounted. It follows the topic `topic` gives on the event
 * stream (a function, so a route parameter such as a search's id can change it), and calls
 * `reload` to re-read the screen's data, which the screen reconciles into its rows in place:
 * every minute while the tab is visible, as soon as the tab is visible again, when frames may
 * have been missed, when a stream that resumed starts following the topic, and on the polling
 * timer while the stream is down. A stream that resumed after a lost one brings what it missed. A
 * reload due while `freeze` holds, the screen's list's own or every list's, waits until it ends.
 * Call it while the component is being set up
 */
export const keepCurrent = (
  reload: () => void,
  topic?: () => Topic | undefined,
  freeze: Freeze = interaction
) => {
  let mounted = true;
  let waiting = false;
  let visible = $state(!document.hidden);

  const run = () => {
    if (waiting) return;
    if (!freeze.frozen) return reload();
    waiting = true;
    freeze.whenThawed(() => {
      waiting = false;
      if (mounted) reload();
    });
  };

  $effect(() => () => (mounted = false));
  $effect(() => {
    const followed = topic?.();
    return followed ? live.retain(followed) : undefined;
  });
  $effect(() => {
    const ms = live.pollMs ?? (visible ? RECONCILE_MS : null);
    if (ms === null) return;
    const timer = setInterval(run, ms);
    return () => clearInterval(timer);
  });
  $effect(() => {
    const change = () => {
      visible = !document.hidden;
      if (visible) run();
    };
    document.addEventListener("visibilitychange", change);
    return () => document.removeEventListener("visibilitychange", change);
  });
  // the screen may lack frames once the stream missed some, or resumed to start following the
  // screen's topic: it reads again
  $effect(() => live.onMissed(run));
  $effect(() =>
    live.onAdded((topics) => {
      const followed = topic?.();
      if (followed && topics.includes(followed)) run();
    })
  );
};
