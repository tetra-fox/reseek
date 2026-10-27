<script lang="ts">
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import XIcon from "@lucide/svelte/icons/x";

  import type { Schemas } from "#lib/api/client.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Username from "#lib/components/users/Username.svelte";
  import {
    ago,
    formatBytes,
    formatDate,
    formatDuration,
    formatSpeed,
    getFileName
  } from "#lib/format.js";
  import { keepsKey, layerOpen, withModifier } from "#lib/keys.js";
  import { stateChip, type Direction, type Transfer } from "#lib/transfers/states.js";
  import { endpoint, type Transfers } from "#lib/transfers/transfers.svelte.js";
  import { users } from "#lib/users.svelte.js";

  import RowMenu from "./RowMenu.svelte";

  type Props = {
    transfer: Transfer;
    store: Transfers;
    direction: Direction;
    around: Transfer[];
    onclose: () => void;
    /** moves to the row before or after in the table */
    onstep: (by: -1 | 1) => void;
    onremove: () => void;
    onblock: (deny: boolean) => void;
  };

  let {
    transfer: t,
    store,
    direction,
    around,
    onclose,
    onstep,
    onremove,
    onblock
  }: Props = $props();

  let history = $state.raw<Schemas["TransferStateChange"][] | null>(null);
  let address = $state<string | null>(null);

  // the history and the peer's address are asked for each transfer the drawer shows
  $effect(() => {
    const shown = t;
    history = null;
    void store.history(shown).then((o) => {
      if (o.ok && shown.id === t.id) history = o.data.stateHistory ?? [];
    });
  });
  $effect(() => {
    const name = t.username;
    address = null;
    void users.load(name);
    void endpoint(name).then((o) => {
      if (o.ok && name === t.username) address = `${o.data.address}:${o.data.port}`;
    });
  });

  const user = $derived(users.get(t.username));
  const extension = $derived(getFileName(t.filename).split(".").pop() ?? "");
  const when = (iso: string | null | undefined) =>
    iso ? `${formatDate(iso)} (${ago(iso)})` : "N/A";
  const fields = $derived<[string, string][]>([
    ["Id", t.id],
    ["Batch Id", t.batchId ?? "N/A"],
    ["Direction", t.direction],
    ["State", t.state],
    ["Requested At", when(t.requestedAt)],
    ["Enqueued At", when(t.enqueuedAt)],
    ["Started At", when(t.startedAt)],
    ["Ended At", when(t.endedAt)],
    ["Bytes Transferred", formatBytes(t.bytesTransferred)],
    ["Bytes Remaining", formatBytes(t.bytesRemaining)],
    ["Average Speed", formatSpeed(t.averageSpeed)],
    [
      "Elapsed Time",
      t.elapsedTime !== null && t.elapsedTime !== undefined
        ? formatDuration(t.elapsedTime / 1000)
        : "N/A"
    ],
    [
      "Remaining Time",
      t.remainingTime !== null && t.remainingTime !== undefined
        ? formatDuration(t.remainingTime / 1000)
        : "N/A"
    ],
    ["Percent Complete", `${t.percentComplete.toFixed(2)}%`],
    ["Place In Queue", t.placeInQueue ? String(t.placeInQueue) : "N/A"],
    ["Attempts", String(t.attempts)],
    ["Next Attempt At", when(t.nextAttemptAt)]
  ]);
</script>

<svelte:window
  onkeydown={(e) => {
    // a key another layer took (a menu, a dialog, a card) or one a field or control keeps
    // isn't the drawer's
    if (e.defaultPrevented || withModifier(e) || layerOpen() || keepsKey(e.target, e.key)) return;
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowLeft") onstep(-1);
    else if (e.key === "ArrowRight") onstep(1);
  }} />

<aside
  class="fixed top-0 right-0 bottom-10 z-30 flex w-[min(28rem,100vw)] flex-col border-l bg-surface shadow-raised"
  aria-label="Transfer details">
  <header class="flex items-center gap-2 border-b px-4 py-3">
    <h2 class="min-w-0 flex-1 truncate font-semibold" title={t.filename}>
      {getFileName(t.filename)}
    </h2>
    <Menu.Root>
      <Menu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" aria-label="Actions"
            ><EllipsisIcon /></Button>
        {/snippet}
      </Menu.Trigger>
      <RowMenu transfer={t} {direction} {around} {onremove} {onblock} ondetails={() => {}} />
    </Menu.Root>
    <Button variant="ghost" size="icon-sm" aria-label="Close the details" onclick={onclose}
      ><XIcon /></Button>
  </header>
  <div class="flex flex-1 flex-col gap-section overflow-y-auto p-4 text-sm">
    <section class="flex flex-col gap-1">
      <h3 class="text-xs font-medium text-fg-muted uppercase">File</h3>
      <p>{getFileName(t.filename)}</p>
      <p class="flex items-start gap-1 font-mono text-xs wrap-anywhere">
        {t.filename}
        <button
          type="button"
          aria-label="Copy the remote path"
          onclick={() => void copyText(t.filename)}>
          <CopyIcon class="size-3.5 text-fg-muted" />
        </button>
      </p>
      {#if t.localPath}
        <p class="flex items-start gap-1 font-mono text-xs wrap-anywhere">
          {t.localPath}
          <button
            type="button"
            aria-label="Copy the local path"
            onclick={() => void copyText(t.localPath!)}>
            <CopyIcon class="size-3.5 text-fg-muted" />
          </button>
        </p>
      {/if}
      <p class="text-fg-muted">
        {formatBytes(t.size)} ({t.size.toLocaleString()} bytes){extension ? ` · ${extension}` : ""}
      </p>
    </section>
    <section class="flex flex-col gap-1">
      <h3 class="text-xs font-medium text-fg-muted uppercase">Transfer</h3>
      <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
        {#each fields as [label, value] (label)}
          <dt class="font-medium">{label}</dt>
          <dd class="min-w-0 wrap-anywhere">{value}</dd>
        {/each}
      </dl>
      {#if t.exception}
        <div class="mt-2 flex items-start gap-1">
          <pre
            class="min-w-0 flex-1 rounded-control bg-surface-muted p-2 font-mono text-xs whitespace-pre-wrap">{t.exception}</pre>
          <button
            type="button"
            aria-label="Copy the error"
            onclick={() => void copyText(t.exception!)}>
            <CopyIcon class="size-3.5 text-fg-muted" />
          </button>
        </div>
      {/if}
    </section>
    <section class="flex flex-col gap-1">
      <h3 class="text-xs font-medium text-fg-muted uppercase">Peer</h3>
      <Username name={t.username} />
      <p class="text-fg-muted">
        {user?.presence ?? "Unknown"} · group {user?.group.name ?? "?"}{address
          ? ` · ${address}`
          : ""}
      </p>
      {#if user?.statistics}
        <p class="text-fg-muted">
          {user.statistics.fileCount?.toLocaleString() ?? "?"} files · {formatSpeed(
            user.statistics.averageSpeed
          )}
        </p>
      {/if}
    </section>
    <section class="flex flex-col gap-1">
      <h3 class="text-xs font-medium text-fg-muted uppercase">Timeline</h3>
      {#if history === null}
        <p class="text-fg-muted">Loading</p>
      {:else if !history.length}
        <p class="text-fg-muted">No states recorded</p>
      {:else}
        <ol class="flex flex-col gap-1">
          {#each history as change, i (`${change.at}:${change.state}`)}
            <li class="flex gap-2">
              <span class="text-fg-muted tabular-nums">{formatDate(change.at)}</span>
              <span class="min-w-0 flex-1">{stateChip({ ...t, state: change.state }).text}</span>
              {#if i > 0}
                <span class="text-xs text-fg-muted">
                  +{formatDuration((Date.parse(change.at) - Date.parse(history[i - 1].at)) / 1000)}
                </span>
              {/if}
            </li>
          {/each}
        </ol>
      {/if}
    </section>
    {#if t.batchId}
      {@const batch = store.list.rows.filter((o) => o.batchId === t.batchId)}
      <section class="flex flex-col gap-1">
        <h3 class="text-xs font-medium text-fg-muted uppercase">Batch</h3>
        <p class="font-mono text-xs">{t.batchId}</p>
        <p class="text-fg-muted">
          {batch.length} files · {formatBytes(batch.reduce((n, o) => n + o.size, 0))} ·
          {batch.filter((o) => o.state === "Completed, Succeeded").length} done
        </p>
      </section>
    {/if}
  </div>
</aside>
