<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import RepeatIcon from "@lucide/svelte/icons/repeat";
  import { untrack } from "svelte";

  import { api, byUser } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import TransferRow from "#lib/components/transfers/TransferRow.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { act, storeOf } from "#lib/transfers/actions.js";
  import { type ColumnId, templateOf } from "#lib/transfers/grid/columns.js";
  import { isActive, isCancelable, type Transfer } from "#lib/transfers/states.js";
  import { uploads } from "#lib/transfers/transfers.svelte.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { blocking } from "#lib/users/block.svelte.js";

  // the transfers with one user, both directions or one, newest first, live
  let { username }: { username: string } = $props();

  const PAGE = 100;
  type Which = "both" | "Download" | "Upload";

  const rows = new Keyed<Transfer>((t) => t.id);
  let wanted = $state(PAGE);
  let which = $state<Which>("both");
  let removing = $state<Transfer | null>(null);
  let confirmOpen = $state(false);

  /** the newest `wanted` transfers in the directions asked for, with how many there are in all */
  const listed = new Loader(
    () => {
      const direction = which === "both" ? undefined : which;
      return call(() =>
        api.GET("/api/v0/users/{username}/transfers", {
          params: {
            path: { username },
            query: { ...byUser(username).query, direction, offset: 0, limit: wanted }
          }
        })
      );
    },
    (data) => rows.reconcile(data.transfers)
  );

  keepCurrent(
    () => void listed.load(),
    () => "transfers"
  );
  $effect(() => {
    void which;
    void wanted;
    untrack(() => void listed.load());
  });
  // this user's transfers change in place; a new one shows with the next load
  $effect(() =>
    live.on("transfers.updated", (t) => {
      if (rows.get(t.id)) rows.upsert(t);
    })
  );
  $effect(() =>
    live.on("transfers.added", (t) => {
      if (t.username === username) void listed.load();
    })
  );

  const shown = $derived((listed.data?.transfers ?? []).flatMap((t) => rows.get(t.id) ?? []));
  const total = $derived(listed.data?.total ?? 0);
  const list = new ListControl<Transfer>({
    lines: () => shown,
    key: (t) => t.id,
    id: (t) => t.id
  });

  const direction = (t: Transfer) => (t.direction === "Upload" ? "upload" : "download");
  const remove = (t: Transfer) => {
    if (!isActive(t.state)) return void act(storeOf(t), "remove", [t.id]);
    removing = t;
    confirmOpen = true;
  };

  /** Cancels the user's unfinished uploads listed here, then asks to block them */
  const denyAndBlock = async () => {
    const ids = shown
      .filter((t) => t.direction === "Upload" && isCancelable(t.state))
      .map((t) => t.id);
    await act(uploads, "cancel", ids);
    blocking.ask(username, ["uploads"]);
  };

  /** the transfers screen's cells for the file, its size, its state and when it was asked for */
  const columns: ColumnId[] = $derived(
    viewport.phone ? ["file", "actions"] : ["file", "size", "progress", "requested", "actions"]
  );
  const template = $derived(`1.5rem ${templateOf(columns)}`);
</script>

<div class="flex flex-wrap items-center gap-inline">
  <Segmented
    options={[
      { id: "both" as Which, label: "Both" },
      { id: "Download" as Which, label: "Downloads" },
      { id: "Upload" as Which, label: "Uploads" }
    ]}
    bind:value={
      () => which,
      (next) => {
        which = next;
        wanted = PAGE;
      }
    }
    label="Direction"
    size="sm" />
  <span class="ml-auto text-sm text-fg-muted">{plural(total, "transfer", "transfers")}</span>
</div>

<LoadError loader={listed} />

{#if listed.loading}
  <Skeleton class="h-40" />
{:else if !shown.length}
  <EmptyState icon={RepeatIcon} title="No transfers with this user" />
{:else}
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label={`Transfers with ${username}`}
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}>
    <div role="rowgroup" {@attach freezesWhileUsed}>
      {#each shown as t, i (t.id)}
        <div
          role="row"
          {...list.line(t, i)}
          class={[
            "group relative isolate grid min-w-max items-center list-row py-1 pl-2",
            rows.removed.has(t.id) && "opacity-40"
          ]}
          style:grid-template-columns={template}>
          <span role="gridcell" title={t.direction}>
            {#if t.direction === "Upload"}<ArrowUpIcon class="size-4 text-success" />{:else}
              <ArrowDownIcon class="size-4 text-info" />{/if}
          </span>
          <TransferRow
            transfer={t}
            direction={direction(t)}
            {columns}
            depth={0}
            showFolder
            selected={list.selected.has(t.id)}
            speeds={[]}
            onselect={(range) => list.tick(t.id, range)}
            onremove={() => remove(t)}
            onblock={(deny) => (deny ? void denyAndBlock() : blocking.ask(username))} />
        </div>
      {/each}
    </div>
  </div>
  {#if shown.length < total}
    <Button variant="outline" class="self-center" onclick={() => (wanted += PAGE)}
      >Load more</Button>
  {/if}
{/if}

<ConfirmDialog
  bind:open={confirmOpen}
  title="Remove a running transfer?"
  body="The transfer is canceled first. The file on disk is kept."
  confirm="Remove"
  danger
  onconfirm={() => removing && void act(storeOf(removing), "remove", [removing.id])} />
