<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileAudioIcon from "@lucide/svelte/icons/file-audio";
  import FileVideoIcon from "@lucide/svelte/icons/file-video";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import GripVerticalIcon from "@lucide/svelte/icons/grip-vertical";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import XIcon from "@lucide/svelte/icons/x";

  import { rowAnalysis } from "#lib/analysis/batch.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import AnalysisMark from "#lib/components/analysis/AnalysisMark.svelte";
  import DownloadFill from "#lib/components/files/DownloadFill.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Progress from "#lib/components/ui/Progress.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { kindOf } from "#lib/files/kinds.js";
  import {
    ago,
    dateTime,
    formatBytes,
    formatBytesTransferred,
    formatCount,
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
  import { type ColumnId, fileInset } from "#lib/transfers/grid/columns.js";
  import { type Direction, rowActions, stateChip, type Transfer } from "#lib/transfers/states.js";
  import { downloads } from "#lib/transfers/transfers.svelte.js";
  import { placeText, waits, whyText } from "#lib/transfers/why.svelte.js";
  import { lineStart } from "#lib/tree/indent.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
  import { users } from "#lib/users/users.svelte.js";

  import Sparkline from "./Sparkline.svelte";
  import TransferItems from "./TransferItems.svelte";

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
    /** opens the inspector on the file; a screen without the inspector leaves it out */
    ondetails?: () => void;
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
  // a download waiting here has a place in the local queue, which dragging its handle onto
  // another waiting download's moves it before that one
  const localPlace = $derived(direction === "download" ? downloads.places.get(t.id) : undefined);
  const DRAGGED = "application/x-reseek-downloads";
  let dropping = $state(false);
  const dragStart = (e: DragEvent) => {
    e.dataTransfer?.setData(DRAGGED, JSON.stringify([t.id]));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  };
  const dragOver = (e: DragEvent) => {
    if (!can.reorder || !e.dataTransfer?.types.includes(DRAGGED)) return;
    e.preventDefault();
    dropping = true;
  };
  const drop = (e: DragEvent) => {
    dropping = false;
    const ids = JSON.parse(e.dataTransfer?.getData(DRAGGED) || "[]") as string[];
    if (!ids.length || ids.includes(t.id)) return;
    e.preventDefault();
    void downloads.move(ids, t.id);
  };
  // under a group header the file's icon and name start where a header at the row's depth has its
  // own, past its tick and toggle, the select column holding the row's tick. A phone's row has no
  // tick or toggle to pass, so it starts under the header's tick and keeps its width for the name
  /** the folder shown after the name, where no folder line above says it */
  const folder = $derived(showFolder ? getDirectoryName(t.filename) : "");
  const inset = $derived(
    depth ? `${viewport.phone ? lineStart(depth) : fileInset(depth)}rem` : undefined
  );
</script>

{#snippet label()}
  <!-- the folder after the name gives way first; a name alone is the control's own text -->
  {#if folder}
    <span class="max-w-full shrink-0 truncate" dir="auto">{getFileName(t.filename)}</span>
    <MiddleTruncate text={folder} path class="text-fg-muted" />
  {:else}
    {getFileName(t.filename)}
  {/if}
{/snippet}

{#snippet name()}
  <!-- the analysis mark keeps its word and the name gives way at its end; in a cell too narrow
       for an icon, a few letters and the mark, the mark goes so the name keeps them -->
  <span class="@container flex min-w-0 flex-1">
    <span class="grid min-w-0 flex-1 grid-cols-[minmax(0,max-content)_auto] items-center gap-1.5">
      <span class="flex min-w-0 items-center gap-1.5">
        <Icon class="size-4 shrink-0 text-fg-muted" />
        <Tooltip text={t.filename} dense>
          {#snippet child({ props })}
            {#if ondetails}
              <button
                {...props}
                type="button"
                class={["min-w-0 text-left hover:underline", folder ? "flex gap-1.5" : "truncate"]}
                dir={folder ? undefined : "auto"}
                onclick={() => ondetails()}>
                {@render label()}
              </button>
            {:else}
              <span
                {...props}
                class={["min-w-0", folder ? "flex gap-1.5" : "truncate"]}
                dir={folder ? undefined : "auto"}>{@render label()}</span>
            {/if}
          {/snippet}
        </Tooltip>
      </span>
      {#if direction === "download"}
        <span class="flex @max-[10em]:hidden"><AnalysisMark mark={marks.download(t.id)} /></span>
      {/if}
    </span>
  </span>
{/snippet}

{#snippet progress()}
  {#if t.state === "InProgress"}
    <Progress
      value={t.percentComplete}
      label={`Progress of ${getFileName(t.filename)}`}
      class="h-2 min-w-0 flex-1" />
    <span class="font-mono text-xs tabular-nums">{chip.text}</span>
  {:else}
    {#if write && can.reorder}
      <!-- a download waiting here is dragged onto another to start before it; the keyboard has
           the row menu's Start next and Start last -->
      <Tooltip text="Drag onto another waiting download to start before it" dense>
        {#snippet child({ props })}
          <span
            {...props}
            role="presentation"
            draggable="true"
            class={[
              "flex shrink-0 cursor-grab items-center gap-1 rounded-badge text-fg-muted",
              dropping && "ring-2 ring-accent"
            ]}
            ondragstart={dragStart}
            ondragover={dragOver}
            ondragleave={() => (dropping = false)}
            ondrop={drop}>
            <GripVerticalIcon class="size-3.5" />
            {#if localPlace}<span class="text-xs tabular-nums">#{formatCount(localPlace)} here</span
              >{/if}
          </span>
        {/snippet}
      </Tooltip>
    {/if}
    <Tooltip text={chip.title ?? (retryIn ? `next attempt ${retryIn}` : undefined)} dense spoken>
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
        "justify-end font-mono text-xs whitespace-nowrap tabular-nums",
      column === "actions" && "pl-0"
    ]}
    style:padding-left={column === "file" ? inset : undefined}>
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
            <Tooltip text={plural(t.size, "byte", "bytes")} dense>
              {#snippet child({ props })}
                <span {...props} class="shrink-0 whitespace-nowrap tabular-nums"
                  >{formatBytes(t.size)}</span>
              {/snippet}
            </Tooltip>
            {@render progress()}
          </div>
        </div>
      {:else}
        {@render name()}
      {/if}
    {:else if column === "user"}
      {#if direction === "upload" && user?.group.color}
        <Tooltip text={`Group ${user.group.name}`} dense spoken>
          {#snippet child({ props })}
            <span
              {...props}
              class="size-2 shrink-0 rounded-full"
              style:background-color={user.group.color}></span>
          {/snippet}
        </Tooltip>
      {/if}
      <Username name={t.username} folder={getFileName(getDirectoryName(t.filename))} />
      {#if user?.block}<Badge tone="danger">blocked</Badge>{/if}
    {:else if column === "size"}
      <Tooltip text={plural(t.size, "byte", "bytes")} dense>
        {#snippet child({ props })}<span {...props}>{formatBytes(t.size)}</span>{/snippet}
      </Tooltip>
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
        {#if t.placeInQueue}<span class="tabular-nums">#{formatCount(t.placeInQueue)}</span>{/if}
        {#if can.queuePosition}
          {@const failed = storeOf(t).queueErrors.get(t.id)}
          <Tooltip text={failed === undefined ? "Check queue position" : queueFailure(failed)}>
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
      {#if at}
        <Tooltip text={ago(at)} dense>
          {#snippet child({ props })}<span {...props} class="truncate">{dateTime(at)}</span
            >{/snippet}
        </Tooltip>
      {/if}
    {:else if column === "elapsed"}
      {t.elapsedTime !== null && t.elapsedTime !== undefined
        ? formatDuration(t.elapsedTime / 1000)
        : ""}
    {:else if column === "attempts"}
      {formatCount(t.attempts)}
    {:else if column === "job"}
      {#if t.jobId}
        {@const job = t.jobId}
        <Tooltip text={job} dense>
          {#snippet child({ props })}
            <span {...props} class="font-mono text-xs">{job.slice(0, 8)}</span>
          {/snippet}
        </Tooltip>
      {/if}
    {:else if column === "group"}
      {user?.group.name ?? ""}
    {:else if column === "actions"}
      <div class="ml-auto flex items-center row-action">
        <!-- a phone shows only the menu, which holds every one of these too -->
        <span class="contents max-md:hidden">
          {#if write && can.retry}
            <Tooltip text="Retry">
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Retry"
                  onclick={() => void act(storeOf(t), "retry", [t.id])}>
                  <RotateCwIcon />
                </Button>
              {/snippet}
            </Tooltip>
          {/if}
          {#if write && can.cancel}
            <Tooltip text="Cancel">
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Cancel"
                  onclick={() => void act(storeOf(t), "cancel", [t.id])}>
                  <XIcon />
                </Button>
              {/snippet}
            </Tooltip>
          {/if}
          {#if write}
            <Tooltip text="Remove">
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Remove"
                  onclick={onremove}>
                  <Trash2Icon />
                </Button>
              {/snippet}
            </Tooltip>
          {/if}
          {#if can.open}
            <Tooltip text="Open">
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Open"
                  onclick={() => void open(t)}>
                  <ExternalLinkIcon />
                </Button>
              {/snippet}
            </Tooltip>
          {/if}
          {#if can.openFolder}
            <Tooltip text="Open folder">
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Open folder"
                  onclick={() => openFolder([t])}>
                  <FolderOpenIcon />
                </Button>
              {/snippet}
            </Tooltip>
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
          <Menu.Content align="end" class="min-w-56">
            <TransferItems
              transfer={t}
              {direction}
              {onremove}
              {onblock}
              ondetails={ondetails ? () => ondetails() : undefined}
              analysis={ondetails ? rowAnalysis(t, direction) : null} />
          </Menu.Content>
        </Menu.LazyRoot>
      </div>
    {/if}
  </div>
{/each}
