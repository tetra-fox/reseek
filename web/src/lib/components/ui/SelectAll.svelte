<script lang="ts">
  import type { AllTick } from "#lib/lists/list.svelte.js";

  import Checkbox from "./Checkbox.svelte";

  // a list's header tick: checks every row the list shows, or clears them once all are checked
  let { all }: { all: AllTick } = $props();
</script>

<Checkbox
  aria-label="Select all"
  checked={all.checked}
  indeterminate={all.indeterminate}
  disabled={all.empty}
  onCheckedChange={() => all.toggle()} />
