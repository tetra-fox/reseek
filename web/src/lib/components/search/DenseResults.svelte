<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderDownIcon from "@lucide/svelte/icons/folder-down";
  import LockIcon from "@lucide/svelte/icons/lock";
  import { SvelteSet } from "svelte/reactivity";

  import FileMenu from "#lib/components/files/FileMenu.svelte";
  import Suspect from "#lib/components/files/Suspect.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { asItem, enqueue } from "#lib/downloads.js";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import {
    formatAttributes,
    formatBytes,
    formatSeconds,
    getFileName,
    plural
  } from "#lib/format.js";
  import { Arrivals } from "#lib/lists/arrivals.svelte.js";
  import { freezesWhileUsed, heldOrder } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid } from "#lib/lists/table.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { DENSE_COLUMNS, type DenseColumn, type DenseRow, denseView } from "#lib/search/dense.js";
  import { rem } from "#lib/ui/viewport.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import Country from "./Country.svelte";

  type Props = {
    /** one row per file, in the order the results are in */
    rows: DenseRow[];
    searchId: string;
    /** downloads need the Soulseek server and write access */
    canDownload: boolean;
  };

  let { rows, searchId, canDownload }: Props = $props();

  let headerHeight = $state(0);
  const selected = new SvelteSet<string>();
  const arrivals = new Arrivals();

  const grid = new Grid<DenseRow>({
    columns: () => DENSE_COLUMNS,
    data: () => rows,
    sort: () => denseView.sort,
    setSort: (sort) => (denseView.sort = sort as typeof denseView.sort),
    dragged: () => denseView.dragged,
    drag: (id, width) => (denseView.dragged[id as DenseColumn] = width),
    clearable: true
  });

  // while the user holds the list still, rows keep their places and new ones go below
  const hold = heldOrder<DenseRow>((r) => r.key);
  const sorted = $derived(hold(grid.rows));
  $effect(() => arrivals.see(sorted.map((r) => r.key)));

  /** one row's height, 1.75rem in pixels */
  const rowHeight = $derived(1.75 * rem.px);
  const chosen = $derived(sorted.filter((r) => selected.has(r.key)));
  const chosenBytes = $derived(chosen.reduce((n, r) => n + r.file.size, 0));

  /** the files of each user's folder, for a row's "Download folder" */
  const folders = $derived.by(() => {
    const map = new Map<string, DenseRow[]>();
    for (const r of rows) {
      const key = `${r.username}\u0000${r.folder}`;
      const list = map.get(key);
      if (list) list.push(r);
      else map.set(key, [r]);
    }
    return map;
  });
  const folderOf = (r: DenseRow) => folders.get(`${r.username}\u0000${r.folder}`) ?? [r];

  // the users on screen are looked up, for their presence and the offline toggle
  $effect(() => {
    for (const name of new Set(lines.drawn.map((drawn) => drawn.line.username))) {
      void users.load(name);
    }
  });

  /** Enqueues rows, one batch per user, and says what came of it */
  const download = async (list: DenseRow[]) => {
    const byUser = new Map<string, DenseRow[]>();
    for (const r of list) {
      const mine = byUser.get(r.username);
      if (mine) mine.push(r);
      else byUser.set(r.username, [r]);
    }
    const failed: string[] = [];
    let queued = 0;
    for (const [username, mine] of byUser) {
      const result = await enqueue({
        username,
        files: mine.map((r) => asItem(r.file)),
        searchId
      });
      if (result.kind === "done") queued += mine.length;
      else failed.push(`${username}: ${result.text}`);
    }
    if (!failed.length) toast(`${plural(queued, "file", "files")} queued`);
    else toast(`${plural(queued, "file", "files")} queued. Failed: ${failed.join("; ")}`, "error");
  };

  const list = new ListControl<DenseRow>({
    lines: () => sorted,
    key: (r) => r.key,
    id: (r) => r.key,
    onopen: (r) => {
      if (canDownload) void download(selected.size > 1 ? chosen : [r]);
    },
    rendered: (index) => lines.isDrawn(index),
    reveal: (index) => lines.scrollTo(index),
    selected
  });

  const lines: ListWindow<DenseRow> = new ListWindow(
    () => sorted,
    (r) => r.key,
    () => ({
      estimateSize: () => rowHeight,
      overscan: 12,
      scrollMargin: headerHeight,
      scrollPaddingStart: headerHeight,
      sizes: rowHeight,
      // a control in a line that scrolled out of the window gives focus back to the list
      onchange: () => list.scrolled()
    })
  );

  /** Gives the grid DOM focus, without scrolling */
  export const focus = list.focus;
</script>

<div class="flex flex-col gap-2">
  <!-- every row, the header too, has the cells' font with tabular figures, so a column width in
       ch measures the same in each; the header's labels are smaller inside it -->
  <div
    class="relative overflow-auto rounded-card border bg-surface text-sm tabular-nums shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
    style:height="max(20rem, calc(var(--content-h) - 16rem))"
    style:--line-h={`${rowHeight}px`}
    role="grid"
    aria-label="Search results"
    aria-rowcount={sorted.length}
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach lines.attach}
    {@attach list.attach}>
    <div class="sticky top-0 z-10" style:min-width={grid.width} bind:offsetHeight={headerHeight}>
      <GridHeader {grid} class="h-8 border-b bg-surface-muted text-sm font-medium text-fg-muted" />
    </div>
    <div
      class="relative"
      role="rowgroup"
      style:height={`${lines.total}px`}
      style:min-width={grid.width}
      {@attach freezesWhileUsed}>
      {#each lines.drawn as { line: row, index, start, size } (row.key)}
        {@const name = getFileName(row.file.filename)}
        {@const folder = folderOf(row)}
        <div
          role="row"
          {...list.line(row, index)}
          class={[
            "group absolute inset-x-0 top-0 grid items-center list-row hover:bg-hover",
            selected.has(row.key) && "bg-accent-soft/60",
            arrivals.isFresh(row.key) && "animate-row-in"
          ]}
          style:transform={`translateY(${start - headerHeight}px)`}
          style:height={`${size}px`}
          style:grid-template-columns={grid.template}
          onmousedown={(e) => {
            // the middle button scrolls the page by dragging, unless kept from it
            if (e.button === 1 && canDownload) e.preventDefault();
          }}
          onauxclick={(e) => {
            if (e.button === 1 && canDownload) void download([row]);
          }}>
          <span role="gridcell" class="flex justify-center">
            <Checkbox
              aria-label={`Select ${name}`}
              checked={selected.has(row.key)}
              onclick={(e) => {
                e.preventDefault();
                list.tick(row.key, e.shiftKey);
              }}
              onCheckedChange={(on) => on !== selected.has(row.key) && list.toggle(row.key)} />
          </span>
          <span role="gridcell" class="flex min-w-0 items-center gap-1.5 px-2">
            <span class="min-w-0 truncate"><Username name={row.username} /></span>
            {#if row.country}<Country code={row.country} />{/if}
          </span>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{formatBytes(row.speed, 1)}/s</span>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums">{row.queue}</span>
          <span role="gridcell" class={["px-2", row.free ? "text-success" : "text-fg-muted"]}>
            {row.free ? "Yes" : "No"}
          </span>
          <span
            role="gridcell"
            class="flex min-w-0 items-center gap-1 px-2"
            title={row.file.filename}>
            {#if row.file.locked}<LockIcon class="size-3.5 shrink-0 text-fg-muted" />{/if}
            <span class="truncate">{name}</span>
          </span>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{formatBytes(row.file.size)}</span>
          <span role="gridcell" class="flex min-w-0 items-center gap-1 truncate px-2 text-fg-muted">
            {#if row.file.suspect}<Suspect suspect={row.file.suspect} />{/if}
            {formatAttributes(row.file)}
          </span>
          <span role="gridcell" class="truncate px-2 text-right text-fg-muted tabular-nums">
            {formatSeconds(row.file.length)}
          </span>
          <span
            role="gridcell"
            class="truncate px-2 font-mono text-xs text-fg-muted"
            title={row.folder}>
            {row.folder}
          </span>
          <span role="gridcell" class="flex items-center justify-end gap-0.5 pr-1 row-action">
            {#if canDownload}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Download the folder, ${plural(folder.length, "file", "files")}, ${formatBytes(folder.reduce((n, r) => n + r.file.size, 0))}`}
                title={`Download the folder: ${plural(folder.length, "file", "files")}, ${formatBytes(folder.reduce((n, r) => n + r.file.size, 0))}`}
                onclick={() => void download(folder)}>
                <FolderDownIcon />
              </Button>
            {/if}
            <FileMenu
              path={row.file.filename}
              username={row.username}
              ondownload={canDownload ? () => void download([row]) : undefined}>
              {#snippet trigger({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Actions for ${name}`}>
                  <EllipsisIcon />
                </Button>
              {/snippet}
            </FileMenu>
          </span>
        </div>
      {/each}
    </div>
  </div>
  {#if chosen.length}
    <div class="selection-bar" role="toolbar" aria-label="Selected files">
      <span class="text-sm"
        >{plural(chosen.length, "file", "files")} · {formatBytes(chosenBytes)}</span>
      {#if canDownload}
        <Button size="sm" onclick={() => void download(chosen)}><DownloadIcon />Download</Button>
      {/if}
      <Button size="sm" variant="ghost" onclick={() => selected.clear()}>Clear</Button>
    </div>
  {/if}
</div>
