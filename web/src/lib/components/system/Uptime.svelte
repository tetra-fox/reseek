<script lang="ts">
  import { clock } from "#lib/clock.svelte.js";
  import { formatDate, formatDuration } from "#lib/format.js";

  // how long the daemon has run, counting each second while the page shows it: hours, minutes
  // and seconds under a day, days, hours and minutes past one
  let { since }: { since: string } = $props();

  const started = $derived(Date.parse(since));
</script>

<span title={formatDate(since)}>{formatDuration((clock.second - started) / 1000, 3)}</span>
