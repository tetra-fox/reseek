<script lang="ts">
  import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import ChevronsLeftIcon from "@lucide/svelte/icons/chevrons-left";
  import ChevronsRightIcon from "@lucide/svelte/icons/chevrons-right";

  import Button from "./Button.svelte";
  import Tooltip from "./Tooltip.svelte";

  // first, previous, the pages around this one, next, last
  type Props = { page: number; pages: number; onpage: (page: number) => void };
  let { page, pages, onpage }: Props = $props();

  /** pages shown on each side of the current one */
  const AROUND = 2;
  const numbers = $derived(
    Array.from({ length: pages }, (_, i) => i + 1).filter((n) => Math.abs(n - page) <= AROUND)
  );
</script>

<nav aria-label="Pages" class="flex items-center gap-0.5">
  <Tooltip text="First page">
    {#snippet child({ props })}
      <Button
        {...props}
        variant="ghost"
        size="icon-sm"
        aria-label="First page"
        disabled={page <= 1}
        onclick={() => onpage(1)}><ChevronsLeftIcon /></Button>
    {/snippet}
  </Tooltip>
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
  {#each numbers as n (n)}
    <Button
      variant={n === page ? "secondary" : "ghost"}
      size="sm"
      aria-current={n === page ? "page" : undefined}
      onclick={() => onpage(n)}>{n}</Button>
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
  <Tooltip text="Last page">
    {#snippet child({ props })}
      <Button
        {...props}
        variant="ghost"
        size="icon-sm"
        aria-label="Last page"
        disabled={page >= pages}
        onclick={() => onpage(pages)}><ChevronsRightIcon /></Button>
    {/snippet}
  </Tooltip>
</nav>
