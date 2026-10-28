<script lang="ts">
  import { useCrumbs } from "#lib/crumbs.svelte.js";
  import type { Crumb } from "#lib/nav.js";

  // puts `steps` in the header's breadcrumb while it is shown, for a page whose steps depend on
  // which of its parts is open
  let { steps }: { steps: Crumb[] } = $props();

  useCrumbs(() => steps);
</script>
