<script lang="ts">
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import { Grid, type GridSort } from "#lib/table.svelte.js";

  type Row = { key: string; name: string; size: number };

  // a grid of name and size over rows a test changes, keeping its sort and widths where the test
  // reads them, for the grid's tests
  let {
    source,
    kept
  }: {
    source: { value: Row[] };
    kept: { sort: GridSort; dragged: Record<string, number> };
  } = $props();

  // svelte-ignore state_referenced_locally
  const rows = source;
  // svelte-ignore state_referenced_locally
  const state = kept;
  const grid = new Grid<Row>({
    columns: () => [
      { id: "pick", label: "Select", width: "2rem", quiet: true, fixed: true },
      { id: "name", label: "Name", width: "10rem", grow: true, value: (r) => r.name.toLowerCase() },
      { id: "size", label: "Size", width: "6ch", end: true, descFirst: true, value: (r) => r.size }
    ],
    data: () => rows.value,
    sort: () => state.sort,
    setSort: (sort) => (state.sort = sort),
    dragged: () => state.dragged,
    drag: (id, width) => (state.dragged[id] = width),
    clearable: true,
    ties: "name"
  });
</script>

<div role="grid" aria-label="Rows" data-template={grid.template}>
  <GridHeader {grid} />
  {#each grid.rows as row (row.key)}
    <div role="row" data-key={row.key}>{row.name}</div>
  {/each}
</div>
