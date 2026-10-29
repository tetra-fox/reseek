<script lang="ts">
  import type { SvelteSet } from "svelte/reactivity";

  import { app } from "#lib/app.svelte.js";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import { getFileName, parseDuration } from "#lib/format.js";
  import type { Grid } from "#lib/lists/table.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import type { ColumnId } from "#lib/transfers/grid/columns.js";
  import type { ViewSettings } from "#lib/transfers/grid/settings.svelte.js";
  import { type Group, type Item, treeOf } from "#lib/transfers/grid/view.js";
  import type { Direction, Transfer } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import type { TreeLine } from "#lib/tree/folders.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

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

  const columns = $derived(grid.columns.map((c) => c.id as ColumnId));
  /**
   * A group header's slot in the pinned stack: a line per level, so the user, their folder and its
   * subfolder stack in order. A phone keeps two, the top group and the innermost folder
   */
  const slot = (depth: number) => (viewport.phone ? Math.min(depth, 1) : depth);
  const tree = $derived(treeOf(items));
  const flat = $derived(view.groupBy === "flat" || view.groupBy === "batch");
  const ids = (group: Group) => group.transfers.map((t) => t.id);
  /** the group each grouped row is in, whose files play after it */
  const groupOf = $derived(
    new Map(items.flatMap((i) => (i.type === "row" && i.group ? [[i.transfer.id, i.group]] : [])))
  );
  const ticked = (group: Group) => group.transfers.filter((t) => selected.has(t.id)).length;

  let table = $state<ReturnType<typeof FileTree<Transfer>> | null>(null);

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
      const due = (table?.drawn() ?? []).find(
        (line) =>
          line.kind === "item" &&
          line.item.state === "Queued, Remotely" &&
          now - (asked.get(line.item.id) ?? 0) >= refreshMs
      );
      if (due?.kind !== "item") return;
      asked.set(due.item.id, now);
      void store.placeInQueue(due.item);
    }, ASK_EVERY_MS);
    return () => clearInterval(timer);
  });

  /** A key the focused row acts on: Delete removes, R retries, C cancels, O opens */
  const actOnKey = (e: KeyboardEvent, line: TreeLine<Transfer>) => {
    if (line.kind !== "item") return false;
    const t = line.item;
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
  };

  /** Gives the grid DOM focus, without scrolling */
  export const focus = () => table?.focus();

  /** Whether `element` is inside the table */
  export const has = (element: Element | null) => !!element && !!table?.has(element);
</script>

<!-- every row, the header too, has the cells' font with tabular figures, so a column width in ch
     measures the same in each; the header's labels are smaller inside it -->
<FileTree
  bind:this={table}
  lines={tree.lines}
  {expansion}
  label={direction === "download" ? "Downloads" : "Uploads"}
  columns={grid.template}
  width={grid.width}
  {selected}
  idOf={(t) => t.id}
  nameOf={(line) => (line.kind === "folder" ? line.node.label : getFileName(line.item.filename))}
  lineRem={LINES[view.density]}
  {slot}
  framed={false}
  class="tabular-nums shadow-card"
  layer={() => drawerOpen}
  onopen={(t) => ondetails(t)}
  onkey={actOnKey}
  lineClass={(line) =>
    line.kind === "item" && store.list.removed.has(line.item.id) ? "opacity-40" : []}>
  {#snippet header()}
    <GridHeader {grid} class="h-9 border-b bg-surface-muted text-sm font-medium text-fg-muted" />
  {/snippet}
  {#snippet folder(node)}
    {@const group = tree.groups.get(node.key)}
    {#if group}
      <GroupHeader
        {group}
        {store}
        {direction}
        {expansion}
        ticked={ticked(group)}
        onselect={(on) => {
          for (const id of ids(group)) {
            if (on) selected.add(id);
            else selected.delete(id);
          }
        }}
        onremove={() => onremove(group.transfers)}
        oncancel={() => oncancel(group.transfers)}
        onblock={(deny) => onblock(group.username, deny)} />
    {/if}
  {/snippet}
  {#snippet item(t, depth, tick)}
    <TransferRow
      transfer={t}
      {direction}
      {columns}
      {depth}
      showFolder={flat}
      selected={selected.has(t.id)}
      speeds={store.speeds.get(t.id) ?? []}
      around={groupOf.get(t.id)?.transfers ?? [t]}
      onselect={tick}
      ondetails={(analyze, large) => ondetails(t, analyze, large)}
      onremove={() => onremove([t])}
      onblock={(deny) => onblock(t.username, deny)} />
  {/snippet}
</FileTree>
