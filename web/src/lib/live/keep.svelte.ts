import { live, type Topic } from "./live.svelte";

/**
 * Keeps a screen current while it is mounted: follows `topic` on the event stream, calls `reload`
 * on a timer while the stream is down or the refresh setting polls, and once more when the stream
 * says frames were missed. Call it while the component is being set up
 */
export const keepCurrent = (reload: () => void, topic?: Topic) => {
  $effect(() => (topic ? live.retain(topic) : undefined));
  $effect(() => {
    const ms = live.pollMs;
    if (ms === null) return;
    const timer = setInterval(reload, ms);
    return () => clearInterval(timer);
  });
  $effect(() => live.on("lagged", reload));
};
