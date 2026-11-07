<script lang="ts">
  import SelectAll from "#lib/components/ui/SelectAll.svelte";
  import { FILE_COLUMNS, type FileColumn } from "#lib/files/columns.js";
  import type { AllTick } from "#lib/lists/list.svelte.js";

  // the column headers over a tree's file rows, each a `columnheader` in the rows' grid, the
  // tick's holding Select all where the tree has a selection
  let { columns, all }: { columns: FileColumn[]; all?: AllTick } = $props();
</script>

{#each columns as column (column)}
  <span
    role="columnheader"
    class={[
      (column === "size" || column === "length") && "pr-3 text-right",
      column === "tick" && "flex"
    ]}>
    {#if column === "tick" && all}
      <SelectAll {all} />
    {:else}
      <!-- the tick and the menu are named for assistive technology only -->
      <span class={[(column === "tick" || column === "menu") && "sr-only"]}
        >{FILE_COLUMNS[column].label}</span>
    {/if}
  </span>
{/each}
