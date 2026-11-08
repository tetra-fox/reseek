<script lang="ts">
  import type { SvelteSet } from "svelte/reactivity";

  import { rowAnalysis } from "#lib/analysis/batch.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import { app } from "#lib/app.svelte.js";
  import FileTree from "#lib/components/files/FileTree.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import UserMenuItems from "#lib/components/users/UserMenuItems.svelte";
  import { getFileName, parseDuration } from "#lib/format.js";
  import type { Grid } from "#lib/lists/table.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import { type ColumnId, fileInset, MARK_REM } from "#lib/transfers/grid/columns.js";
  import type { ViewSettings } from "#lib/transfers/grid/settings.svelte.js";
  import { type Group, groupingOf, type Item, treeOf } from "#lib/transfers/grid/view.js";
  import { type GroupHandlers, groupActions } from "#lib/transfers/groupActions.js";
  import type { Direction, Transfer } from "#lib/transfers/states.js";
  import type { Transfers } from "#lib/transfers/transfers.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import type { TreeLine } from "#lib/tree/folders.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  import GroupHeader from "./GroupHeader.svelte";
  import GroupItems from "./GroupItems.svelte";
  import TransferItems from "./TransferItems.svelte";
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
    /** opens the inspector on a transfer */
    ondetails: (t: Transfer) => void;
    /** focus came to a transfer, which the inspector shows */
    inspect: (t: Transfer) => void;
    onremove: (transfers: Transfer[]) => void;
    /** removes with the Delete key, which asks first for more than one */
    ondelete: (transfers: Transfer[]) => void;
    oncancel: (transfers: Transfer[]) => void;
    onblock: (username: string, deny: boolean) => void;
    /** a key the row with focus acts on: retry, cancel, open */
    onkey: (key: "retry" | "cancel" | "open", t: Transfer) => void;
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
    inspect,
    onremove,
    ondelete,
    oncancel,
    onblock,
    onkey
  }: Props = $props();

  const columns = $derived(grid.columns.map((c) => c.id as ColumnId));

  // what a row's and a group's actions ask of this list, the same from their buttons and menus
  const rowHandlers = (t: Transfer) => ({
    ondetails: () => ondetails(t),
    onremove: () => onremove([t]),
    onblock: (deny: boolean) => onblock(t.username, deny)
  });
  const groupHandlers = (group: Group): GroupHandlers => ({
    onremove: () => onremove(group.transfers),
    oncancel: () => oncancel(group.transfers),
    onblock: (deny) => onblock(group.username, deny)
  });
  const actionsOf = (group: Group) => groupActions(group, store, direction, groupHandlers(group));
  /** a user's or a folder's line always has the user's items; a job's only its own actions */
  const menuable = (line: TreeLine<Transfer>) => {
    if (line.kind === "item") return true;
    const group = tree.groups.get(line.node.key);
    return !!group && (group.kind !== "job" || actionsOf(group).length > 0);
  };
  /**
   * A group header's slot in the pinned stack: a line per level, so the user, their folder and its
   * subfolder stack in order. A phone keeps two, the top group and the innermost folder
   */
  const slot = (depth: number) => (viewport.phone ? Math.min(depth, 1) : depth);
  const tree = $derived(treeOf(items));
  /**
   * what the File column's least width adds to its own: the inset of the deepest row, and room
   * for an analysis mark when a row shows one
   */
  const fileRoom = $derived.by(() => {
    let deepest = 0;
    let marked = false;
    for (const i of items) {
      if (i.type === "group") deepest = Math.max(deepest, i.group.depth + 1);
      else if (direction === "download" && marks.download(i.transfer.id)) marked = true;
    }
    return fileInset(deepest) + (marked ? MARK_REM : 0);
  });
  /** rows that no folder line is over show their folder */
  const flat = $derived(groupingOf(view) === null || groupingOf(view) === "job");
  const ids = (group: Group) => group.transfers.map((t) => t.id);
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
<div class="contents" style:--file-room={`${fileRoom}rem`}>
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
    {menuable}
    {slot}
    framed={false}
    class="tabular-nums"
    {inspect}
    onopen={(t) => ondetails(t)}
    onkey={actOnKey}
    lineClass={(line) =>
      line.kind === "item" && store.list.removed.has(line.item.id) ? "opacity-40" : []}>
    {#snippet header(all)}
      <GridHeader {grid} {all} class="h-9 border-b bg-surface-muted text-sm text-fg-muted" />
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
          {...groupHandlers(group)} />
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
        onselect={tick}
        {...rowHandlers(t)} />
    {/snippet}
    <!-- a right-click, a long press or the menu key opens what the line's buttons offer -->
    {#snippet menu(line)}
      {#if line.kind === "item"}
        <TransferItems
          transfer={line.item}
          {direction}
          {...rowHandlers(line.item)}
          analysis={rowAnalysis(line.item, direction)} />
      {:else}
        {@const group = tree.groups.get(line.node.key)}
        {#if group}
          {@const actions = actionsOf(group)}
          <GroupItems {actions} />
          {#if group.kind !== "job"}
            {#if actions.length}<Menu.Separator />{/if}
            <UserMenuItems name={group.username} named />
          {/if}
        {/if}
      {/if}
    {/snippet}
  </FileTree>
</div>
