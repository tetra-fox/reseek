<script lang="ts">
  import ChevronsDownUpIcon from "@lucide/svelte/icons/chevrons-down-up";
  import ChevronsUpDownIcon from "@lucide/svelte/icons/chevrons-up-down";

  import Button from "#lib/components/ui/Button.svelte";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";

  // opens or closes every folder of a tree. Folders that come into the tree later start the same
  // way, until the other button is pressed
  let { expansion }: { expansion: Expansion } = $props();
</script>

<Button variant="outline" size="sm" onclick={() => expansion.reset(true)}>
  <ChevronsUpDownIcon />Expand all
</Button>
<Button variant="outline" size="sm" onclick={() => expansion.reset(false)}>
  <ChevronsDownUpIcon />Collapse all
</Button>
