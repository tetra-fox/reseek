<script lang="ts">
  import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";

  import { formatCount } from "#lib/format.js";
  import { pagerItems } from "#lib/lists/pages.js";

  import Button from "./Button.svelte";
  import Tooltip from "./Tooltip.svelte";

  // previous, the first and last pages and those around this one with a gap for the rest, next
  type Props = { page: number; pages: number; onpage: (page: number) => void };
  let { page, pages, onpage }: Props = $props();

  const items = $derived(pagerItems(page, pages));
</script>

<nav aria-label="Pages" class="flex flex-wrap items-center gap-0.5">
  <Tooltip text="Previous page">
    {#snippet child({ props })}
      <Button
        {...props}
        variant="ghost"
        size="icon-sm"
        aria-label="Previous page"
        disabled={page <= 1}
        onclick={() => onpage(page - 1)}><ChevronLeftIcon /></Button>
    {/snippet}
  </Tooltip>
  {#each items as item, i (item === "gap" ? `gap ${i}` : item)}
    {#if item === "gap"}
      <span class="px-1 text-sm text-fg-subtle" aria-hidden="true">…</span>
    {:else}
      <Button
        variant={item === page ? "secondary" : "ghost"}
        size="sm"
        class="tabular-nums"
        aria-label={`Page ${item}`}
        aria-current={item === page ? "page" : undefined}
        onclick={() => onpage(item)}>{formatCount(item)}</Button>
    {/if}
  {/each}
  <Tooltip text="Next page">
    {#snippet child({ props })}
      <Button
        {...props}
        variant="ghost"
        size="icon-sm"
        aria-label="Next page"
        disabled={page >= pages}
        onclick={() => onpage(page + 1)}><ChevronRightIcon /></Button>
    {/snippet}
  </Tooltip>
</nav>
