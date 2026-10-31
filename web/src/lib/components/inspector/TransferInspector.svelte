<script lang="ts">
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import XIcon from "@lucide/svelte/icons/x";

  import { type Target, targetKey } from "#lib/analysis/analysis.js";
  import { analyzableDownload } from "#lib/analysis/batch.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import AnalysisMark from "#lib/components/analysis/AnalysisMark.svelte";
  import RowMenu from "#lib/components/transfers/RowMenu.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
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
  import { act, denyAndBlock, downloadTrack, open, storeOf } from "#lib/transfers/actions.js";
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
  /** the transfers of its folder, which the player plays around it */
  const around = $derived(
    store.list.rows.filter(
      (o) =>
        !store.hidden.has(o.id) &&
        o.username === t.username &&
        getDirectoryName(o.filename) === getDirectoryName(t.filename)
    )
  );
  const target = $derived<Target>({ kind: "download", username: t.username, id: t.id });
  const analysis = $derived(
    can.open && analyzableDownload(t) ? { target, track: downloadTrack(t) } : null
  );

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
    { label: "Batch ID", value: t.batchId },
    { label: "Remote path", value: t.filename },
    { label: "Local path", value: t.localPath },
    { label: "Peer address", value: address },
    { label: "Error", value: t.exception }
  ]);
</script>

{#snippet chips()}
  <Badge tone={chip.tone} class="max-w-full truncate" title={chip.title ?? undefined}
    >{chip.text}</Badge>
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
  <RowMenu
    transfer={t}
    {direction}
    {around}
    onremove={onremove ?? (() => store.removeLater([t.id]))}
    onblock={(deny) => (deny ? void denyAndBlock(t.username) : blocking.ask(t.username))}
    onanalyze={analysis ? (large) => inspector.analyze(targetKey(target), large) : undefined} />
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
