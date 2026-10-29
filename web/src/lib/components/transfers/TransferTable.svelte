<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import { parseDuration } from "#lib/format.js";
  import { Arrivals } from "#lib/lists/arrivals.svelte.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import type { Grid } from "#lib/lists/table.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import type { ColumnId } from "#lib/transfers/grid/columns.js";
  import { depthOf, nesting, pins } from "#lib/transfers/grid/pinned.js";
  import type { ViewSettings } from "#lib/transfers/grid/settings.svelte.js";
  import type { Group, Item } from "#lib/transfers/grid/view.js";
  import type { Direction, Transfer } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import { rem, viewport } from "#lib/ui/viewport.svelte.js";

  import GroupHeader from "./GroupHeader.svelte";
  import TransferRow from "./TransferRow.svelte";

  type Props = {
    /** the table's lines, from the grid's rows in its order */
    items: Item[];
    /** the columns and the sort */
    grid: Grid<Transfer>;
    store: Transfers;
    direction: Direction;
    view: ViewSettings;
    /** which groups are open */
    expansion: Expansion;
    /** the selected transfers' ids, which the page acts on */
    selected: SvelteSet<string>;
    /** opens the details drawer, at the file's analysis with `analyze` */
    ondetails: (t: Transfer, analyze?: boolean, large?: boolean) => void;
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
    grid,
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

  /** the scroll position, which decides the group headers pinned under the column header */
  let scrollTop = $state(0);
  /** where the rows start in the scrolled content, under the column header */
  let rowsTop = $state(0);
  /** the scroll box's height */
  let boxHeight = $state(0);

  // a phone's row has two lines, the name over its size and progress
  const row = $derived(rem.px * LINES[view.density][viewport.phone ? "phone" : "wide"]);
  const pinnable = $derived(view.groupBy !== "flat");
  const columns = $derived(grid.columns.map((c) => c.id as ColumnId));
  /**
   * A group header's slot under the column header: a line per level, so the user, their folder and
   * its subfolder stack in order. A phone keeps two, the top group and the innermost folder
   */
  const slot = (depth: number) => (viewport.phone ? Math.min(depth, 1) : depth);
  const nest = $derived(nesting(items));
  // the headers of the groups the top lines are in stay at the top while their rows scroll
  const pinned = $derived(
    pinnable
      ? pins(items, nest, scrollTop, row, slot).map((pin) => ({ ...pin, item: items[pin.index] }))
      : []
  );
  const pinnedId = (index: number) => `${list.uid}-pinned-${index}`;
  const flat = $derived(view.groupBy === "flat" || view.groupBy === "batch");
  const ids = (group: Group) => group.transfers.map((t) => t.id);
  const ticked = (group: Group) => group.transfers.filter((t) => selected.has(t.id)).length;

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
      const due = lines.drawn
        .map((drawn) => drawn.line)
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
    // the pinned copy of a group header is rendered only while its group is pinned
    rendered: (index, element) =>
      element?.dataset.pinned ? pinned.some((pin) => pin.index === index) : lines.isDrawn(index),
    reveal: (index) => reveal(index),
    // svelte-ignore state_referenced_locally
    selected
  });

  const lines: ListWindow<Item> = new ListWindow(
    () => items,
    (item) => item.key,
    () => ({
      estimateSize: () => row,
      overscan: 10,
      scrollMargin: rowsTop,
      scrollPaddingStart: rowsTop,
      sizes: row,
      // a control in a line that scrolled out of the window gives focus back to the list
      onchange: () => list.scrolled()
    })
  );

  /**
   * Brings the line at `index` into view, stopping a line brought down under the column header
   * and the group headers that will be pinned over it there: one per level it is in
   */
  const reveal = (index: number) => {
    const start = lines.startOf(index);
    if (start === undefined) return lines.scrollTo(index);
    const depth = depthOf(items[index]);
    const covered = pinnable && depth > 0 ? (slot(depth - 1) + 1) * row : 0;
    const highest = start - rowsTop - covered;
    const lowest = start + row - boxHeight;
    if (lines.offset > highest) lines.scrollToOffset(highest);
    else if (lines.offset < lowest) lines.scrollToOffset(lowest);
  };

  /** Gives the grid DOM focus, without scrolling */
  export const focus = list.focus;

  /** Whether `element` is inside the table */
  export const has = list.has;

  const focusedPin = $derived(pinned.find((pin) => pin.item.key === list.focusedKey));
  const activeId = $derived(focusedPin ? pinnedId(focusedPin.index) : list.activeId);
</script>

<!-- every row, the header too, has the cells' font with tabular figures, so a column width in ch
     measures the same in each; the header's labels are smaller inside it -->
<div
  class="relative overflow-auto rounded-card border bg-surface text-sm tabular-nums shadow-card outline-none"
  style:height="max(20rem, calc(var(--content-h) - 14rem))"
  style:--line-h={`${row}px`}
  role="grid"
  aria-label={direction === "download" ? "Downloads" : "Uploads"}
  aria-rowcount={items.length}
  aria-multiselectable="true"
  aria-activedescendant={activeId}
  tabindex="0"
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  bind:clientHeight={boxHeight}
  {@attach lines.attach}
  {@attach list.attach}>
  <!-- the pinned group headers are drawn under the column header, which a pushed one goes
       under, and over the rows, opaque so no row shows through -->
  <div class="sticky top-0 z-10" style:min-width={grid.width} bind:offsetHeight={rowsTop}>
    <GridHeader {grid} class="h-9 border-b bg-surface-muted text-sm font-medium text-fg-muted" />
    {#each pinned as pin, n (pin.item.key)}
      {#if pin.item.type === "group"}
        {@const group = pin.item.group}
        <div
          class="absolute inset-x-0 list-row bg-surface"
          id={pinnedId(pin.index)}
          style:top={`calc(100% + ${pin.top}px)`}
          style:height={`${row}px`}
          style:z-index={n - pinned.length}
          data-line={pin.item.key}
          data-focused={list.isFocused(pin.item) || undefined}
          data-pinned="true">
          <GroupHeader
            {group}
            {store}
            {direction}
            {expansion}
            ticked={ticked(group)}
            onselect={(on) => list.setAll(ids(group), on)}
            onremove={() => onremove(group.transfers)}
            oncancel={() => oncancel(group.transfers)}
            onblock={(deny) => onblock(group.username, deny)} />
        </div>
      {/if}
    {/each}
  </div>
  <div
    class="relative"
    style:height={`${lines.total}px`}
    style:min-width={grid.width}
    role="rowgroup"
    {@attach freezesWhileUsed}>
    {#each lines.drawn as { line: item, index, start, size } (item.key)}
      <div
        class={[
          "absolute inset-x-0 top-0 list-row",
          item.type === "row" && [
            "group grid items-center hover:bg-hover",
            item.transfer.state.startsWith("Completed, ") &&
              item.transfer.state !== "Completed, Succeeded" &&
              item.transfer.state !== "Completed, Cancelled" &&
              "bg-danger-soft/40",
            selected.has(item.transfer.id) && "bg-accent-soft/60",
            store.list.removed.has(item.transfer.id) && "opacity-40",
            arrivals.isFresh(item.key) && "animate-row-in"
          ]
        ]}
        style:transform={`translateY(${start - rowsTop}px)`}
        style:height={`${size}px`}
        style:grid-template-columns={item.type === "row" ? grid.template : undefined}
        role="row"
        {...list.line(item, index)}
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
            depth={item.group ? item.group.depth + 1 : 0}
            showFolder={flat}
            selected={selected.has(item.transfer.id)}
            speeds={store.speeds.get(item.transfer.id) ?? []}
            around={item.group?.transfers ?? [item.transfer]}
            onselect={(range) => list.tick(item.transfer.id, range)}
            ondetails={(analyze, large) => ondetails(item.transfer, analyze, large)}
            onremove={() => onremove([item.transfer])}
            onblock={(deny) => onblock(item.transfer.username, deny)} />
        {/if}
      </div>
    {/each}
  </div>
</div>
