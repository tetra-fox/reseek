<script lang="ts">
  import type { Keyed } from "#lib/lists/keyed.svelte.js";

  // a list rendered the way every list of the app is, for the stable-DOM tests
  let { list }: { list: Keyed<{ id: string; name: string; size: number }> } = $props();
</script>

<ul>
  {#each list.rows as row (row.id)}
    <li data-id={row.id} class={{ removed: list.removed.has(row.id) }}>
      <span class="name">{row.name}</span> <span class="size">{row.size}</span>
    </li>
  {/each}
</ul>
