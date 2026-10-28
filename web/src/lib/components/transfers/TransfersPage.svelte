<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { SvelteSet } from "svelte/reactivity";

  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { blocking } from "#lib/components/users/block.svelte.js";
  import { useCrumbs } from "#lib/crumbs.svelte.js";
  import { formatBytes, getDirectoryName, getFileName, plural } from "#lib/format.js";
  import { heldOrder } from "#lib/interaction.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { act, denyAndBlock, exportJson, open, openFolder } from "#lib/transfers/actions.js";
  import { activeAmong, type BulkAction, bulkRequest } from "#lib/transfers/bulk.js";
  import { views } from "#lib/transfers/settings.svelte.js";
  import {
    type ChipId,
    type Direction,
    rowActions,
    STATE_CHIPS,
    type Transfer
  } from "#lib/transfers/states.js";
  import { downloads, uploads } from "#lib/transfers/transfers.svelte.js";
  import { unseen } from "#lib/transfers/unseen.svelte.js";
  import { inStates, type Item, layout, sortTransfers, textFilter } from "#lib/transfers/view.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { users } from "#lib/users.svelte.js";

  import AddDownload from "./AddDownload.svelte";
  import DetailsDrawer from "./DetailsDrawer.svelte";
  import MessageAll from "./MessageAll.svelte";
  import Toolbar from "./Toolbar.svelte";
  import TransferTable from "./TransferTable.svelte";

  // the downloads or the uploads screen: one dense table, live by frames and kept current
  let { direction }: { direction: Direction } = $props();

  // svelte-ignore state_referenced_locally
  const store = direction === "download" ? downloads : uploads;
  // svelte-ignore state_referenced_locally
  const view = views[direction];

  keepCurrent(
    () => void store.load(),
    () => "transfers"
  );
  // what finishes while the screen is open is seen, so the navigation shows no badge for it
  $effect(() => unseen.seen(direction));
  void store.load();

  let typed = $state("");
  let text = $state("");
  let working = $state(false);
  let adding = $state(false);
  let messaging = $state(false);
  let detailId = $state<string | null>(null);
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
  const filtered = $derived.by(() => {
    const byText = textFilter(text);
    const byState = inStates(view.filters.states);
    return all.filter((t) => byState(t) && (!byText || byText(t)));
  });

  // while the user holds the table still, rows keep their places and new ones go below
  const hold = heldOrder<Item>((item) => item.key);
  const items = $derived(
    hold(layout(sortTransfers(filtered, view.sort.id, view.sort.dir), view.groupBy, folds.isOpen))
  );
  const rows = $derived(items.flatMap((i) => (i.type === "row" ? [i.transfer] : [])));
  const chosen = $derived(all.filter((t) => selected.has(t.id)));
  const detail = $derived(
    detailId && !store.hidden.has(detailId) ? store.list.get(detailId) : undefined
  );
  const detailAround = $derived(
    detail
      ? all.filter(
          (t) =>
            getDirectoryName(t.filename) === getDirectoryName(detail.filename) &&
            t.username === detail.username
        )
      : []
  );

  // the users on screen are looked up for their group, colour and block
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
      "They stop where they are.",
      "Cancel them",
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

  const step = (by: -1 | 1) => {
    const at = rows.findIndex((r) => r.id === detailId);
    const next = rows[at + by];
    if (next) detailId = next.id;
  };

  const oneFolder = $derived(
    new Set(chosen.map((t) => `${t.username}\u0000${getDirectoryName(t.filename)}`)).size === 1
  );
  const oneUser = $derived(new Set(chosen.map((t) => t.username)).size === 1);
  const write = $derived(session.can("ReadWrite"));

  // with the details open, the breadcrumb names the transfer and goes back to the list
  useCrumbs(() =>
    detail
      ? [
          { label: detail.username, onclick: () => (detailId = null) },
          { label: getFileName(detail.filename), copy: detail.filename }
        ]
      : []
  );
</script>

<Toolbar
  {direction}
  {view}
  {counts}
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
    title={direction === "download" ? "No downloads to display" : "No uploads to display"}
    description={direction === "download"
      ? "Search for something, or browse a user's shares, and download from there; Add takes a path or a link."
      : "Uploads show here when other users download what you share."} />
{:else}
  <TransferTable
    bind:this={table}
    {items}
    {store}
    {direction}
    {view}
    expansion={folds}
    {selected}
    ondetails={(t) => (detailId = t.id)}
    onremove={remove}
    ondelete={remove}
    oncancel={cancel}
    onblock={blockUser}
    {onkey}
    drawerOpen={!!detail} />
{/if}

{#if selected.size}
  <div
    class="sticky bottom-12 z-20 mx-auto flex flex-wrap items-center gap-inline rounded-card border bg-overlay px-4 py-2 shadow-raised"
    role="toolbar"
    aria-label="Selected transfers">
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

{#if detail}
  <DetailsDrawer
    transfer={detail}
    {store}
    {direction}
    around={detailAround}
    onclose={() => (detailId = null)}
    onstep={step}
    onremove={() => remove([detail])}
    onblock={(deny) => blockUser(detail.username, deny)} />
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
