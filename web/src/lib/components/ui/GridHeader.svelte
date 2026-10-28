<script lang="ts" generics="T extends object">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import type { Snippet } from "svelte";

  import type { Grid, GridColumn } from "#lib/table.svelte.js";

  // a grid's header row: a label per column, a click on one sorts by it, and its right edge
  // drags the column's width. A label that doesn't fit is cut, with the whole of it on hover
  type Props = {
    grid: Grid<T>;
    /** the row's own classes: its height, border and background */
    class?: string;
    /** what a quiet column's header holds instead of its label alone */
    cell?: Snippet<[GridColumn<T>]>;
  };

  let { grid, class: className, cell }: Props = $props();
</script>

<div
  role="row"
  class={["grid items-center", className]}
  style:grid-template-columns={grid.template}>
  {#each grid.columns as column (column.id)}
    {@const sort = grid.sortOf(column)}
    <div
      role="columnheader"
      aria-sort={sort === "asc" ? "ascending" : sort === "desc" ? "descending" : undefined}
      class={["relative flex min-w-0 items-center px-2 text-xs", column.end && "justify-end"]}>
      {#if column.quiet}
        <span class="sr-only">{column.label}</span>
        {@render cell?.(column)}
      {:else if grid.sorts(column)}
        <button
          type="button"
          class="flex min-w-0 items-center gap-1 hover:text-fg"
          title={column.label}
          onclick={() => grid.toggle(column)}>
          <span class="truncate">{column.label}</span>
          {#if sort === "asc"}<ArrowUpIcon
              class="size-3 shrink-0" />{:else if sort === "desc"}<ArrowDownIcon
              class="size-3 shrink-0" />{/if}
        </button>
      {:else}
        <span class="truncate" title={column.label}>{column.label}</span>
      {/if}
      {#if !column.fixed}
        <!-- dragging the edge sets the column's width, which is kept. TanStack's drag follows mouse
             and touch events, not pointer events -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <span
          class="absolute top-1 right-0 bottom-1 w-1.5 cursor-col-resize touch-none border-r"
          role="separator"
          aria-orientation="vertical"
          aria-label={`Width of ${column.label}`}
          onmousedown={(e) => grid.resize(column.id, e)}
          ontouchstart={(e) => grid.resize(column.id, e)}></span>
      {/if}
    </div>
  {/each}
</div>
