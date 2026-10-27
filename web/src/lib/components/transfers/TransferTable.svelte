<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import { parseDuration } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { keepsKey, layerOpen, withModifier } from "#lib/keys.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import { COLUMNS, type ColumnId } from "#lib/transfers/columns.js";
  import type { ViewSettings } from "#lib/transfers/settings.svelte.js";
  import type { Direction, Transfer } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Group, Item } from "#lib/transfers/view.js";

  import GroupHeader from "./GroupHeader.svelte";
  import TransferRow from "./TransferRow.svelte";

  type Props = {
    items: Item[];
    store: Transfers;
    direction: Direction;
    view: ViewSettings;
    folded: SvelteSet<string>;
    selected: SvelteSet<string>;
    /** ticks a row; with `range` every row from the last ticked one to it */
    onselect: (t: Transfer, range: boolean) => void;
    onselectgroup: (group: Group, on: boolean) => void;
    ondetails: (t: Transfer) => void;
    onremove: (transfers: Transfer[]) => void;
    /** removes with the Delete key, which asks first for more than one */
    ondelete: (transfers: Transfer[]) => void;
    oncancel: (transfers: Transfer[]) => void;
    onblock: (username: string, deny: boolean) => void;
    /** a key the row with focus acts on: retry, cancel, open */
    onkey: (key: "retry" | "cancel" | "open", t: Transfer) => void;
    /** the details drawer is open, so Escape is the drawer's */
    drawerOpen: boolean;
  };

  let {
    items,
    store,
    direction,
    view,
    folded,
    selected,
    onselect,
    onselectgroup,
    ondetails,
    onremove,
    ondelete,
    oncancel,
    onblock,
    onkey,
    drawerOpen
  }: Props = $props();

  const uid = $props.id();

  /** lines drawn beyond the visible ones, above and below */
  const OVERSCAN = 10;

  let scrollTop = $state(0);
  let height = $state(600);
  let grid = $state<HTMLDivElement | null>(null);
  /** where the rows start in the scrolled content, under the column header */
  let rowsTop = $state(0);
  let resizing: { id: ColumnId; x: number; width: number } | null = null;

  const row = $derived(view.density === "compact" ? 28 : 36);
  const columns = $derived(
    view.columns
      .filter((c) => c.visible && !(c.id === "user" && view.groupBy === "user"))
      .map((c) => c.id)
  );
  const template = $derived(
    view.columns
      .filter((c) => columns.includes(c.id))
      .map((c) => (c.id === "file" ? `minmax(${c.width}px, 1fr)` : `${c.width}px`))
      .join(" ")
  );
  const width = $derived(
    view.columns.filter((c) => columns.includes(c.id)).reduce((n, c) => n + c.width, 0)
  );
  const start = $derived(Math.max(0, Math.floor(scrollTop / row) - OVERSCAN));
  const end = $derived(Math.min(items.length, Math.ceil((scrollTop + height) / row) + OVERSCAN));
  // the header of the group the top line belongs to stays at the top while its rows scroll:
  // from when its own header starts to go under the column header
  const sticky = $derived.by(() => {
    const top = Math.min(Math.floor(scrollTop / row), items.length - 1);
    for (let i = top; i >= 0; i--) {
      const item = items[i];
      if (item.type === "group" && item.group.depth === 0) return i * row < scrollTop ? item : null;
    }
    return null;
  });
  const rows = $derived(items.flatMap((i) => (i.type === "row" ? [i.transfer] : [])));
  const flat = $derived(view.groupBy === "flat" || view.groupBy === "batch");
  const ticked = (group: Group) => group.transfers.filter((t) => selected.has(t.id)).length;
  const label = (id: ColumnId) => COLUMNS.find((c) => c.id === id)?.label ?? id;

  // the visible remotely queued downloads ask the peer for their place now and then, spread
  // over the period so they don't all ask at once
  const refreshMs = $derived(
    parseDuration(fields(fields(fields(app.options).transfers).ui).queue_refresh) ?? 120_000
  );
  // as one string, so the timers below start over only when the set of rows changes
  const queued = $derived(
    direction === "download"
      ? items
          .slice(start, end)
          .flatMap((i) =>
            i.type === "row" && i.transfer.state === "Queued, Remotely" ? [i.transfer.id] : []
          )
          .join(" ")
      : ""
  );
  $effect(() => {
    const due = queued ? queued.split(" ") : [];
    const period = refreshMs;
    if (!due.length) return;
    const timers: ReturnType<typeof setTimeout>[] = [];
    const ask = (id: string) => {
      const t = store.list.get(id);
      if (t?.state === "Queued, Remotely") void store.placeInQueue(t);
    };
    const round = setInterval(() => {
      due.forEach((id, n) => timers.push(setTimeout(() => ask(id), (n * period) / due.length)));
    }, period);
    return () => {
      clearInterval(round);
      for (const timer of timers) clearTimeout(timer);
    };
  });

  const sortBy = (id: ColumnId) => {
    const key = COLUMNS.find((c) => c.id === id)?.sort;
    if (!key) return;
    view.sort = { id: key, dir: view.sort.id === key && view.sort.dir === "asc" ? "desc" : "asc" };
  };

  /*
   * The focus model. The grid element keeps DOM focus; rows are not focusable, and a control
   * that had focus hands it back to the grid when the table scrolls. The focused line, a row or
   * a group header, is named by its key in `items` and looked up there whenever it is used, so
   * filters, regrouping and new frames never leave it pointing at a stale copy. When that line
   * leaves `items`, the line now at its last index takes over. Moving is by position in `items`:
   * down to the next row, up to the previous one, skipping headers. `aria-activedescendant`
   * names the focused line only while it is rendered.
   */
  let focusedKey = $state<string | null>(null);
  /** where the focused line was last seen, for when it leaves */
  let lastIndex = 0;
  const focusIndex = $derived.by(() => {
    if (focusedKey === null || !items.length) return -1;
    const at = items.findIndex((i) => i.key === focusedKey);
    if (at !== -1) lastIndex = at;
    return at !== -1 ? at : Math.min(lastIndex, items.length - 1);
  });
  const focusedItem = $derived(focusIndex === -1 ? null : items[focusIndex]);
  // a line that left hands the focus to the one now in its place
  $effect(() => {
    if (focusedItem && focusedItem.key !== focusedKey) focusedKey = focusedItem.key;
  });
  const activeId = $derived(
    focusIndex >= start && focusIndex < end ? `${uid}-${focusIndex}` : undefined
  );

  /** Makes the line `target` is in the focused one: a click or a focus there moves the focus to it */
  const takeLine = (target: EventTarget | null) => {
    const line =
      target instanceof Element ? target.closest<HTMLElement>("[data-line]")?.dataset.line : null;
    if (line) focusedKey = line;
  };

  /** Focuses the row at `index` in `items`, back on the grid, scrolled into view under the headers */
  const moveTo = (index: number) => {
    const item = items[index];
    if (item?.type !== "row" || !grid) return;
    focusedKey = item.key;
    if (document.activeElement !== grid) grid.focus({ preventScroll: true });
    const top = rowsTop + index * row;
    const pinned = view.groupBy === "flat" ? 0 : row;
    if (top < grid.scrollTop + rowsTop + pinned) grid.scrollTop = top - rowsTop - pinned;
    else if (top + row > grid.scrollTop + height) grid.scrollTop = top + row - height;
  };

  /** The index of the nearest row after (`step` 1) or before (-1) `from`, -1 when none */
  const nextRow = (from: number, step: 1 | -1) => {
    for (let i = from + step; i >= 0 && i < items.length; i += step) {
      if (items[i].type === "row") return i;
    }
    return -1;
  };

  /** A control in a row that kept focus gives it back to the grid before its row scrolls away */
  const scrolled = (e: Event & { currentTarget: HTMLDivElement }) => {
    scrollTop = e.currentTarget.scrollTop;
    const active = document.activeElement;
    if (active && active !== grid && grid?.contains(active)) grid.focus({ preventScroll: true });
  };

  /**
   * The table's keys. A focused control keeps the keys it handles (`keepsKey`); the rest act on
   * the line the control is in, else on the focused line. On a row every key acts; on a group
   * header only Ctrl/Cmd+A and Space, which select the group
   */
  const keydown = (e: KeyboardEvent) => {
    if (keepsKey(e)) return;
    takeLine(e.target);
    const item = focusedItem;
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "a") {
      const group = item?.type === "group" ? item.group : (item?.group ?? null);
      for (const t of group?.transfers ?? rows) selected.add(t.id);
    } else if (withModifier(e)) return;
    else if (e.key === "ArrowDown") {
      const to = nextRow(focusIndex, 1);
      if (to !== -1) moveTo(to);
    } else if (e.key === "ArrowUp") {
      const to = focusIndex === -1 ? nextRow(-1, 1) : nextRow(focusIndex, -1);
      if (to !== -1) moveTo(to);
    } else if (e.key === "Escape") {
      // the drawer, a menu, a dialog or a card closes first; an Escape that closes nothing clears
      if (drawerOpen || layerOpen()) return;
      selected.clear();
    } else if (!item) return;
    else if (item.type === "group") {
      if (e.key !== " ") return;
      const all = item.group.transfers.every((t) => selected.has(t.id));
      onselectgroup(item.group, !all);
    } else if (e.key === " ") onselect(item.transfer, false);
    else if (e.key === "Enter") ondetails(item.transfer);
    else if (!session.can("ReadWrite") && ["Delete", "r", "R", "c", "C"].includes(e.key)) return;
    else if (e.key === "Delete")
      ondelete(selected.size > 1 ? rows.filter((t) => selected.has(t.id)) : [item.transfer]);
    else if (e.key === "r" || e.key === "R") onkey("retry", item.transfer);
    else if (e.key === "c" || e.key === "C") onkey("cancel", item.transfer);
    else if (e.key === "o" || e.key === "O") onkey("open", item.transfer);
    else return;
    e.preventDefault();
  };
</script>

<div
  class="relative overflow-auto rounded-card border bg-surface text-sm shadow-card outline-none [overflow-anchor:auto]"
  style:height="max(20rem, calc(100svh - 20rem))"
  role="grid"
  aria-label={direction === "download" ? "Downloads" : "Uploads"}
  aria-rowcount={items.length}
  aria-multiselectable="true"
  aria-activedescendant={activeId}
  tabindex="0"
  bind:this={grid}
  bind:clientHeight={height}
  onscroll={scrolled}
  onpointerdown={(e) => takeLine(e.target)}
  onfocusin={(e) => takeLine(e.target)}
  onkeydown={keydown}>
  <div class="sticky top-0 z-10" style:min-width={`${width}px`} bind:offsetHeight={rowsTop}>
    <div
      role="row"
      class="grid h-9 items-center border-b bg-surface-muted text-xs font-medium text-fg-muted"
      style:grid-template-columns={template}>
      {#each columns as id (id)}
        {@const sortable = COLUMNS.find((c) => c.id === id)?.sort}
        <div role="columnheader" class="relative flex min-w-0 items-center px-2">
          {#if id === "select" || id === "actions"}
            <span class="sr-only">{label(id)}</span>
          {:else if sortable}
            <button
              type="button"
              class="flex min-w-0 items-center gap-1 truncate"
              onclick={() => sortBy(id)}>
              {label(id)}
              {#if view.sort.id === sortable}
                {#if view.sort.dir === "asc"}<ArrowUpIcon class="size-3" />{:else}<ArrowDownIcon
                    class="size-3" />{/if}
              {/if}
            </button>
          {:else}
            <span class="truncate">{label(id)}</span>
          {/if}
          {#if id !== "select" && id !== "actions"}
            <!-- dragging the edge sets the column's width, which is kept -->
            <span
              class="absolute top-1 right-0 bottom-1 w-1.5 cursor-col-resize border-r"
              role="separator"
              aria-label={`Width of ${label(id)}`}
              onpointerdown={(e) => {
                const column = view.columns.find((c) => c.id === id)!;
                resizing = { id, x: e.clientX, width: column.width };
                e.currentTarget.setPointerCapture(e.pointerId);
              }}
              onpointermove={(e) => {
                if (!resizing) return;
                const column = view.columns.find((c) => c.id === resizing!.id)!;
                column.width = Math.max(40, resizing.width + e.clientX - resizing.x);
              }}
              onpointerup={() => (resizing = null)}></span>
          {/if}
        </div>
      {/each}
    </div>
    {#if sticky?.type === "group"}
      <div class="absolute inset-x-0 top-full" style:height={`${row}px`} data-line={sticky.key}>
        <GroupHeader
          group={sticky.group}
          {store}
          {direction}
          folded={folded.has(sticky.key)}
          ticked={ticked(sticky.group)}
          ontoggle={() =>
            folded.has(sticky.key) ? folded.delete(sticky.key) : folded.add(sticky.key)}
          onselect={(on) => onselectgroup(sticky.group, on)}
          onremove={() => onremove(sticky.group.transfers)}
          oncancel={() => oncancel(sticky.group.transfers)}
          onblock={(deny) => onblock(sticky.group.username, deny)} />
      </div>
    {/if}
  </div>
  <div
    class="relative"
    style:height={`${items.length * row}px`}
    style:min-width={`${width}px`}
    role="rowgroup"
    {@attach freezesWhileUsed}>
    {#each items.slice(start, end) as item, i (item.key)}
      <div
        class={[
          "absolute inset-x-0",
          focusedKey === item.key && "ring-1 ring-accent ring-inset",
          item.type === "row" && [
            "group grid items-center border-b hover:bg-hover",
            item.transfer.state.startsWith("Completed, ") &&
              item.transfer.state !== "Completed, Succeeded" &&
              item.transfer.state !== "Completed, Cancelled" &&
              "bg-danger-soft/40",
            selected.has(item.transfer.id) && "bg-accent-soft/60",
            store.list.removed.has(item.transfer.id) && "opacity-40"
          ]
        ]}
        style:top={`${(start + i) * row}px`}
        style:height={`${row}px`}
        style:grid-template-columns={item.type === "row" ? template : undefined}
        role="row"
        id={`${uid}-${start + i}`}
        aria-selected={item.type === "row" ? selected.has(item.transfer.id) : undefined}
        data-id={item.key}
        data-line={item.key}>
        {#if item.type === "group"}
          <GroupHeader
            group={item.group}
            {store}
            {direction}
            folded={folded.has(item.key)}
            ticked={ticked(item.group)}
            ontoggle={() => (folded.has(item.key) ? folded.delete(item.key) : folded.add(item.key))}
            onselect={(on) => onselectgroup(item.group, on)}
            onremove={() => onremove(item.group.transfers)}
            oncancel={() => oncancel(item.group.transfers)}
            onblock={(deny) => onblock(item.group.username, deny)} />
        {:else}
          <TransferRow
            transfer={item.transfer}
            {direction}
            {columns}
            showFolder={flat}
            selected={selected.has(item.transfer.id)}
            speeds={store.speeds.get(item.transfer.id) ?? []}
            around={item.group?.transfers ?? [item.transfer]}
            onselect={(range) => onselect(item.transfer, range)}
            ondetails={() => ondetails(item.transfer)}
            onremove={() => onremove([item.transfer])}
            onblock={(deny) => onblock(item.transfer.username, deny)} />
        {/if}
      </div>
    {/each}
  </div>
</div>
