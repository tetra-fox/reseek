import { live, type Topic } from "./live.svelte";

/**
 * Keeps a screen current while it is mounted: follows the topic `topic` gives on the event
 * stream (a function, so a route parameter such as a search's id can change it), calls `reload`
 * on a timer while the stream is down or the refresh setting polls, and once more when the stream
 * says frames were missed. Call it while the component is being set up
 */
export const keepCurrent = (reload: () => void, topic?: () => Topic | undefined) => {
  $effect(() => {
    const followed = topic?.();
    return followed ? live.retain(followed) : undefined;
  });
  $effect(() => {
    const ms = live.pollMs;
    if (ms === null) return;
    const timer = setInterval(reload, ms);
    return () => clearInterval(timer);
  });
  $effect(() => live.on("lagged", reload));
};
