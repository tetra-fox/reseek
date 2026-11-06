<script lang="ts">
  import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

  import type { Schemas } from "#lib/api/client.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // the warning that a file's size doesn't fit its length and format. It is a hint to look
  // twice, never a verdict, so the row it is on is otherwise shown as any other
  let { suspect }: { suspect: Schemas["SuspectHint"] } = $props();

  const first = $derived(
    suspect.reason === "belowDeclaredBitrate"
      ? `Size implies ${suspect.impliedKbps} kbps, well below the stated ${suspect.expectedKbps} kbps`
      : `Size implies ${suspect.impliedKbps} kbps, low for a lossless file`
  );
  const text = $derived(
    `${first}. Tags, cover art, silence or an unusual encoder can also cause this, so check the spectrum after downloading.`
  );
</script>

<Tooltip {text}>
  {#snippet child({ props })}
    <button {...props} type="button" class="inline-flex align-text-bottom" aria-label={first}>
      <TriangleAlertIcon class="size-3.5 text-warning" />
    </button>
  {/snippet}
</Tooltip>
