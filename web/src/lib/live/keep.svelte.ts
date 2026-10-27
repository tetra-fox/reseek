import { interaction } from "#lib/interaction.svelte.js";

import { live, type Topic } from "./live.svelte";

/** How often a screen re-reads its data in the background while its tab is visible */
export const RECONCILE_MS = 60_000;

/**
 * Keeps a screen current while it is mounted. It follows the topic `topic` gives on the event
 * stream (a function, so a route parameter such as a search's id can change it), and calls
 * `reload` to re-read the screen's data, which the screen reconciles into its rows in place:
 * every minute while the tab is visible, as soon as the tab is visible again, when the stream
 * greets after the connection was lost, when the stream says frames were missed, and on the
 * polling timer while the stream is down. A reload due while the user holds a list still waits
 * until they let go. Call it while the component is being set up
 */
export const keepCurrent = (reload: () => void, topic?: () => Topic | undefined) => {
  let mounted = true;
  let waiting = false;
  let visible = $state(!document.hidden);

  const run = () => {
    if (waiting) return;
    if (!interaction.frozen) return reload();
    waiting = true;
    interaction.whenThawed(() => {
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
  // a stream that opens after the old one was lost may have missed frames in between
  let before = live.status;
  $effect(() => {
    const status = live.status;
    if (status === "connected" && before === "polling") run();
    before = status;
  });
  $effect(() => live.on("lagged", run));
};
