<script lang="ts">
  import type { Target } from "#lib/analysis/analysis.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";

  // a file's way to the Analysis page, in a menu: Compare only while a file waits to be compared to
  let { target, name }: { target: Target; name: string } = $props();
</script>

<Menu.Item onSelect={() => slots.open(target, name)}
  ><A.openAnalysis.icon />{A.openAnalysis.label}</Menu.Item>
{#if slots.occupied}
  <Menu.Item onSelect={() => slots.compare(target, name)}
    ><A.compare.icon />{A.compare.label}</Menu.Item>
{/if}
