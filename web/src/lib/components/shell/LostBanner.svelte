<script lang="ts">
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

  import { live } from "#lib/live/live.svelte.js";

  // a ticking clock for the countdown, only while the banner shows, read again whenever the
  // retry is set so its first number is the wait itself
  let now = $state(Date.now());
  const shown = $derived(live.status === "polling");
  $effect(() => {
    if (!shown) return;
    void live.retryAt;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const seconds = $derived(
    live.retryAt === null ? null : Math.max(0, Math.ceil((live.retryAt - now) / 1000))
  );
</script>

<!-- the screen stays as it is underneath; the shell polls until the stream is back -->
{#if shown || live.status === "rejected"}
  <div
    class="flex items-center gap-2 border-b border-warning/40 bg-warning-soft px-gutter py-2 text-sm text-warning-soft-fg"
    role="status">
    <TriangleAlertIcon class="size-4 shrink-0 text-warning-icon" />
    <span>
      {#if shown}
        Lost connection to reseek. {seconds ? `Retrying in ${seconds}s.` : "Retrying..."}
      {:else}
        Live updates are off: {live.rejection}
      {/if}
    </span>
  </div>
{/if}
