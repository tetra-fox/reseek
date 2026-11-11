<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileAudioIcon from "@lucide/svelte/icons/file-audio";
  import FileVideoIcon from "@lucide/svelte/icons/file-video";
  import LockIcon from "@lucide/svelte/icons/lock";
  import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import type { FileColumn } from "#lib/files/columns.js";
  import { downloadOf, starting } from "#lib/files/downloads.js";
  import { kindOf } from "#lib/files/kinds.js";
  import { formatAttributes, formatBytes, formatSeconds, getFileName } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";
  import { act } from "#lib/transfers/actions.js";
  import { fillOf } from "#lib/transfers/states.js";
  import { downloads, fileKey } from "#lib/transfers/transfers.svelte.js";

  import DownloadFill from "./DownloadFill.svelte";
  import FileMenu from "./FileMenu.svelte";
  import Suspect from "./Suspect.svelte";

  type Props = {
    /** the file, its `filename` the full remote path */
    file: Schemas["File"] & { locked?: boolean };
    /** whose share it is in */
    username: string;
    /** the cells drawn, from `fileColumns` */
    columns: FileColumn[];
    selected: boolean;
    /** ticks the row; `range` when Shift was held */
    ontick: (range: boolean) => void;
    /** enqueues the file, where it can be downloaded now */
    ondownload?: () => void;
    /** opens the inspector on the file, on a screen that has one */
    ondetails?: () => void;
  };

  let { file, username, columns, selected, ontick, ondownload, ondetails }: Props = $props();

  const icons = { audio: FileAudioIcon, video: FileVideoIcon, other: FileIcon };
  const Icon = $derived(file.locked ? LockIcon : icons[kindOf(file.filename)]);
  const name = $derived(getFileName(file.filename));
  const download = $derived(downloadOf(username, file.filename));
  const asked = $derived(starting.has(fileKey(username, file.filename)));
  const fill = $derived(fillOf(download, asked));
  const shows = (column: FileColumn) => columns.includes(column);
</script>

<!-- the cells of a remote file's row, each a gridcell, in the order `columns` gives; behind them
     the file's download, when it has one -->
<DownloadFill transfer={download} starting={asked} />
{#if shows("tick")}
  <span role="gridcell" class="flex">
    {#if !file.locked}
      <Checkbox aria-label={`Select ${name}`} checked={selected} {ontick} />
    {/if}
  </span>
{/if}
<!-- the name is cut in the middle, where names that share a start keep their ends in view -->
{#snippet nameText()}
  <Tooltip text={file.filename} dense>
    {#snippet child({ props })}<span {...props} class="flex min-w-0"
        ><MiddleTruncate text={name} /></span
      >{/snippet}
  </Tooltip>
{/snippet}

<span role="gridcell" class="flex min-w-0 items-center gap-1.5 pr-2">
  <Icon class="size-4 shrink-0 text-fg-muted" />
  {#if !shows("size")}
    <!-- a phone gives the name the row's width, with the size under it -->
    <span class="flex min-w-0 flex-col">
      {@render nameText()}
      <span class="text-xs whitespace-nowrap text-fg-muted tabular-nums"
        >{formatBytes(file.size)}</span>
    </span>
  {:else}
    {@render nameText()}
  {/if}
  {#if fill === "waiting"}
    <span class="shrink-0 text-xs text-fg-muted">Queued</span>
  {:else if fill === "done"}
    <CheckIcon class="size-4 shrink-0 text-success" aria-label="Downloaded" />
  {:else if fill === "failed" && download && session.can("ReadWrite")}
    <Tooltip text={download.exception ?? undefined}>
      {#snippet child({ props })}
        <Button
          {...props}
          variant="ghost"
          size="sm"
          class="h-6 shrink-0 text-danger"
          onclick={() => void act(downloads, "retry", [download.id])}>
          <RotateCwIcon />Retry
        </Button>
      {/snippet}
    </Tooltip>
  {/if}
</span>
{#if shows("size")}
  <span role="gridcell" class="pr-3 text-right font-mono text-xs whitespace-nowrap tabular-nums"
    >{formatBytes(file.size)}</span>
{/if}
{#if shows("quality")}
  <span role="gridcell" class="flex min-w-0 items-center gap-1 text-fg-muted">
    {#if "suspect" in file && file.suspect}<Suspect suspect={file.suspect} />{/if}
    <span class="truncate font-mono text-xs">{formatAttributes(file)}</span>
  </span>
{/if}
{#if shows("length")}
  <span
    role="gridcell"
    class="pr-3 text-right font-mono text-xs whitespace-nowrap text-fg-muted tabular-nums"
    >{formatSeconds(file.length)}</span>
{/if}
{#if shows("menu")}
  <span role="gridcell" class="flex justify-end pr-1">
    <FileMenu path={file.filename} {username} {ondownload} {ondetails}>
      {#snippet trigger({ props })}
        <Button
          {...props}
          variant="ghost"
          size="icon-sm"
          class="row-action"
          aria-label={`Actions for ${name}`}>
          <EllipsisIcon />
        </Button>
      {/snippet}
    </FileMenu>
  </span>
{/if}
