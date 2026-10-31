<script lang="ts">
  import { Combobox } from "bits-ui";
  import type { Snippet } from "svelte";

  let { heading, children }: { heading: string; children: Snippet } = $props();
</script>

<Combobox.Group>
  <Combobox.GroupHeading class="px-2 pt-2 pb-0.5 text-xs font-medium text-fg-muted">
    {heading}
  </Combobox.GroupHeading>
  {@render children()}
</Combobox.Group>
