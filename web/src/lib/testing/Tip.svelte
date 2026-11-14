<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";

  // a static element with a tooltip, for the Tooltip tests
  let {
    text,
    dense = false,
    dormant = false
  }: { text?: string; dense?: boolean; dormant?: boolean } = $props();
</script>

<Tooltip {text} {dense} {dormant}>
  {#snippet child({ props })}<span {...props} role="note">tip</span>{/snippet}
</Tooltip>
