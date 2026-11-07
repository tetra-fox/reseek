<script lang="ts">
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import XIcon from "@lucide/svelte/icons/x";

  import { rowAnalysis } from "#lib/analysis/batch.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import AnalysisMark from "#lib/components/analysis/AnalysisMark.svelte";
  import TransferItems from "#lib/components/transfers/TransferItems.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import {
    ago,
    formatBytes,
    formatBytesTransferred,
    formatDuration,
    formatSpeed,
    getDirectoryName,
    getFileName
  } from "#lib/format.js";
  import {
    type Field,
    inspector,
    inspectorPrefs,
    SETTLE_MS
  } from "#lib/inspector/inspector.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { act, denyAndBlock, open, storeOf } from "#lib/transfers/actions.js";
  import {
    type Direction,
    isQueued,
    rowActions,
    stateChip,
    type Transfer
  } from "#lib/transfers/states.js";
  import { endpoint } from "#lib/transfers/transfers.svelte.js";
  import { placeText, waits } from "#lib/transfers/why.svelte.js";
  import { blocking } from "#lib/users/block.svelte.js";

  import Inspector from "./Inspector.svelte";
  import TransferHistory from "./TransferHistory.svelte";

  type Props = {
    transfer: Transfer;
    /** what the list the file was picked from knows of it, first in its details */
    facts?: Field[];
    /** removes it; by default it leaves its store, with the store's undo */
    onremove?: () => void;
  };

  let { transfer: t, facts = [], onremove }: Props = $props();

  const direction = $derived<Direction>(t.direction === "Upload" ? "upload" : "download");
  const store = $derived(storeOf(t));
  const can = $derived(rowActions(t, direction));
  const write = $derived(session.can("ReadWrite"));
  const chip = $derived(stateChip(t));
  const moving = $derived(t.state === "InProgress");
  const offered = $derived(rowAnalysis(t, direction));
  const analysis = $derived(offered?.target ?? null);

  /** where the peer can be reached, asked for only while the section that shows it is open */
  let address = $state<string | null>(null);
  $effect(() => {
    const name = t.username;
    address = null;
    if (!inspectorPrefs.sections.technical) return;
    const timer = setTimeout(() => {
      void endpoint(name).then((o) => {
        if (o.ok && name === t.username) address = `${o.data.address}:${o.data.port}`;
      });
    }, SETTLE_MS);
    return () => clearTimeout(timer);
  });
  // an upload's place is asked of the daemon, a download's comes with the transfer
  const waiting = $derived(direction === "upload" && isQueued(t.state));
  $effect(() => {
    if (!waiting) return;
    const shown = t;
    const timer = setTimeout(() => void waits.load(shown), SETTLE_MS);
    return () => clearTimeout(timer);
  });
  const wait = $derived(waiting ? waits.get(t.id) : undefined);

  const milliseconds = (ms: number | null | undefined) => (ms ? formatDuration(ms / 1000) : null);
  const details = $derived<Field[]>([
    { label: "Folder", value: getDirectoryName(t.filename) },
    ...facts,
    { label: "Speed", value: t.averageSpeed > 0 ? formatSpeed(t.averageSpeed) : null },
    { label: "Time left", value: moving ? milliseconds(t.remainingTime) : null },
    { label: "Time taken", value: milliseconds(t.elapsedTime) },
    { label: "Attempts", value: t.attempts > 1 ? String(t.attempts) : null },
    {
      label: "Queue place",
      value: wait ? placeText(wait) : t.placeInQueue ? String(t.placeInQueue) : null
    },
    { label: "Next attempt", value: t.nextAttemptAt ? ago(t.nextAttemptAt) : null }
  ]);
  const technical = $derived<Field[]>([
    { label: "State", value: t.state },
    { label: "ID", value: t.id },
    { label: "Download job ID", value: t.jobId },
    { label: "Remote path", value: t.filename },
    { label: "Local path", value: t.localPath },
    { label: "Peer address", value: address },
    { label: "Error", value: t.exception }
  ]);
</script>

{#snippet chips()}
  <Tooltip text={chip.title ?? undefined} spoken>
    {#snippet child({ props })}
      <Badge {...props} tone={chip.tone} class="max-w-full truncate">{chip.text}</Badge>
    {/snippet}
  </Tooltip>
  {#if direction === "download"}<AnalysisMark mark={marks.download(t.id)} />{/if}
{/snippet}

{#snippet actions()}
  {#if can.open}
    <Button variant="outline" size="sm" onclick={() => void open(t)}
      ><ExternalLinkIcon />Open</Button>
  {/if}
  {#if write && can.retry}
    <Button variant="outline" size="sm" onclick={() => void act(store, "retry", [t.id])}
      ><RotateCwIcon />Retry</Button>
  {/if}
  {#if write && can.cancel}
    <Button variant="outline" size="sm" onclick={() => void act(store, "cancel", [t.id])}
      ><XIcon />Cancel</Button>
  {/if}
{/snippet}

{#snippet menu()}
  <Menu.Content align="end" class="min-w-56">
    <TransferItems
      transfer={t}
      {direction}
      onremove={onremove ?? (() => store.removeLater([t.id]))}
      onblock={(deny) => (deny ? void denyAndBlock(t.username) : blocking.ask(t.username))}
      analysis={offered} />
  </Menu.Content>
{/snippet}

{#snippet history()}
  <TransferHistory transfer={t} />
{/snippet}

<Inspector
  name={getFileName(t.filename)}
  path={t.filename}
  {chips}
  size={moving ? formatBytesTransferred(t.size, t.bytesTransferred) : formatBytes(t.size)}
  percent={moving ? t.percentComplete : null}
  username={t.username}
  {actions}
  {menu}
  {details}
  {analysis}
  {history}
  {technical} />
