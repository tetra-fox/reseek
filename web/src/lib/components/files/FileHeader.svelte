<script lang="ts">
  import { FILE_COLUMNS, type FileColumn } from "#lib/files/columns.js";

  // the column headers over a tree's file rows, each a `columnheader` in the rows' grid
  let { columns }: { columns: FileColumn[] } = $props();
</script>

{#each columns as column (column)}
  <span
    role="columnheader"
    class={[(column === "size" || column === "length") && "pr-3 text-right"]}>
    <!-- the tick and the menu are named for assistive technology only -->
    <span class={[(column === "tick" || column === "menu") && "sr-only"]}
      >{FILE_COLUMNS[column].label}</span>
  </span>
{/each}
