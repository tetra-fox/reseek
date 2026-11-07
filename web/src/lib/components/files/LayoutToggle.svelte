<script lang="ts">
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import { type Layout, LAYOUTS } from "#lib/lists/layout.js";

  let { value = $bindable() }: { value: Layout } = $props();
</script>

<!-- the switch between a view's tree and its list, the same wherever files are listed -->
<Segmented options={LAYOUTS} bind:value label="View" size="sm" />
