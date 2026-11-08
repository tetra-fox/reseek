<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { ago, chatTime, formatDate } from "#lib/format.js";
  import { clock } from "#lib/ui/clock.svelte.js";

  // the time cell of an activity log's line, with the date before today, and the full date and how
  // long ago as its tooltip
  let { at }: { at: string } = $props();
</script>

<span role="gridcell" class="pt-0.5 text-xs whitespace-nowrap text-fg-subtle tabular-nums">
  <Tooltip text={`${formatDate(at)}, ${ago(at, clock.now)}`} dense>
    {#snippet child({ props })}
      <time {...props} datetime={at}>{chatTime(at, clock.now)}</time>
    {/snippet}
  </Tooltip>
</span>
