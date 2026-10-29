<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import { Arrivals } from "#lib/arrivals.svelte.js";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import { parseDuration } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import type { Grid } from "#lib/table.svelte.js";
  import type { ColumnId } from "#lib/transfers/columns.js";
  import type { ViewSettings } from "#lib/transfers/settings.svelte.js";
  import type { Direction, Transfer } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Group, Item } from "#lib/transfers/view.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import { rem, viewport } from "#lib/viewport.svelte.js";
  import { ListWindow } from "#lib/virtual.svelte.js";

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
    /** opens the details drawer, at the file's analysis with `analyse` */
    ondetails: (t: Transfer, analyse?: boolean) => void;
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

  /** the scroll position, which decides the group header pinned under the column header */
  let scrollTop = $state(0);
  /** where the rows start in the scrolled content, under the column header */
  let rowsTop = $state(0);

  // a phone's row has two lines, the name over its size and progress
  const row = $derived(rem.px * LINES[view.density][viewport.phone ? "phone" : "wide"]);
  const pinnable = $derived(view.groupBy !== "flat");
  const columns = $derived(grid.columns.map((c) => c.id as ColumnId));
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
    // the pinned copy of a group header is rendered only while its group is the pinned one
    rendered: (index, element) =>
      element?.dataset.pinned ? sticky?.key === items[index]?.key : lines.isDrawn(index),
    reveal: (index) => lines.scrollTo(index),
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
      // a row brought into view stops under the column header and the pinned group header
      scrollPaddingStart: rowsTop + (pinnable ? row : 0),
      sizes: row,
      // a control in a line that scrolled out of the window gives focus back to the list
      onchange: () => list.scrolled()
    })
  );

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
  class="relative overflow-auto rounded-card border bg-surface text-sm tabular-nums shadow-card outline-none"
  style:height="max(20rem, calc(var(--content-h) - 14rem))"
  role="grid"
  aria-label={direction === "download" ? "Downloads" : "Uploads"}
  aria-rowcount={items.length}
  aria-multiselectable="true"
  aria-activedescendant={activeId}
  tabindex="0"
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  {@attach lines.attach}
  {@attach list.attach}>
  <div class="sticky top-0 z-10" style:min-width={grid.width} bind:offsetHeight={rowsTop}>
    <GridHeader {grid} class="h-9 border-b bg-surface-muted text-sm font-medium text-fg-muted" />
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
    style:height={`${lines.total}px`}
    style:min-width={grid.width}
    role="rowgroup"
    {@attach freezesWhileUsed}>
    {#each lines.drawn as { line: item, index, start, size } (item.key)}
      <div
        class={[
          "absolute inset-x-0 top-0",
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
            ondetails={(analyse) => ondetails(item.transfer, analyse)}
            onremove={() => onremove([item.transfer])}
            onblock={(deny) => onblock(item.transfer.username, deny)} />
        {/if}
      </div>
    {/each}
  </div>
</div>
