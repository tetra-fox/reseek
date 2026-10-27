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

  /** lines drawn beyond the visible ones, above and below */
  const OVERSCAN = 10;

  let scrollTop = $state(0);
  let height = $state(600);
  /** the row the keys act on, and the group Ctrl/Cmd+A selects in */
  let focused = $state<string | null>(null);
  let scope = $state.raw<Group | null>(null);
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

  /** Makes the line with `key` the one the keys act on: a row, or a group header's first row */
  const focusLine = (key: string) => {
    const item = items.find((i) => i.key === key);
    if (item?.type === "row") {
      focused = item.transfer.id;
      scope = item.group;
    } else if (item?.type === "group") {
      focused = item.group.transfers[0]?.id ?? focused;
      scope = item.group;
    }
  };

  const lineOf = (target: EventTarget | null) =>
    target instanceof Element
      ? target.closest<HTMLElement>("[data-line]")?.dataset.line
      : undefined;

  /** Moves the keys to row `to`, back on the table itself, scrolled into view below the headers */
  const move = (to: number) => {
    const next = rows[Math.max(0, Math.min(rows.length - 1, to))];
    if (!next || !grid) return;
    focusLine(next.id);
    if (document.activeElement !== grid) grid.focus({ preventScroll: true });
    const top = rowsTop + items.findIndex((i) => i.key === next.id) * row;
    const pinned = view.groupBy === "flat" ? 0 : row;
    if (top < grid.scrollTop + rowsTop + pinned) grid.scrollTop = top - rowsTop - pinned;
    else if (top + row > grid.scrollTop + height) grid.scrollTop = top + row - height;
  };

  /**
   * The table's keys. A focused control keeps the keys it handles (`keepsKey`); the rest act on
   * the line the control is in, or on the focused row
   */
  const keydown = (e: KeyboardEvent) => {
    if (keepsKey(e.target, e.key)) return;
    const line = lineOf(e.target);
    if (line) focusLine(line);
    const at = rows.findIndex((t) => t.id === focused);
    const current = rows[at];
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "a") {
      for (const t of scope?.transfers ?? rows) selected.add(t.id);
    } else if (withModifier(e)) return;
    else if (e.key === "ArrowDown") move(at + 1);
    else if (e.key === "ArrowUp") move(at === -1 ? 0 : at - 1);
    else if (e.key === "Escape") {
      // the drawer, a menu, a dialog or a card closes first; an Escape that closes nothing clears
      if (drawerOpen || layerOpen()) return;
      selected.clear();
    } else if (!current) return;
    else if (e.key === " ") onselect(current, false);
    else if (e.key === "Enter") ondetails(current);
    else if (!session.can("ReadWrite") && ["Delete", "r", "R", "c", "C"].includes(e.key)) return;
    else if (e.key === "Delete")
      ondelete(selected.size > 1 ? rows.filter((t) => selected.has(t.id)) : [current]);
    else if (e.key === "r" || e.key === "R") onkey("retry", current);
    else if (e.key === "c" || e.key === "C") onkey("cancel", current);
    else if (e.key === "o" || e.key === "O") onkey("open", current);
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
  aria-activedescendant={focused ? `transfer-${focused}` : undefined}
  tabindex="0"
  bind:this={grid}
  bind:clientHeight={height}
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
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
          item.type === "row" && [
            "group grid items-center border-b hover:bg-hover",
            item.transfer.state.startsWith("Completed, ") &&
              item.transfer.state !== "Completed, Succeeded" &&
              item.transfer.state !== "Completed, Cancelled" &&
              "bg-danger-soft/40",
            selected.has(item.transfer.id) && "bg-accent-soft/60",
            focused === item.transfer.id && "ring-1 ring-accent ring-inset",
            store.list.removed.has(item.transfer.id) && "opacity-40"
          ]
        ]}
        style:top={`${(start + i) * row}px`}
        style:height={`${row}px`}
        style:grid-template-columns={item.type === "row" ? template : undefined}
        role="row"
        tabindex="-1"
        id={item.type === "row" ? `transfer-${item.key}` : undefined}
        aria-selected={item.type === "row" ? selected.has(item.transfer.id) : undefined}
        data-id={item.key}
        data-line={item.key}
        onpointerdown={() => focusLine(item.key)}
        onfocusin={() => focusLine(item.key)}>
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
