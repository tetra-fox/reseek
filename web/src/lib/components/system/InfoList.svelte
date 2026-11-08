<script lang="ts">
  import type { Snippet } from "svelte";

  // labels beside their values, as the Info, Account and Data screens show facts
  let { children }: { children: Snippet } = $props();
</script>

<!-- a label takes at most 40% of the line and wraps past that; a value too long for the rest, an
     address or a version, wraps rather than widening the card -->
<dl class="info">{@render children()}</dl>

<style>
  .info {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(0, 1fr);
    gap: 0.25rem 1rem;
    font-size: 0.875rem;
  }
  .info :global(dd) {
    overflow-wrap: anywhere;
  }
  .info :global(dt) {
    color: var(--color-fg-muted);
  }
</style>
