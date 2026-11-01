<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import { analyzableDownload, analyzeBatch, downloadCandidate } from "#lib/analysis/batch.js";
  import { MARK_FILTERS, type MarkFilter, passes } from "#lib/analysis/marks.js";
  import { analyzerOffered, marks } from "#lib/analysis/marks.svelte.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import TransferInspector from "#lib/components/inspector/TransferInspector.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { formatBytes, getDirectoryName, getFileName, plural } from "#lib/format.js";
  import { inspector, useInspector } from "#lib/inspector/inspector.svelte.js";
  import { heldOrder } from "#lib/lists/interaction.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { useCrumbs } from "#lib/shell/crumbs.svelte.js";
  import { act, denyAndBlock, exportJson, open, openFolder } from "#lib/transfers/actions.js";
  import { activeAmong, type BulkAction, bulkRequest } from "#lib/transfers/bulk.js";
  import { transferGrid } from "#lib/transfers/grid/grid.svelte.js";
  import { views } from "#lib/transfers/grid/settings.svelte.js";
  import {
    inStates,
    type Item,
    layout,
    type MarkLookup,
    textFilter
  } from "#lib/transfers/grid/view.js";
  import {
    type ChipId,
    type Direction,
    rowActions,
    STATE_CHIPS,
    type Transfer
  } from "#lib/transfers/states.js";
  import { downloads, uploads } from "#lib/transfers/transfers.svelte.js";
  import { unseen } from "#lib/transfers/unseen.svelte.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { blocking } from "#lib/users/block.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import AddDownload from "./AddDownload.svelte";
  import MessageAll from "./MessageAll.svelte";
  import Toolbar from "./Toolbar.svelte";
  import TransferTable from "./TransferTable.svelte";

  // the downloads or the uploads screen: one dense table, live by frames and kept current
  type Props = {
    direction: Direction;
    /** a state filter the screen opens with, from a link such as the dashboard's */
    filter?: string | null;
  };
  let { direction, filter = null }: Props = $props();

  // svelte-ignore state_referenced_locally
  const store = direction === "download" ? downloads : uploads;
  // svelte-ignore state_referenced_locally
  const view = views[direction];

  keepCurrent(
    () => void store.load(),
    () => "transfers"
  );
  // a link naming a state filter shows that one alone
  $effect(() => {
    const chip = STATE_CHIPS.find((c) => c.id === filter);
    if (chip) untrack(() => (view.filters.states = [chip.id]));
  });

  // what finishes while the screen is open is seen, so the navigation shows no badge for it
  $effect(() => unseen.view(direction));
  void store.load();

  let typed = $state("");
  let text = $state("");
  let working = $state(false);
  let adding = $state(false);
  let messaging = $state(false);
  /** the transfer the inspector shows, which follows focus in the table */
  let shownId = $state<string | null>(null);
  const folds = new Expansion(true);
  const selected = new SvelteSet<string>();
  let confirm = $state<{ title: string; body: string; label: string; run: () => void } | null>(
    null
  );
  let confirmOpen = $state(false);
  let table = $state<TransferTable | null>(null);

  $effect(() => {
    const value = typed;
    const timer = setTimeout(() => (text = value), 150);
    return () => clearTimeout(timer);
  });

  // a removal that can still be undone has taken its rows off the screen already
  const all = $derived(store.list.rows.filter((t) => !store.hidden.has(t.id)));
  const counts = $derived.by(() => {
    const result = { all: all.length } as Record<ChipId | "all", number>;
    for (const chip of STATE_CHIPS) result[chip.id] = all.filter((t) => chip.test(t.state)).length;
    return result;
  });
  // a download's analysis, which its chip shows and the analysis filters read
  $effect(() => (direction === "download" ? marks.retain() : undefined));
  // svelte-ignore state_referenced_locally
  const markLookup: MarkLookup | undefined =
    direction === "download"
      ? (t) => ({
          mark: marks.download(t.id),
          analyzable: analyzableDownload(t)
        })
      : undefined;
  const marked = (lookup: MarkLookup, filter: MarkFilter) => (t: Transfer) => {
    const found = lookup(t);
    return passes(filter, found.mark, found.analyzable);
  };
  const markCounts = $derived.by(() => {
    if (!markLookup) return null;
    const result = {} as Record<MarkFilter, number>;
    for (const f of MARK_FILTERS) result[f.id] = all.filter(marked(markLookup, f.id)).length;
    return result;
  });
  const filtered = $derived.by(() => {
    const byText = textFilter(text, markLookup);
    const byState = inStates(view.filters.states);
    const byMark =
      markLookup && view.filters.analysis ? marked(markLookup, view.filters.analysis) : null;
    return all.filter((t) => byState(t) && (!byMark || byMark(t)) && (!byText || byText(t)));
  });

  // while the user holds the table still, rows keep their places and new ones go below
  const hold = heldOrder<Item>((item) => item.key);
  // svelte-ignore state_referenced_locally
  const grid = transferGrid(direction, view, () => filtered);
  const items = $derived(hold(layout(grid.rows, view.groupBy, folds.isOpen)));
  const rows = $derived(items.flatMap((i) => (i.type === "row" ? [i.transfer] : [])));
  const chosen = $derived(all.filter((t) => selected.has(t.id)));
  const shown = $derived(
    shownId && !store.hidden.has(shownId) ? store.list.get(shownId) : undefined
  );
  useInspector(inspected, () => !!shown);

  // the users on screen are looked up for their group, color and block
  $effect(() => {
    for (const name of new Set(rows.slice(0, 200).map((t) => t.username))) void users.load(name);
  });

  /** the control in the table a confirm was opened from, which its close returns focus to */
  let opener: Element | null = null;

  /** Asks first when `when` holds, then runs */
  const ask = (when: boolean, title: string, body: string, label: string, run: () => void) => {
    if (!when) return run();
    opener = document.activeElement;
    confirm = { title, body, label, run };
    confirmOpen = true;
  };

  /**
   * A confirm closing onto a control that is gone, as a row that left the list, or disabled, as a
   * toolbar button while its action runs, gives focus to the table instead of the page
   */
  const closeFocus = (e: Event) => {
    const gone =
      !!opener && (!opener.isConnected || (opener instanceof HTMLButtonElement && opener.disabled));
    if (gone && table) {
      e.preventDefault();
      table.focus();
    }
    opener = null;
  };

  const runBulk = async (action: BulkAction, transfers: Transfer[]) => {
    const request = bulkRequest(action, transfers);
    if (!request) return toast(`Nothing to ${action}`);
    working = true;
    await act(store, action, request.ids);
    working = false;
    if (action === "remove") for (const id of request.ids) selected.delete(id);
  };

  /** A header button: its scope over the filtered transfers */
  const bulk = (action: BulkAction, test: (state: string) => boolean, scope: string) => {
    const targets = filtered.filter((t) => test(t.state));
    if (action === "cancel") return cancel(targets);
    if (action === "remove") {
      // nothing filtered out: the daemon removes every completed transfer in one call
      const everything =
        scope === "completed" && targets.length === all.filter((t) => test(t.state)).length;
      return remove(targets, everything);
    }
    void runBulk(action, targets);
  };

  /** Cancels transfers, asking first when any is moving bytes */
  const cancel = (transfers: Transfer[]) => {
    const active = activeAmong(transfers);
    ask(
      active > 0,
      `Cancel ${plural(active, "active transfer", "active transfers")}?`,
      "Any transfer in progress stops immediately.",
      "Cancel transfers",
      () => void runBulk("cancel", transfers)
    );
  };

  /** Removes transfers from the screen at once, and from the daemon once its Undo has passed */
  const remove = (transfers: Transfer[], everything = false) => {
    const ids = transfers.map((t) => t.id);
    for (const id of ids) selected.delete(id);
    store.removeLater(ids, { all: everything });
  };

  /** Opens the block dialog; Deny and block cancels the user's unfinished uploads first */
  const blockUser = (username: string, deny: boolean) =>
    deny ? void denyAndBlock(username) : blocking.ask(username);

  /** A row key, only where the row's own buttons and menu offer the same */
  const onkey = (key: "retry" | "cancel" | "open", t: Transfer) => {
    if (!rowActions(t, direction)[key]) return;
    if (key === "open") return void open(t);
    void runBulk(key, [t]);
  };

  /** Opens the inspector on a transfer */
  const details = (t: Transfer) => {
    shownId = t.id;
    inspector.show();
  };

  const oneFolder = $derived(
    new Set(chosen.map((t) => `${t.username}\u0000${getDirectoryName(t.filename)}`)).size === 1
  );
  const oneUser = $derived(new Set(chosen.map((t) => t.username)).size === 1);
  const write = $derived(session.can("ReadWrite"));

  // with the inspector open, the breadcrumb names the transfer and closes it
  useCrumbs(() =>
    shown && inspector.open
      ? [
          { label: shown.username, onclick: () => inspector.close() },
          { label: getFileName(shown.filename), copy: shown.filename }
        ]
      : []
  );
</script>

{#snippet inspected()}
  {#if shown}
    <TransferInspector transfer={shown} onremove={() => remove([shown])} />
  {/if}
{/snippet}

<Toolbar
  {direction}
  {view}
  {counts}
  {markCounts}
  bind:text={typed}
  disabled={working || !filtered.length}
  onbulk={bulk}
  onadd={() => (adding = true)}
  onmessage={() => (messaging = true)} />

{#if !store.loaded && store.error}
  <ErrorSegment caption={store.error} retry={() => void store.load()} retryLabel="Retry" />
{:else if !store.loaded}
  <div class="flex flex-col gap-px overflow-hidden rounded-card border bg-surface shadow-card">
    {#each [0, 1, 2, 3, 4, 5] as n (n)}
      <div class="flex h-9 items-center gap-3 px-3" aria-hidden="true">
        <Skeleton class="size-4" /><Skeleton class="h-3 flex-1" /><Skeleton class="h-3 w-24" />
      </div>
    {/each}
  </div>
{:else if !all.length}
  <EmptyState
    icon={direction === "download" ? DownloadIcon : UploadIcon}
    title={direction === "download" ? "No downloads yet" : "No uploads yet"}
    description={direction === "download"
      ? "Downloads start from search results, from a user's shares, or from Add with a path or link."
      : undefined} />
{:else}
  <TransferTable
    bind:this={table}
    {items}
    {grid}
    {store}
    {direction}
    {view}
    expansion={folds}
    {selected}
    ondetails={details}
    inspect={(t) => (shownId = t.id)}
    onremove={remove}
    ondelete={remove}
    oncancel={cancel}
    onblock={blockUser}
    {onkey} />
{/if}

{#if selected.size}
  <div class="selection-bar" role="toolbar" aria-label="Selected transfers">
    <span class="text-sm">
      {chosen.length} selected · {formatBytes(chosen.reduce((n, t) => n + t.size, 0))}
    </span>
    {#if write}
      {#if direction === "download"}
        <Button
          size="sm"
          variant="outline"
          disabled={working}
          onclick={() => void runBulk("retry", chosen)}>Retry</Button>
      {/if}
      <Button size="sm" variant="outline" disabled={working} onclick={() => cancel(chosen)}
        >Cancel</Button>
      <Button size="sm" variant="outline" disabled={working} onclick={() => remove(chosen)}
        >Remove</Button>
    {/if}
    {#if write && direction === "download" && analyzerOffered() && chosen.some(analyzableDownload)}
      <Button
        size="sm"
        variant="outline"
        onclick={() => void analyzeBatch(chosen.map(downloadCandidate), "No downloads selected")}
        >Analyze</Button>
    {/if}
    {#if direction === "download" && oneFolder}
      <Button size="sm" variant="outline" onclick={() => openFolder(chosen)}>Open folder</Button>
    {/if}
    {#if write && oneUser}
      <Button size="sm" variant="outline" onclick={() => blockUser(chosen[0].username, false)}>
        Block user
      </Button>
    {/if}
    <Button size="sm" variant="outline" onclick={() => exportJson(chosen)}>Export JSON</Button>
    <Button size="sm" variant="ghost" onclick={() => selected.clear()}>Clear</Button>
  </div>
{/if}

<AddDownload bind:open={adding} />
{#if direction === "upload"}
  <MessageAll bind:open={messaging} usernames={[...new Set(filtered.map((t) => t.username))]} />
{/if}

{#if confirm}
  <ConfirmDialog
    bind:open={confirmOpen}
    title={confirm.title}
    body={confirm.body}
    confirm={confirm.label}
    danger
    onconfirm={confirm.run}
    onclosefocus={closeFocus} />
{/if}
