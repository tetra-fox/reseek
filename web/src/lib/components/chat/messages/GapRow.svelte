<script lang="ts">
  import type { Gap } from "#lib/chat/messages/messages.js";
  import { dateTime, shortTime } from "#lib/format.js";
  import { gapNotice } from "#lib/wording.js";

  // a stretch of room history reseek wasn't there to receive
  let { gap }: { gap: Gap } = $props();

  const sameDay = $derived(
    !!gap.to && new Date(gap.from).toDateString() === new Date(gap.to).toDateString()
  );
  const time = (iso: string) => (sameDay ? shortTime(iso) : dateTime(iso));
  const span = $derived(
    gap.to ? `from ${time(gap.from)} to ${time(gap.to)}` : `from ${dateTime(gap.from)}`
  );
</script>

<p role="note" class="px-3 py-2 text-center text-xs text-fg-muted">{gapNotice(span)}</p>
