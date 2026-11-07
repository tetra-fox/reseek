<script lang="ts" generics="T extends object">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import type { Snippet } from "svelte";

  import type { AllTick } from "#lib/lists/list.svelte.js";
  import type { Grid, GridColumn } from "#lib/lists/table.svelte.js";

  import SelectAll from "./SelectAll.svelte";
  import Tooltip from "./Tooltip.svelte";

  // a grid's header row: a label per column, a click on one sorts by it, and its right edge
  // drags the column's width. A label that doesn't fit is cut, with the whole of it on hover
  type Props = {
    grid: Grid<T>;
    /** the row's own classes: its height, border and background */
    class?: string;
    /** what a quiet column's header holds instead of its label alone */
    cell?: Snippet<[GridColumn<T>]>;
    /** the list's Select all, drawn over its `select` column */
    all?: AllTick;
  };

  let { grid, class: className, cell, all }: Props = $props();
</script>

<div
  role="row"
  class={["group/head grid items-center", className]}
  style:grid-template-columns={grid.template}>
  {#each grid.columns as column (column.id)}
    {@const sort = grid.sortOf(column)}
    <div
      role="columnheader"
      aria-sort={sort === "asc" ? "ascending" : sort === "desc" ? "descending" : undefined}
      class={[
        "relative flex min-w-0 items-center self-stretch px-2 text-xs",
        column.end && "justify-end"
      ]}>
      {#if column.id === "select" && all}
        <SelectAll {all} />
      {:else if column.quiet}
        <span class="sr-only">{column.label}</span>
        {@render cell?.(column)}
      {:else if grid.sorts(column)}
        <button
          type="button"
          class="flex min-w-0 items-center gap-1 self-stretch hover:text-fg"
          onclick={() => grid.toggle(column)}>
          <Tooltip text={column.label} clipped>
            {#snippet child({ props })}
              <span {...props} class="truncate">{column.label}</span>
            {/snippet}
          </Tooltip>
          {#if sort === "asc"}<ArrowUpIcon
              class="size-3 shrink-0" />{:else if sort === "desc"}<ArrowDownIcon
              class="size-3 shrink-0" />{/if}
        </button>
      {:else}
        <Tooltip text={column.label} clipped>
          {#snippet child({ props })}
            <span {...props} class="truncate">{column.label}</span>
          {/snippet}
        </Tooltip>
      {/if}
      {#if !column.fixed}
        <!-- dragging the edge sets the column's width, which is kept. TanStack's drag follows mouse
             and touch events, not pointer events. The edge shows while the header is in use -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <span
          class="absolute inset-y-0 right-0 w-1.5 cursor-col-resize touch-none after:absolute after:inset-y-3 after:right-0 after:border-r after:opacity-0 after:transition-[opacity,border-color] after:duration-(--motion-fast) group-focus-within/head:after:opacity-100 group-hover/head:after:opacity-100 hover:after:border-accent data-resizing:after:border-accent data-resizing:after:opacity-100 [@media(hover:none)]:after:opacity-100"
          data-resizing={grid.header(column.id)?.column.getIsResizing() || undefined}
          role="separator"
          aria-orientation="vertical"
          aria-label={`Width of ${column.label}`}
          onmousedown={(e) => grid.resize(column.id, e)}
          ontouchstart={(e) => grid.resize(column.id, e)}></span>
      {/if}
    </div>
  {/each}
</div>
