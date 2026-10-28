<script lang="ts">
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileTextIcon from "@lucide/svelte/icons/file-text";

  import { api, root, type Schemas } from "#lib/api/client.js";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { dateTime, formatBytes } from "#lib/format.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { call } from "#lib/request.js";

  // the log files on disk, newest first, to read in a tab or save
  /** files larger than this are offered for saving only */
  const MAX_VIEW = 50 * 1024 * 1024;

  const files = new Loader(async () => {
    const outcome = await call(() => api.GET("/api/v0/logs/files"));
    // no log directory yet is no files
    return !outcome.ok && outcome.status === 404 ? { ok: true as const, data: [] } : outcome;
  });
  void files.load();

  let sort = $state<{ by: "name" | "date"; dir: "asc" | "desc" }>({ by: "date", dir: "desc" });
  const rows = $derived(
    (files.data ?? []).toSorted((a, b) => {
      const sign = sort.dir === "asc" ? 1 : -1;
      return sort.by === "name"
        ? sign * a.name.localeCompare(b.name, undefined, { numeric: true })
        : sign * (Date.parse(a.modifiedAt) - Date.parse(b.modifiedAt));
    })
  );
  const url = (f: Schemas["FilesystemFile"]) =>
    `${root()}/api/v0/logs/files/${encodeURIComponent(f.name)}`;

  let grid = $state<HTMLElement | null>(null);
  // Enter follows the row's own link: a view in a new tab, or the save of a file too big to show
  const open = (f: Schemas["FilesystemFile"]) =>
    grid?.querySelector<HTMLAnchorElement>(`a[data-open="${CSS.escape(f.name)}"]`)?.click();

  const list = new ListControl<Schemas["FilesystemFile"]>({
    lines: () => rows,
    key: (f) => f.name,
    id: (f) => f.name,
    onopen: open
  });
  const sortBy = (by: "name" | "date") =>
    (sort = { by, dir: sort.by === by ? (sort.dir === "asc" ? "desc" : "asc") : "asc" });
</script>

{#snippet header(label: string, by: "name" | "date")}
  <button type="button" class="flex items-center gap-1" onclick={() => sortBy(by)}>
    {label}
    {#if sort.by === by}
      {#if sort.dir === "asc"}<ArrowUpIcon class="size-3" />{:else}<ArrowDownIcon
          class="size-3" />{/if}
    {/if}
  </button>
{/snippet}

{#if files.loading}
  <Skeleton class="h-40" />
{:else if files.error}
  <ErrorSegment caption={files.error} retry={() => void files.load()} retryLabel="Retry" />
{:else if !rows.length}
  <EmptyState icon={FileTextIcon} title="No log files" />
{:else}
  <p class="flex items-center gap-1.5 font-mono text-sm text-fg-muted">/logs</p>
  <div
    class="overflow-x-auto rounded-card border bg-surface text-sm shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Log files"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    bind:this={grid}
    {@attach list.attach}>
    <div
      role="row"
      class="grid min-w-max grid-cols-[minmax(14rem,1fr)_10rem_6rem_5rem] gap-2 border-b bg-surface-muted px-2 py-1.5 text-xs font-medium text-fg-muted">
      <span role="columnheader">{@render header("Name", "name")}</span>
      <span role="columnheader">{@render header("Modified", "date")}</span>
      <span role="columnheader" class="text-right">Size</span>
      <span role="columnheader" class="sr-only">Actions</span>
    </div>
    {#each rows as f, i (f.name)}
      <div
        role="row"
        {...list.line(f, i)}
        class={[
          "grid min-w-max grid-cols-[minmax(14rem,1fr)_10rem_6rem_5rem] items-center gap-2 border-b px-2 py-1 last:border-b-0 hover:bg-hover",
          list.selected.has(f.name) && "bg-accent-soft/60",
          list.isFocused(f) && "ring-1 ring-accent ring-inset"
        ]}>
        <span role="gridcell" class="flex items-center gap-1.5"
          ><FileIcon class="size-4 text-fg-muted" />{f.name}</span>
        <span role="gridcell" class="text-xs text-fg-muted">{dateTime(f.modifiedAt)}</span>
        <span role="gridcell" class="text-right tabular-nums">{formatBytes(f.length)}</span>
        <span role="gridcell" class="flex justify-end">
          {#if f.length <= MAX_VIEW}
            <Button
              variant="ghost"
              size="icon-sm"
              href={url(f)}
              target="_blank"
              rel="noopener"
              data-open={f.name}
              aria-label={`View ${f.name} in a new tab`}
              title={`View ${f.name} in a new tab`}><ExternalLinkIcon /></Button>
          {/if}
          <Button
            variant="ghost"
            size="icon-sm"
            href={url(f)}
            download={f.name}
            data-open={f.length > MAX_VIEW ? f.name : undefined}
            aria-label={`Download ${f.name}`}
            title={`Download ${f.name}`}><DownloadIcon /></Button>
        </span>
      </div>
    {/each}
  </div>
{/if}
