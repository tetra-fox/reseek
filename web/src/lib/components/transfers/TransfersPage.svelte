<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { SvelteSet } from "svelte/reactivity";

  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { block } from "#lib/components/users/actions.js";
  import { formatBytes, getDirectoryName, plural } from "#lib/format.js";
  import { interaction } from "#lib/interaction.svelte.js";
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
  import {
    type Group,
    inStates,
    type Item,
    layout,
    sortTransfers,
    textFilter
  } from "#lib/transfers/view.js";
  import { users } from "#lib/users.svelte.js";

  import AddDownload from "./AddDownload.svelte";
  import DetailsDrawer from "./DetailsDrawer.svelte";
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
  void store.load();

  let typed = $state("");
  let text = $state("");
  let working = $state(false);
  let adding = $state(false);
  let detailId = $state<string | null>(null);
  let anchor: string | null = null;
  const folded = new SvelteSet<string>();
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

  const all = $derived(store.list.rows);
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
  let shown: Item[] = [];
  const items = $derived.by(() => {
    const fresh = layout(
      sortTransfers(filtered, view.sort.id, view.sort.dir),
      view.groupBy,
      folded
    );
    if (!interaction.frozen || !shown.length) return (shown = fresh);
    const place = new Map(shown.map((item, i) => [item.key, i]));
    return (shown = fresh.toSorted(
      (a, b) => (place.get(a.key) ?? Infinity) - (place.get(b.key) ?? Infinity)
    ));
  });
  const rows = $derived(items.flatMap((i) => (i.type === "row" ? [i.transfer] : [])));
  const chosen = $derived(all.filter((t) => selected.has(t.id)));
  const detail = $derived(detailId ? store.list.get(detailId) : undefined);
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
    opener = table?.has(document.activeElement) ? document.activeElement : null;
    confirm = { title, body, label, run };
    confirmOpen = true;
  };

  /** A confirm closing onto a control whose row left the list gives focus to the table */
  const closeFocus = (e: Event) => {
    if (opener && !opener.isConnected) {
      e.preventDefault();
      table?.focus();
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

  /** A header button: its scope over the filtered transfers, asking first where it should */
  const bulk = (action: BulkAction, test: (state: string) => boolean, scope: string) => {
    const targets = filtered.filter((t) => test(t.state));
    if (
      action === "remove" &&
      scope === "completed" &&
      targets.length === all.filter((t) => test(t.state)).length
    ) {
      // nothing filtered out: the daemon removes every completed transfer in one call
      return void (async () => {
        working = true;
        const outcome = await store.removeCompleted();
        working = false;
        toast(
          outcome.ok ? "Completed transfers removed" : outcome.error,
          outcome.ok ? "info" : "error"
        );
      })();
    }
    if (action === "cancel") return cancel(targets);
    if (action === "remove") return remove(targets);
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

  /** Removes transfers, asking first for more than `many` or any still running */
  const remove = (transfers: Transfer[], many = 10) => {
    const running = activeAmong(transfers);
    ask(
      transfers.length > many || running > 0,
      `Remove ${plural(transfers.length, "transfer", "transfers")}?`,
      running
        ? `${running} still running ${running === 1 ? "is" : "are"} cancelled first.`
        : "They leave the list; their files stay.",
      "Remove",
      () => void runBulk("remove", transfers)
    );
  };

  const blockUser = (username: string, deny: boolean) =>
    ask(
      true,
      deny ? `Deny and block ${username}?` : `Block ${username}?`,
      deny
        ? "Their unfinished uploads are cancelled, then they are blocked with your default block scopes."
        : "They are blocked with your default block scopes and their queued uploads are cancelled.",
      "Block",
      () =>
        void (deny
          ? denyAndBlock(username)
          : block(username, true).then((o) =>
              toast(o.ok ? `${username} is blocked` : o.error, o.ok ? "info" : "error")
            ))
    );

  const select = (t: Transfer, range: boolean) => {
    if (range && anchor) {
      const from = rows.findIndex((r) => r.id === anchor);
      const to = rows.findIndex((r) => r.id === t.id);
      if (from !== -1 && to !== -1) {
        for (const r of rows.slice(Math.min(from, to), Math.max(from, to) + 1)) selected.add(r.id);
        return;
      }
    }
    anchor = t.id;
    if (selected.has(t.id)) selected.delete(t.id);
    else selected.add(t.id);
  };

  const selectGroup = (group: Group, on: boolean) => {
    for (const t of group.transfers) {
      if (on) selected.add(t.id);
      else selected.delete(t.id);
    }
  };

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
</script>

<Toolbar
  {direction}
  {view}
  {counts}
  bind:text={typed}
  disabled={working || !filtered.length}
  onbulk={bulk}
  onadd={() => (adding = true)} />

{#if !store.loaded && store.error}
  <ErrorSegment caption={store.error} retry={() => void store.load()} retryLabel="Retry" />
{:else if !store.loaded}
  <div class="flex justify-center py-10"><Spinner class="size-8" /></div>
{:else if !all.length}
  <EmptyState
    icon={direction === "download" ? DownloadIcon : UploadIcon}
    title={direction === "download" ? "No downloads to display" : "No uploads to display"} />
{:else}
  <TransferTable
    bind:this={table}
    {items}
    {store}
    {direction}
    {view}
    {folded}
    {selected}
    onselect={select}
    onselectgroup={selectGroup}
    ondetails={(t) => (detailId = t.id)}
    onremove={remove}
    ondelete={(transfers) => remove(transfers, 1)}
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
