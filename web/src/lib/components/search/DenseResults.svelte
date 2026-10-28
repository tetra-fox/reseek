<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderDownIcon from "@lucide/svelte/icons/folder-down";
  import LockIcon from "@lucide/svelte/icons/lock";
  import { SvelteSet } from "svelte/reactivity";

  import { Arrivals } from "#lib/arrivals.svelte.js";
  import FileMenu from "#lib/components/files/FileMenu.svelte";
  import Suspect from "#lib/components/files/Suspect.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { asItem, enqueue } from "#lib/downloads.js";
  import {
    formatAttributes,
    formatBytes,
    formatSeconds,
    getFileName,
    plural
  } from "#lib/format.js";
  import { freezesWhileUsed, heldOrder } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import {
    DENSE_COLUMNS,
    type DenseColumn,
    type DenseRow,
    denseView,
    nextSort,
    sortDense
  } from "#lib/search/dense.js";
  import { toast } from "#lib/toast.svelte.js";
  import { users } from "#lib/users.svelte.js";

  import Country from "./Country.svelte";

  type Props = {
    /** one row per file, in the order the results are in */
    rows: DenseRow[];
    searchId: string;
    /** downloads need the Soulseek server and write access */
    canDownload: boolean;
  };

  let { rows, searchId, canDownload }: Props = $props();

  /** one row's height, and the rows drawn beyond the visible ones */
  const ROW = 28;
  const OVERSCAN = 12;

  let scrollTop = $state(0);
  let height = $state(600);
  let grid = $state<HTMLDivElement | null>(null);
  let headerHeight = $state(0);
  let resizing: { id: DenseColumn; x: number; width: number } | null = null;
  const selected = new SvelteSet<string>();
  const arrivals = new Arrivals();

  // while the user holds the list still, rows keep their places and new ones go below
  const hold = heldOrder<DenseRow>((r) => r.key);
  const sorted = $derived(hold(sortDense(rows, denseView.sort)));
  $effect(() => arrivals.see(sorted.map((r) => r.key)));

  const template = $derived(
    `2.5rem ${DENSE_COLUMNS.map((c) => `${denseView.widths[c.id]}px`).join(" ")} 4.5rem`
  );
  const width = $derived(40 + 72 + DENSE_COLUMNS.reduce((n, c) => n + denseView.widths[c.id], 0));
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const end = $derived(Math.min(sorted.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
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
    for (const name of new Set(sorted.slice(start, end).map((r) => r.username))) {
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
    else toast(`${queued} queued; ${failed.join("; ")}`, "error");
  };

  const list = new ListControl<DenseRow>({
    lines: () => sorted,
    key: (r) => r.key,
    id: (r) => r.key,
    onopen: (r) => {
      if (canDownload) void download(selected.size > 1 ? chosen : [r]);
    },
    rendered: (index) => index >= start && index < end,
    reveal: (index) => {
      if (!grid) return;
      const top = index * ROW;
      if (top < grid.scrollTop) grid.scrollTop = top;
      else if (top + ROW > grid.scrollTop + height - headerHeight) {
        grid.scrollTop = top + ROW - height + headerHeight;
      }
    },
    selected
  });

  /** Gives the grid DOM focus, without scrolling */
  export const focus = list.focus;
</script>

<div class="flex flex-col gap-2">
  <div
    class="relative overflow-auto rounded-card border bg-surface text-sm shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
    style:height="max(20rem, calc(var(--content-h) - 16rem))"
    role="grid"
    aria-label="Search results"
    aria-rowcount={sorted.length}
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    bind:this={grid}
    bind:clientHeight={height}
    onscroll={(e) => {
      scrollTop = e.currentTarget.scrollTop;
      list.scrolled();
    }}
    {@attach list.attach}>
    <div
      role="row"
      class="sticky top-0 z-10 grid h-8 items-center border-b bg-surface-muted text-xs font-medium text-fg-muted"
      style:grid-template-columns={template}
      style:min-width={`${width}px`}
      bind:offsetHeight={headerHeight}>
      <span role="columnheader" class="sr-only">Select</span>
      {#each DENSE_COLUMNS as column (column.id)}
        {@const sort = denseView.sort?.id === column.id ? denseView.sort.dir : null}
        <span
          role="columnheader"
          aria-sort={sort === "asc" ? "ascending" : sort === "desc" ? "descending" : undefined}
          class={["relative flex min-w-0 items-center px-2", column.end && "justify-end"]}>
          <button
            type="button"
            class="flex min-w-0 items-center gap-1 truncate hover:text-fg"
            onclick={() => (denseView.sort = nextSort(denseView.sort, column.id))}>
            {column.label}
            {#if sort === "asc"}<ArrowUpIcon
                class="size-3" />{:else if sort === "desc"}<ArrowDownIcon class="size-3" />{/if}
          </button>
          <!-- dragging the edge sets the column's width, which is kept -->
          <span
            class="absolute top-1 right-0 bottom-1 w-1.5 cursor-col-resize border-r"
            role="separator"
            aria-label={`Width of ${column.label}`}
            onpointerdown={(e) => {
              resizing = { id: column.id, x: e.clientX, width: denseView.widths[column.id] };
              e.currentTarget.setPointerCapture(e.pointerId);
            }}
            onpointermove={(e) => {
              if (!resizing) return;
              denseView.widths[resizing.id] = Math.max(40, resizing.width + e.clientX - resizing.x);
            }}
            onpointerup={() => (resizing = null)}></span>
        </span>
      {/each}
      <span role="columnheader" class="sr-only">Actions</span>
    </div>
    <div
      class="relative"
      role="rowgroup"
      style:height={`${sorted.length * ROW}px`}
      style:min-width={`${width}px`}
      {@attach freezesWhileUsed}>
      {#each sorted.slice(start, end) as row, i (row.key)}
        {@const name = getFileName(row.file.filename)}
        {@const folder = folderOf(row)}
        <div
          role="row"
          {...list.line(row, start + i)}
          class={[
            "group absolute inset-x-0 grid items-center border-b hover:bg-hover",
            selected.has(row.key) && "bg-accent-soft/60",
            list.isFocused(row) && "ring-1 ring-accent ring-inset",
            arrivals.isFresh(row.key) && "animate-row-in"
          ]}
          style:top={`${(start + i) * ROW}px`}
          style:height={`${ROW}px`}
          style:grid-template-columns={template}
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
            <span class="flex min-w-0"><Username name={row.username} /></span>
            {#if row.country}<Country code={row.country} />{/if}
          </span>
          <span role="gridcell" class="px-2 text-right tabular-nums"
            >{formatBytes(row.speed, 1)}/s</span>
          <span role="gridcell" class="px-2 text-right tabular-nums">{row.queue}</span>
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
          <span role="gridcell" class="px-2 text-right tabular-nums"
            >{formatBytes(row.file.size)}</span>
          <span role="gridcell" class="flex min-w-0 items-center gap-1 truncate px-2 text-fg-muted">
            {#if row.file.suspect}<Suspect suspect={row.file.suspect} />{/if}
            {formatAttributes(row.file)}
          </span>
          <span role="gridcell" class="px-2 text-right text-fg-muted tabular-nums">
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
                class="size-6"
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
                  class="size-6"
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
    <div
      class="sticky bottom-2 z-20 mx-auto flex flex-wrap items-center gap-inline rounded-card border bg-overlay px-4 py-2 shadow-raised"
      role="toolbar"
      aria-label="Selected files">
      <span class="text-sm"
        >{plural(chosen.length, "file", "files")} · {formatBytes(chosenBytes)}</span>
      {#if canDownload}
        <Button size="sm" onclick={() => void download(chosen)}><DownloadIcon />Download</Button>
      {/if}
      <Button size="sm" variant="ghost" onclick={() => selected.clear()}>Clear</Button>
    </div>
  {/if}
</div>
