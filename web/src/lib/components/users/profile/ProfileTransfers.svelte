<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import RepeatIcon from "@lucide/svelte/icons/repeat";
  import { untrack } from "svelte";

  import { api, byUser } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import RowMenu from "#lib/components/transfers/RowMenu.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import ConfirmDialog from "#lib/components/ui/ConfirmDialog.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { dateTime, formatBytes, getDirectoryName, getFileName, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { Keyed } from "#lib/lists/keyed.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { call } from "#lib/request.js";
  import { act, storeOf } from "#lib/transfers/actions.js";
  import { isActive, isCancelable, stateChip, type Transfer } from "#lib/transfers/states.js";
  import { uploads } from "#lib/transfers/transfers.svelte.js";
  import { blocking } from "#lib/users/block.svelte.js";

  // the transfers with one user, both directions or one, newest first, live
  let { username }: { username: string } = $props();

  const PAGE = 100;
  type Which = "both" | "Download" | "Upload";

  const rows = new Keyed<Transfer>((t) => t.id);
  let order = $state<string[]>([]);
  let total = $state(0);
  let wanted = $state(PAGE);
  let which = $state<Which>("both");
  let loaded = $state(false);
  let error = $state("");
  let removing = $state<Transfer | null>(null);
  let confirmOpen = $state(false);

  let generation = 0;
  const load = async () => {
    const at = ++generation;
    const direction = which === "both" ? undefined : which;
    const outcome = await call(() =>
      api.GET("/api/v0/users/{username}/transfers", {
        params: {
          path: { username },
          query: { ...byUser(username).query, direction, offset: 0, limit: wanted }
        }
      })
    );
    if (at !== generation) return;
    loaded = true;
    if (!outcome.ok) return void (error = outcome.error);
    error = "";
    rows.reconcile(outcome.data.transfers);
    order = outcome.data.transfers.map((t) => t.id);
    total = outcome.data.total;
  };

  keepCurrent(
    () => void load(),
    () => "transfers"
  );
  $effect(() => {
    void which;
    void wanted;
    untrack(() => void load());
  });
  // this user's transfers change in place; a new one shows with the next load
  $effect(() =>
    live.on("transfers.updated", (t) => {
      if (rows.get(t.id)) rows.upsert(t);
    })
  );
  $effect(() =>
    live.on("transfers.added", (t) => {
      if (t.username === username) void load();
    })
  );

  const shown = $derived(order.flatMap((id) => rows.get(id) ?? []));
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

  const COLUMNS = "1.5rem minmax(12rem,1fr) 6rem 10rem 8rem 2.5rem";
</script>

<div class="flex flex-wrap items-center gap-inline">
  <div class="flex gap-1" role="group" aria-label="Direction">
    {#each [{ id: "both", label: "Both" }, { id: "Download", label: "Downloads" }, { id: "Upload", label: "Uploads" }] as d (d.id)}
      <Button
        size="sm"
        variant={which === d.id ? "secondary" : "outline"}
        aria-pressed={which === d.id}
        onclick={() => {
          which = d.id as Which;
          wanted = PAGE;
        }}>{d.label}</Button>
    {/each}
  </div>
  <span class="ml-auto text-sm text-fg-muted">{plural(total, "transfer", "transfers")}</span>
</div>

<LoadError loader={{ error, load }} />

{#if !loaded}
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
        {@const chip = stateChip(t)}
        <div
          role="row"
          {...list.line(t, i)}
          class={[
            "grid min-w-max items-center gap-2 list-row px-2 py-1.5 hover:bg-hover",
            list.selected.has(t.id) && "bg-accent-soft/60",
            rows.removed.has(t.id) && "opacity-40"
          ]}
          style:grid-template-columns={COLUMNS}>
          <span role="gridcell" title={t.direction}>
            {#if t.direction === "Upload"}<ArrowUpIcon class="size-4 text-success" />{:else}
              <ArrowDownIcon class="size-4 text-info" />{/if}
          </span>
          <span role="gridcell" class="min-w-0 truncate" title={t.filename}>
            {getFileName(t.filename)}
            <span class="text-fg-muted"> {getDirectoryName(t.filename)}</span>
          </span>
          <span role="gridcell" class="text-right tabular-nums">{formatBytes(t.size)}</span>
          <span role="gridcell" class="min-w-0">
            <Badge tone={chip.tone} class="max-w-full truncate" title={chip.title ?? undefined}
              >{chip.text}</Badge>
          </span>
          <span role="gridcell" class="text-xs text-fg-muted">{dateTime(t.requestedAt)}</span>
          <span role="gridcell">
            <Menu.Root>
              <Menu.Trigger>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`More for ${getFileName(t.filename)}`}>
                    <EllipsisIcon />
                  </Button>
                {/snippet}
              </Menu.Trigger>
              <RowMenu
                transfer={t}
                direction={direction(t)}
                around={shown.filter(
                  (o) => getDirectoryName(o.filename) === getDirectoryName(t.filename)
                )}
                onremove={() => remove(t)}
                onblock={(deny) => (deny ? void denyAndBlock() : blocking.ask(username))} />
            </Menu.Root>
          </span>
        </div>
      {/each}
    </div>
  </div>
  {#if order.length < total}
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
