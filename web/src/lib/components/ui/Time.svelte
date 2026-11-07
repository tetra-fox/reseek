<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { ago, dateTime } from "#lib/format.js";
  import { clock } from "#lib/ui/clock.svelte.js";

  // a moment as "3 minutes ago", kept current, the exact time on hover
  let { at, class: className }: { at: string; class?: string } = $props();
</script>

<Tooltip text={dateTime(at)}>
  {#snippet child({ props })}
    <time {...props} datetime={at} class={className}>{ago(at, clock.now)}</time>
  {/snippet}
</Tooltip>
