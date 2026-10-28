<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import { Arrivals } from "#lib/arrivals.svelte.js";
  import { cssWidth, parseDuration, sumWidths } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import { COLUMNS, type ColumnId, PHONE_COLUMNS } from "#lib/transfers/columns.js";
  import type { ViewSettings } from "#lib/transfers/settings.svelte.js";
  import type { Direction, Transfer } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Group, Item } from "#lib/transfers/view.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import { remPx, viewport } from "#lib/viewport.svelte.js";

  import GroupHeader from "./GroupHeader.svelte";
  import TransferRow from "./TransferRow.svelte";

  type Props = {
    items: Item[];
    store: Transfers;
    direction: Direction;
    view: ViewSettings;
    /** which groups are open */
    expansion: Expansion;
    /** the selected transfers' ids, which the page acts on */
    selected: SvelteSet<string>;
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
    expansion,
    selected,
    ondetails,
    onremove,
    ondelete,
    oncancel,
    onblock,
    onkey,
    drawerOpen
  }: Props = $props();

  /** a line's height in rem for each density, on a wider screen and on a phone */
  const LINES = {
    comfortable: { wide: 2.25, phone: 3.25 },
    compact: { wide: 1.75, phone: 2.75 }
  };
  /** lines drawn beyond the visible ones, above and below */
  const OVERSCAN = 10;

  let scrollTop = $state(0);
  let height = $state(600);
  let grid = $state<HTMLDivElement | null>(null);
  /** where the rows start in the scrolled content, under the column header */
  let rowsTop = $state(0);
  let resizing: { id: ColumnId; x: number; width: number } | null = null;

  // a phone's row has two lines, the name over its size and progress
  const rem = remPx();
  const row = $derived(rem * LINES[view.density][viewport.phone ? "phone" : "wide"]);
  const columns = $derived(
    view.columns
      .filter((c) => c.visible && !(c.id === "user" && view.groupBy === "user"))
      .filter((c) => !viewport.phone || c.id in PHONE_COLUMNS)
      .map((c) => c.id)
  );
  /** a column's width: a phone's own for the few it shows, else pixels dragged to, else its default */
  const widthOf = (c: { id: ColumnId; dragged?: number }): number | string =>
    (viewport.phone && PHONE_COLUMNS[c.id]) ||
    (c.dragged ?? COLUMNS.find((known) => known.id === c.id)!.width);
  const template = $derived(
    view.columns
      .filter((c) => columns.includes(c.id))
      .map((c) => (c.id === "file" ? `minmax(${cssWidth(widthOf(c))}, 1fr)` : cssWidth(widthOf(c))))
      .join(" ")
  );
  const width = $derived(
    sumWidths(view.columns.filter((c) => columns.includes(c.id)).map(widthOf))
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
  const flat = $derived(view.groupBy === "flat" || view.groupBy === "batch");
  const ids = (group: Group) => group.transfers.map((t) => t.id);
  const ticked = (group: Group) => group.transfers.filter((t) => selected.has(t.id)).length;
  const label = (id: ColumnId) => COLUMNS.find((c) => c.id === id)?.label ?? id;

  // the visible remotely queued downloads ask the peer for their place, each once a period and
  // the first time as soon as it shows; one ask a second at most, so a screenful doesn't ask at once
  const refreshMs = $derived(
    parseDuration(fields(fields(fields(app.options).transfers).ui).queue_refresh) ?? 120_000
  );
  /** when each download was last asked for its place; read by the timer only, never shown */
  const asked = new Map<string, number>();
  const ASK_EVERY_MS = 1_000;
  $effect(() => {
    if (direction !== "download") return;
    const timer = setInterval(() => {
      const now = Date.now();
      const due = items
        .slice(start, end)
        .find(
          (i) =>
            i.type === "row" &&
            i.transfer.state === "Queued, Remotely" &&
            now - (asked.get(i.transfer.id) ?? 0) >= refreshMs
        );
      if (due?.type !== "row") return;
      asked.set(due.transfer.id, now);
      void store.placeInQueue(due.transfer);
    }, ASK_EVERY_MS);
    return () => clearInterval(timer);
  });

  // rows that arrive while the table is on screen make an entrance
  const arrivals = new Arrivals();
  $effect(() => arrivals.see(items.map((i) => i.key)));

  const sortBy = (id: ColumnId) => {
    const key = COLUMNS.find((c) => c.id === id)?.sort;
    if (!key) return;
    view.sort = { id: key, dir: view.sort.id === key && view.sort.dir === "asc" ? "desc" : "asc" };
  };

  const list = new ListControl<Item>({
    lines: () => items,
    key: (item) => item.key,
    id: (item) => (item.type === "row" ? item.transfer.id : null),
    group: (item) => item.group?.transfers.map((t) => t.id) ?? null,
    onopen: (item) => {
      if (item.type === "row") ondetails(item.transfer);
    },
    onkey: (e, item) => {
      if (item.type !== "row") return false;
      const t = item.transfer;
      const key = e.key.toLowerCase();
      if (!session.can("ReadWrite") && ["delete", "r", "c"].includes(key)) return false;
      if (key === "delete") {
        // the selection when the focused row is in it, else the focused row alone
        ondelete(
          selected.size > 1 && selected.has(t.id)
            ? store.list.rows.filter((r) => selected.has(r.id))
            : [t]
        );
      } else if (key === "r") onkey("retry", t);
      else if (key === "c") onkey("cancel", t);
      else if (key === "o") onkey("open", t);
      else return false;
      return true;
    },
    layer: () => drawerOpen,
    // the pinned copy of a group header is rendered only while its group is the pinned one
    rendered: (index, element) =>
      element?.dataset.pinned ? sticky?.key === items[index]?.key : index >= start && index < end,
    // scrolled into view under the column header and the pinned group header
    reveal: (index) => {
      if (!grid) return;
      const top = rowsTop + index * row;
      const pinned = view.groupBy === "flat" ? 0 : row;
      if (top < grid.scrollTop + rowsTop + pinned) grid.scrollTop = top - rowsTop - pinned;
      else if (top + row > grid.scrollTop + height) grid.scrollTop = top + row - height;
    },
    // svelte-ignore state_referenced_locally
    selected
  });

  /** Gives the grid DOM focus, without scrolling */
  export const focus = list.focus;

  /** Whether `element` is inside the table */
  export const has = list.has;

  const activeId = $derived(
    sticky && list.focusedKey === sticky.key ? `${list.uid}-pinned` : list.activeId
  );
</script>

<!-- every row, the header too, has the cells' font with tabular figures, so a column width in ch
     measures the same in each; the header's labels are smaller inside it -->
<div
  class="relative overflow-auto rounded-card border bg-surface text-sm tabular-nums shadow-card outline-none [overflow-anchor:auto]"
  style:height="max(20rem, calc(var(--content-h) - 14rem))"
  role="grid"
  aria-label={direction === "download" ? "Downloads" : "Uploads"}
  aria-rowcount={items.length}
  aria-multiselectable="true"
  aria-activedescendant={activeId}
  tabindex="0"
  bind:this={grid}
  bind:clientHeight={height}
  onscroll={(e) => {
    scrollTop = e.currentTarget.scrollTop;
    list.scrolled();
  }}
  {@attach list.attach}>
  <div class="sticky top-0 z-10" style:min-width={width} bind:offsetHeight={rowsTop}>
    <div
      role="row"
      class="grid h-9 items-center border-b bg-surface-muted text-sm font-medium text-fg-muted"
      style:grid-template-columns={template}>
      {#each columns as id (id)}
        {@const sortable = COLUMNS.find((c) => c.id === id)?.sort}
        <div role="columnheader" class="relative flex min-w-0 items-center px-2 text-xs">
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
                // a drag starts from the width the column has on screen, whatever it was set as
                const cell = e.currentTarget.parentElement!.getBoundingClientRect();
                resizing = { id, x: e.clientX, width: cell.width };
                e.currentTarget.setPointerCapture(e.pointerId);
              }}
              onpointermove={(e) => {
                if (!resizing) return;
                const column = view.columns.find((c) => c.id === resizing!.id)!;
                column.dragged = Math.max(40, resizing.width + e.clientX - resizing.x);
              }}
              onpointerup={() => (resizing = null)}></span>
          {/if}
        </div>
      {/each}
    </div>
    {#if sticky?.type === "group"}
      <div
        class={[
          "absolute inset-x-0 top-full",
          list.focusedKey === sticky.key && "ring-1 ring-accent ring-inset"
        ]}
        id={`${list.uid}-pinned`}
        style:height={`${row}px`}
        data-line={sticky.key}
        data-pinned="true">
        <GroupHeader
          group={sticky.group}
          {store}
          {direction}
          {expansion}
          ticked={ticked(sticky.group)}
          onselect={(on) => list.setAll(ids(sticky.group), on)}
          onremove={() => onremove(sticky.group.transfers)}
          oncancel={() => oncancel(sticky.group.transfers)}
          onblock={(deny) => onblock(sticky.group.username, deny)} />
      </div>
    {/if}
  </div>
  <div
    class="relative"
    style:height={`${items.length * row}px`}
    style:min-width={width}
    role="rowgroup"
    {@attach freezesWhileUsed}>
    {#each items.slice(start, end) as item, i (item.key)}
      <div
        class={[
          "absolute inset-x-0",
          list.isFocused(item) && "ring-1 ring-accent ring-inset",
          item.type === "row" && [
            "group grid items-center border-b hover:bg-hover",
            item.transfer.state.startsWith("Completed, ") &&
              item.transfer.state !== "Completed, Succeeded" &&
              item.transfer.state !== "Completed, Cancelled" &&
              "bg-danger-soft/40",
            selected.has(item.transfer.id) && "bg-accent-soft/60",
            store.list.removed.has(item.transfer.id) && "opacity-40",
            arrivals.isFresh(item.key) && "animate-row-in"
          ]
        ]}
        style:top={`${(start + i) * row}px`}
        style:height={`${row}px`}
        style:grid-template-columns={item.type === "row" ? template : undefined}
        role="row"
        {...list.line(item, start + i)}
        data-id={item.key}>
        {#if item.type === "group"}
          <GroupHeader
            group={item.group}
            {store}
            {direction}
            {expansion}
            ticked={ticked(item.group)}
            onselect={(on) => list.setAll(ids(item.group), on)}
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
            onselect={(range) => list.tick(item.transfer.id, range)}
            ondetails={() => ondetails(item.transfer)}
            onremove={() => onremove([item.transfer])}
            onblock={(deny) => onblock(item.transfer.username, deny)} />
        {/if}
      </div>
    {/each}
  </div>
</div>
