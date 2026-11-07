<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { formatDate, formatDuration } from "#lib/format.js";
  import { clock } from "#lib/ui/clock.svelte.js";

  // how long the daemon has run, counting each second while the page shows it: hours, minutes
  // and seconds under a day, days, hours and minutes past one
  let { since }: { since: string } = $props();

  const started = $derived(Date.parse(since));
</script>

<Tooltip text={formatDate(since)}>
  {#snippet child({ props })}
    <span {...props}>{formatDuration((clock.second - started) / 1000, 3)}</span>
  {/snippet}
</Tooltip>
