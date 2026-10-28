<script lang="ts">
  import { clock } from "#lib/clock.svelte.js";
  import { ago, formatDate, shortTime } from "#lib/format.js";

  // the time cell of an activity log's line, with the full date and how long ago as its tooltip
  let { at }: { at: string } = $props();
</script>

<span role="gridcell" class="pt-0.5 text-xs whitespace-nowrap text-fg-subtle tabular-nums"
  ><time datetime={at} title={`${formatDate(at)}, ${ago(at, clock.now)}`}>{shortTime(at)}</time
  ></span>
