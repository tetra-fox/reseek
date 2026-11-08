<script lang="ts" generics="T extends object">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import type { Snippet } from "svelte";

  import type { AllTick } from "#lib/lists/list.svelte.js";
  import type { Grid, GridColumn } from "#lib/lists/table.svelte.js";

  import SelectAll from "./SelectAll.svelte";
  import Tooltip from "./Tooltip.svelte";

  // a grid's header row: a label per column, a click on one sorts by it, and its right edge
  // drags the column's width
  type Props = {
    grid: Grid<T>;
    /**
     * the row's own classes: its height, border and background. Its font stays the rows', so a
     * column width in ch measures the same in the header as in the cells
     */
    class?: string;
    /** what a quiet column's header holds instead of its label alone */
    cell?: Snippet<[GridColumn<T>]>;
    /** the list's Select all, drawn over its `select` column */
    all?: AllTick;
  };

  let { grid, class: className, cell, all }: Props = $props();
</script>

<!-- a label that doesn't fit is cut, with the whole of it on hover. A column's hint shows on hover
     and, outside a sort button, on focus -->
{#snippet label(column: GridColumn<T>)}
  <Tooltip
    text={column.hint ?? column.label}
    clipped={!column.hint}
    dense={!!column.hint && grid.sorts(column)}>
    {#snippet child({ props })}
      <span {...props} class="truncate">{column.label}</span>
    {/snippet}
  </Tooltip>
{/snippet}

<div role="row" class={["group/head grid items-center", className]} style={grid.row}>
  {#each grid.columns as column (column.id)}
    {@const sort = grid.sortOf(column)}
    <div
      role="columnheader"
      aria-sort={sort === "asc" ? "ascending" : sort === "desc" ? "descending" : undefined}
      class={[
        "relative flex min-w-0 items-center self-stretch px-2 text-xs font-medium",
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
          {@render label(column)}
          {#if sort === "asc"}<ArrowUpIcon
              class="size-3 shrink-0" />{:else if sort === "desc"}<ArrowDownIcon
              class="size-3 shrink-0" />{/if}
        </button>
      {:else}
        {@render label(column)}
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
