<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileAudioIcon from "@lucide/svelte/icons/file-audio";
  import FileVideoIcon from "@lucide/svelte/icons/file-video";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import XIcon from "@lucide/svelte/icons/x";

  import { analyzableDownload } from "#lib/analysis/batch.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import AnalysisMark from "#lib/components/analysis/AnalysisMark.svelte";
  import DownloadFill from "#lib/components/files/DownloadFill.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { kindOf } from "#lib/files/kinds.js";
  import {
    ago,
    dateTime,
    formatBytes,
    formatBytesTransferred,
    formatDuration,
    formatSpeed,
    getDirectoryName,
    getFileName,
    NONE,
    plural
  } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";
  import {
    act,
    open,
    openFolder,
    queueFailure,
    queuePosition,
    storeOf
  } from "#lib/transfers/actions.js";
  import { type ColumnId, SELECT_REM } from "#lib/transfers/grid/columns.js";
  import { type Direction, rowActions, stateChip, type Transfer } from "#lib/transfers/states.js";
  import { placeText, waits, whyText } from "#lib/transfers/why.svelte.js";
  import { GAP_REM, lineStart, TICK_REM, TOGGLE_REM } from "#lib/tree/indent.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import RowMenu from "./RowMenu.svelte";
  import Sparkline from "./Sparkline.svelte";

  type Props = {
    transfer: Transfer;
    direction: Direction;
    columns: ColumnId[];
    /** how deep it is under group headers, 0 when nothing groups it */
    depth: number;
    /** the folder is shown under the name, where no group header says it */
    showFolder: boolean;
    selected: boolean;
    speeds: number[];
    /** ticks the row; `range` when shift was held */
    onselect: (range: boolean) => void;
    /**
     * Opens the inspector on the file, at its analysis with `analyze`, in the large view with
     * `large`; a screen without the inspector leaves it out
     */
    ondetails?: (analyze?: boolean, large?: boolean) => void;
    onremove: () => void;
    onblock: (deny: boolean) => void;
  };

  let {
    transfer: t,
    direction,
    columns,
    depth,
    showFolder,
    selected,
    speeds,
    onselect,
    ondetails,
    onremove,
    onblock
  }: Props = $props();

  const icons = { audio: FileAudioIcon, video: FileVideoIcon, other: FileIcon };
  const Icon = $derived(icons[kindOf(t.filename)]);
  const chip = $derived(stateChip(t));
  const can = $derived(rowActions(t, direction));
  const write = $derived(session.can("ReadWrite"));
  const user = $derived(users.get(t.username));
  const wait = $derived(direction === "upload" ? waits.get(t.id) : undefined);
  const retryIn = $derived(t.nextAttemptAt ? ago(t.nextAttemptAt) : null);
  // under a group header the file's icon and name start where a header at the row's depth has its
  // own, past its tick and toggle, the select column holding the row's tick. A phone's row has no
  // tick or toggle to pass, so it starts under the header's tick and keeps its width for the name
  const fileInset = $derived(
    depth
      ? `${viewport.phone ? lineStart(depth) : lineStart(depth) + TICK_REM + TOGGLE_REM + GAP_REM - SELECT_REM}rem`
      : undefined
  );
</script>

{#snippet label()}
  {getFileName(t.filename)}
  {#if showFolder}
    <span class="text-fg-muted"> {getDirectoryName(t.filename)}</span>
  {/if}
{/snippet}

{#snippet name()}
  <Icon class="size-4 shrink-0 text-fg-muted" />
  {#if ondetails}
    <button
      type="button"
      class="min-w-0 truncate text-left hover:underline"
      title={t.filename}
      onclick={() => ondetails()}>
      {@render label()}
    </button>
  {:else}
    <span class="min-w-0 truncate" title={t.filename}>{@render label()}</span>
  {/if}
  {#if direction === "download"}<AnalysisMark mark={marks.download(t.id)} />{/if}
{/snippet}

{#snippet progress()}
  {#if t.state === "InProgress"}
    <div
      class="h-2 min-w-0 flex-1 overflow-hidden rounded-full bg-surface-muted"
      role="progressbar"
      aria-valuenow={Math.round(t.percentComplete)}
      aria-valuemin={0}
      aria-valuemax={100}>
      <div class="h-full bg-accent transition-[width]" style:width={`${t.percentComplete}%`}></div>
    </div>
    <span class="text-xs tabular-nums">{chip.text}</span>
  {:else}
    <Tooltip text={chip.title ?? (retryIn ? `next attempt ${retryIn}` : undefined)}>
      {#snippet child({ props })}
        <span {...props} class="min-w-0">
          <Badge tone={chip.tone} class="max-w-full truncate">{chip.text}</Badge>
        </span>
      {/snippet}
    </Tooltip>
    {#if retryIn}<span class="truncate text-xs text-fg-muted">retry {retryIn}</span>{/if}
  {/if}
{/snippet}

<!-- behind the cells, the transfer's bytes as a fill, its finish as a pulse, a failure as a tint -->
<DownloadFill transfer={t} track={direction === "download"} />
{#each columns as column (column)}
  <div
    role="gridcell"
    class={[
      "flex min-w-0 items-center gap-1.5 px-2",
      ["size", "speed", "eta", "elapsed", "attempts", "transferred"].includes(column) &&
        "justify-end whitespace-nowrap tabular-nums",
      column === "actions" && "pl-0"
    ]}
    style:padding-left={column === "file" ? fileInset : undefined}>
    {#if column === "select"}
      <Checkbox
        aria-label={`Select ${getFileName(t.filename)}`}
        checked={selected}
        onclick={(e) => {
          // a click is handled here, with Shift for a range, and the checkbox's own toggle is
          // skipped; Space on it toggles through onCheckedChange
          e.preventDefault();
          onselect(e.shiftKey);
        }}
        onCheckedChange={(on) => on !== selected && onselect(false)} />
    {:else if column === "file"}
      {#if viewport.phone}
        <!-- a phone gives the name the row's width, with the size and progress under it -->
        <div class="flex min-w-0 flex-1 flex-col gap-0.5">
          <div class="flex min-w-0 items-center gap-1.5">{@render name()}</div>
          <div class="flex min-w-0 items-center gap-1.5 text-xs text-fg-muted">
            <span
              class="shrink-0 whitespace-nowrap tabular-nums"
              title={plural(t.size, "byte", "bytes")}>{formatBytes(t.size)}</span>
            {@render progress()}
          </div>
        </div>
      {:else}
        {@render name()}
      {/if}
    {:else if column === "user"}
      {#if direction === "upload" && user?.group.color}
        <span
          class="size-2 shrink-0 rounded-full"
          style:background-color={user.group.color}
          title={`Group ${user.group.name}`}></span>
      {/if}
      <Username name={t.username} folder={getFileName(getDirectoryName(t.filename))} />
      {#if user?.block}<Badge tone="danger">blocked</Badge>{/if}
    {:else if column === "size"}
      <span title={plural(t.size, "byte", "bytes")}>{formatBytes(t.size)}</span>
    {:else if column === "progress"}
      {@render progress()}
    {:else if column === "speed"}
      {#if t.state === "InProgress"}
        <Sparkline points={speeds} />
        <span>{formatSpeed(t.averageSpeed)}</span>
      {/if}
    {:else if column === "eta"}
      {t.state === "InProgress" && t.remainingTime !== null && t.remainingTime !== undefined
        ? formatDuration(t.remainingTime / 1000)
        : NONE}
    {:else if column === "transferred"}
      {formatBytesTransferred(t.size, t.bytesTransferred)}
    {:else if column === "queue"}
      {#if direction === "download"}
        {#if t.placeInQueue}<span class="tabular-nums">#{t.placeInQueue}</span>{/if}
        {#if can.queuePosition}
          {@const failed = storeOf(t).queueErrors.get(t.id)}
          <Tooltip text={failed === undefined ? undefined : queueFailure(failed)}>
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                aria-label="Check queue position"
                onclick={() => void queuePosition(t)}>
                <RefreshCwIcon />
              </Button>
            {/snippet}
          </Tooltip>
        {/if}
      {:else if t.state.includes("Queued")}
        <Tooltip text={wait ? whyText(wait) : undefined}>
          {#snippet child({ props })}
            <span {...props} class="truncate" role="note" onpointerenter={() => void waits.load(t)}>
              {wait ? placeText(wait) : "Queued"}
            </span>
          {/snippet}
        </Tooltip>
      {/if}
    {:else if column === "requested" || column === "started" || column === "ended"}
      {@const at =
        column === "requested" ? t.requestedAt : column === "started" ? t.startedAt : t.endedAt}
      {#if at}<span title={ago(at)}>{dateTime(at)}</span>{/if}
    {:else if column === "elapsed"}
      {t.elapsedTime !== null && t.elapsedTime !== undefined
        ? formatDuration(t.elapsedTime / 1000)
        : ""}
    {:else if column === "attempts"}
      {t.attempts}
    {:else if column === "batch"}
      {#if t.batchId}<span class="font-mono text-xs" title={t.batchId}>{t.batchId.slice(0, 8)}</span
        >{/if}
    {:else if column === "group"}
      {user?.group.name ?? ""}
    {:else if column === "actions"}
      <div class="ml-auto flex items-center row-action">
        <!-- a phone shows only the menu, which holds every one of these too -->
        <span class="contents max-md:hidden">
          {#if write && can.retry}
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Retry"
              onclick={() => void act(storeOf(t), "retry", [t.id])}>
              <RotateCwIcon />
            </Button>
          {/if}
          {#if write && can.cancel}
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Cancel"
              onclick={() => void act(storeOf(t), "cancel", [t.id])}>
              <XIcon />
            </Button>
          {/if}
          {#if write}
            <Button variant="ghost" size="icon-sm" aria-label="Remove" onclick={onremove}>
              <Trash2Icon />
            </Button>
          {/if}
          {#if can.open}
            <Button variant="ghost" size="icon-sm" aria-label="Open" onclick={() => void open(t)}>
              <ExternalLinkIcon />
            </Button>
          {/if}
          {#if can.openFolder}
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Open folder"
              title="Open folder"
              onclick={() => openFolder([t])}>
              <FolderOpenIcon />
            </Button>
          {/if}
        </span>
        <Menu.LazyRoot>
          {#snippet trigger({ props })}
            <Button
              {...props}
              variant="ghost"
              size="icon-sm"
              aria-label={`More for ${getFileName(t.filename)}`}>
              <EllipsisIcon />
            </Button>
          {/snippet}
          <RowMenu
            transfer={t}
            {direction}
            {onremove}
            {onblock}
            ondetails={ondetails ? () => ondetails() : undefined}
            onanalyze={ondetails && can.open && analyzableDownload(t)
              ? (large) => ondetails(true, large)
              : undefined} />
        </Menu.LazyRoot>
      </div>
    {/if}
  </div>
{/each}
