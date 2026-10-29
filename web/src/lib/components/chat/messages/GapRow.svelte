<script lang="ts">
  import type { Gap } from "#lib/chat/messages/messages.js";
  import { dateTime, shortTime } from "#lib/format.js";
  import { GAP_NOTICE, GAP_REASONS } from "#lib/wording.js";

  // a stretch of room history reseek wasn't there to receive
  let { gap }: { gap: Gap } = $props();

  const sameDay = $derived(
    !!gap.to && new Date(gap.from).toDateString() === new Date(gap.to).toDateString()
  );
  const time = (iso: string) => (sameDay ? shortTime(iso) : dateTime(iso));
  const why = $derived(
    gap.reason === "daemonDown" ? GAP_REASONS.daemonDown : GAP_REASONS.disconnected
  );
</script>

<p role="note" class="px-3 py-2 text-center text-xs text-fg-muted">
  {#if gap.to}
    {GAP_NOTICE} from {time(gap.from)} to {time(gap.to)} {why}
  {:else}
    {GAP_NOTICE} from {dateTime(gap.from)} {why}
  {/if}
</p>
