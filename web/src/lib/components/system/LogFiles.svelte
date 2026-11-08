<script lang="ts" module>
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { gridView } from "#lib/lists/table.svelte.js";

  /** the log files' sort and column widths, kept in this browser */
  const view = gridView("reseek.logs.files", ["name", "date"], { id: "date", dir: "desc" });
</script>

<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileTextIcon from "@lucide/svelte/icons/file-text";

  import { api, root, type Schemas } from "#lib/api/client.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import GridHeader from "#lib/components/ui/GridHeader.svelte";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { dateTime, formatBytes } from "#lib/format.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { Grid, keptIn } from "#lib/lists/table.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  // the log files on disk, newest first, to read in a tab or save
  /** files larger than this are offered for saving only */
  const MAX_VIEW = 50 * 1024 * 1024;

  const files = new Loader(async () => {
    const outcome = await call(() => api.GET("/api/v0/logs/files"));
    // no log directory yet is no files
    return !outcome.ok && outcome.status === 404 ? { ok: true as const, data: [] } : outcome;
  });
  void files.load();

  type File = Schemas["FilesystemFile"];
  // a phone keeps the name and the buttons, with the date and the size under the name
  const grid = new Grid<File>({
    columns: () => [
      { id: "name", label: "Name", width: "10rem", grow: true, value: (f) => f.name.toLowerCase() },
      { id: "date", label: "Modified", width: "10rem", value: (f) => Date.parse(f.modifiedAt) },
      { id: "size", label: "Size", width: "6rem", end: true, value: (f) => f.length },
      { id: "actions", label: "Actions", width: "5rem", quiet: true, fixed: true }
    ],
    data: () => files.data ?? [],
    ...keptIn(view),
    shown: () => (viewport.phone ? ["name", "actions"] : ["name", "date", "size", "actions"])
  });
  const rows = $derived(grid.rows);
  const url = (f: Schemas["FilesystemFile"]) =>
    `${root()}/api/v0/logs/files/${encodeURIComponent(f.name)}`;

  let box = $state<HTMLElement | null>(null);
  // Enter follows the row's own link: a view in a new tab, or the save of a file too big to show
  const open = (f: Schemas["FilesystemFile"]) =>
    box?.querySelector<HTMLAnchorElement>(`a[data-open="${CSS.escape(f.name)}"]`)?.click();

  const list = new ListControl<Schemas["FilesystemFile"]>({
    lines: () => rows,
    key: (f) => f.name,
    id: (f) => f.name,
    onopen: open
  });
</script>

{#if files.loading}
  <Skeleton class="h-40" />
{:else if files.error}
  <ErrorSegment caption={files.error} retry={() => void files.load()} retryLabel="Retry" />
{:else if !rows.length}
  <EmptyState icon={FileTextIcon} title="No log files" />
{:else}
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Log files"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    bind:this={box}
    {@attach list.attach}>
    <GridHeader {grid} class="h-8 border-b bg-surface-muted text-fg-muted" />
    {#each rows as f, i (f.name)}
      <div role="row" {...list.line(f, i)} class="grid items-center list-row py-1" style={grid.row}>
        <span role="gridcell" class="flex min-w-0 items-center gap-1.5 px-2">
          <FileIcon class="size-4 shrink-0 text-fg-muted" />
          <span class="flex min-w-0 flex-col">
            <!-- a file's date is at the end of its name, so the name is cut in the middle -->
            <Tooltip text={f.name} dense>
              {#snippet child({ props })}
                <span {...props} class="flex min-w-0"><MiddleTruncate text={f.name} /></span>
              {/snippet}
            </Tooltip>
            {#if viewport.phone}
              <span class="truncate text-xs text-fg-muted tabular-nums"
                >{dateTime(f.modifiedAt)}, {formatBytes(f.length)}</span>
            {/if}
          </span>
        </span>
        {#if !viewport.phone}
          <span role="gridcell" class="truncate px-2 text-xs text-fg-muted"
            >{dateTime(f.modifiedAt)}</span>
          <span role="gridcell" class="truncate px-2 text-right tabular-nums"
            >{formatBytes(f.length)}</span>
        {/if}
        <span role="gridcell" class="flex justify-end px-1">
          {#if f.length <= MAX_VIEW}
            <Tooltip text={`View ${f.name} in a new tab`}>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="ghost"
                  size="icon-sm"
                  href={url(f)}
                  target="_blank"
                  rel="noopener"
                  data-open={f.name}
                  aria-label={`View ${f.name} in a new tab`}><ExternalLinkIcon /></Button>
              {/snippet}
            </Tooltip>
          {/if}
          <Tooltip text={`Download ${f.name}`}>
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                href={url(f)}
                download={f.name}
                data-open={f.length > MAX_VIEW ? f.name : undefined}
                aria-label={`Download ${f.name}`}><DownloadIcon /></Button>
            {/snippet}
          </Tooltip>
        </span>
      </div>
    {/each}
  </div>
{/if}
