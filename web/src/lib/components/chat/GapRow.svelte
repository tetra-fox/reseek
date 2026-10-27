<script lang="ts">
  import type { Gap } from "#lib/chat/messages.js";
  import { dateTime, shortTime } from "#lib/format.js";

  // a stretch of room history reseek wasn't there to receive
  let { gap }: { gap: Gap } = $props();

  const sameDay = $derived(
    !!gap.to && new Date(gap.from).toDateString() === new Date(gap.to).toDateString()
  );
  const time = (iso: string) => (sameDay ? shortTime(iso) : dateTime(iso));
  const why = $derived(
    gap.reason === "daemonDown" ? "(reseek was not running)" : "(disconnected from Soulseek)"
  );
</script>

<p role="note" class="px-3 py-2 text-center text-xs text-fg-muted">
  {#if gap.to}
    Chat history unavailable from {time(gap.from)} to {time(gap.to)} {why}
  {:else}
    Chat history unavailable from {dateTime(gap.from)} {why}
  {/if}
</p>
