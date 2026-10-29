<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileArchiveIcon from "@lucide/svelte/icons/file-archive";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import PlayIcon from "@lucide/svelte/icons/play";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import SaveIcon from "@lucide/svelte/icons/save";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { type Component, untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  import { queue } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import { FILE_FILTERS, type MarkFilter, passes } from "#lib/analysis/marks.js";
  import { analyzable, analyzerOffered, marks } from "#lib/analysis/marks.svelte.js";
  import { api, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import AnalysisDrawer from "#lib/components/analysis/AnalysisDrawer.svelte";
  import AnalysisJobs from "#lib/components/analysis/AnalysisJobs.svelte";
  import AnalysisMark from "#lib/components/analysis/AnalysisMark.svelte";
  import { isAudio } from "#lib/components/files/links.js";
  import FolderTree from "#lib/components/tree/FolderTree.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Select from "#lib/components/ui/Select.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { confirmation } from "#lib/confirm.svelte.js";
  import { dateTime, formatBytes, plural } from "#lib/format.js";
  import { shiftHeld } from "#lib/keys.js";
  import { archiveUrl, fileTrack, fileUrl, joinPath, type Root } from "#lib/localfiles.js";
  import { href } from "#lib/nav.js";
  import { player } from "#lib/player.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { encodePath } from "#lib/transfers/content.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { type FolderNode, type TreeLine, treeLines } from "#lib/tree/folders.js";
  import { TICK_REM } from "#lib/tree/indent.js";
  import { viewport } from "#lib/viewport.svelte.js";

  import { type Entry, entries, type SortBy } from "./explorer";

  // the downloads or incomplete directory as a file explorer: browse, open, play, delete
  let { root, path }: { root: Root; path: string } = $props();

  const parts = $derived(path.split("/").filter(Boolean));
  /** what each folder read so far holds, by its path under the root; the shown one is `path` */
  const listings = new SvelteMap<string, Schemas["FilesystemDirectory"]>();
  const expansion = new Expansion(false);
  const selected = new SvelteSet<string>();
  let error = $state("");
  let loading = $state(false);
  let sort = $state<{ by: SortBy; dir: "asc" | "desc" }>({ by: "name", dir: "asc" });
  const listing = $derived(listings.get(path) ?? null);

  /** Reads one folder; the one shown says when it can't be read, one below it toasts */
  const read = async (at: string) => {
    if (at === path) loading = true;
    const outcome = await call(() =>
      at
        ? api.GET("/api/v0/files/{root}/directories/{path}", {
            params: { path: { root, path: encodePath(at) }, query: {} }
          })
        : api.GET("/api/v0/files/{root}/directories", { params: { path: { root }, query: {} } })
    );
    if (at === path) loading = false;
    if (outcome.ok) {
      listings.set(at, outcome.data);
      if (at === path) error = "";
    } else if (at === path) {
      error = outcome.status === 404 ? `/${root}/${path} does not exist` : outcome.error;
      listings.delete(at);
    } else {
      toast(outcome.error, "error");
      expansion.set(at, false);
    }
  };
  /** Reads the shown folder again with every open folder under it */
  const load = () => {
    for (const at of listings.keys()) {
      if (at !== path && !expansion.isOpen(at)) listings.delete(at);
    }
    void read(path);
    for (const at of listings.keys()) if (at !== path) void read(at);
  };
  $effect(() => {
    void root;
    void path;
    untrack(() => {
      selected.clear();
      listings.clear();
      expansion.reset(false);
      void read(path);
    });
  });

  /** a folder as a node of the tree, its contents once it has been read */
  const nodeOf = (folder: Entry): FolderNode<Entry> => {
    const read = listings.get(folder.path);
    const inside = read ? entries(read, folder.path.split("/"), sort.by, sort.dir) : [];
    const children = inside.filter((e) => e.kind === "directory").map(nodeOf);
    const items = inside.filter((e) => e.kind === "file" && shows(e));
    return {
      path: folder.path,
      label: folder.name,
      items,
      children,
      count: items.length + children.reduce((n, c) => n + c.count, 0),
      bytes:
        items.reduce((n, e) => n + (e.size ?? 0), 0) + children.reduce((n, c) => n + c.bytes, 0)
    };
  };
  const top = $derived(listing ? entries(listing, parts, sort.by, sort.dir) : []);
  const roots = $derived(top.filter((e) => e.kind === "directory").map(nodeOf));
  /** the shown folder itself, which its own files and the way up belong to */
  const here = $derived<FolderNode<Entry>>({
    path,
    label: parts.at(-1) ?? root,
    items: top.filter((e) => e.kind !== "directory" && shows(e)),
    children: roots,
    count: 0,
    bytes: 0
  });
  const lines = $derived<TreeLine<Entry>[]>([
    ...top
      .filter((e) => e.kind === "up")
      .map((e) => ({ kind: "item" as const, key: e.key, item: e, depth: 0, folder: here })),
    ...treeLines(roots, expansion.isOpen, (e) => e.key),
    ...top
      .filter((e) => e.kind === "file" && shows(e))
      .map((e) => ({ kind: "item" as const, key: e.key, item: e, depth: 0, folder: here }))
  ]);
  /** every entry read so far, by key, for the selection */
  const byKey = $derived.by(() => {
    const map = new Map<string, Entry>();
    for (const [at, read] of listings) {
      for (const e of entries(read, at.split("/").filter(Boolean), sort.by, sort.dir)) {
        map.set(e.key, e);
      }
    }
    return map;
  });
  const managed = $derived(fields(app.options).remote_file_management === true);
  /** which files the list shows by their analysis, every file when null */
  let only = $state<MarkFilter | null>(null);
  const shows = (e: Entry) =>
    e.kind !== "file" || !only || passes(only, marks.file(root, e.path), analyzable(e.name));
  $effect(() => marks.retain());
  /** the file whose analysis the drawer shows, queued when it has none with `start` */
  let analyzing = $state<{ entry: Entry; start: boolean; large: boolean } | null>(null);
  $effect(() => {
    void root;
    analyzing = null;
  });

  /** Queues every file under a folder whose extension the analyzer is offered for */
  const analyzeFolder = async (e: Entry) => {
    const listing = await call(() =>
      api.GET("/api/v0/files/{root}/directories/{path}", {
        params: { path: { root, path: encodePath(e.path) }, query: { recursive: true } }
      })
    );
    if (!listing.ok) return void toast(`Could not read ${e.name}: ${listing.error}`, "error");
    const files = (listing.data.files ?? [])
      .map((f) => f.fullName.replaceAll("\\", "/"))
      .filter(analyzable);
    if (!files.length) return void toast(`${e.name} has no files to analyze`);
    let queued = 0;
    const failed: string[] = [];
    for (const file of files) {
      const outcome = await queue({ kind: "file", root, path: joinPath([e.path, file]) });
      // a file analyzed already or in the queue is left as it is
      if (outcome.ok) queued++;
      else if (outcome.status !== 409) failed.push(`${file}: ${outcome.error}`);
    }
    void jobs.refresh();
    if (failed.length) toast(`Could not queue ${failed.join("; ")}`, "error");
    else if (queued) toast(`${plural(queued, "file", "files")} queued for analysis`);
    else toast(`All files in ${e.name} are already analyzed`);
  };
  const write = $derived(session.can("ReadWrite"));
  const canDelete = $derived(managed && write);

  const go = (to: string[]) =>
    void goto(href(`/system/files?root=${root}&path=${encodeURIComponent(joinPath(to))}`));
  const open = async (e: Entry) => {
    if (e.kind === "up") return go(parts.slice(0, -1));
    if (e.kind === "directory") return go([...parts, e.name]);
    const tab = window.open("about:blank", "_blank");
    const url = await fileUrl(root, e.path, "inline");
    if (!url.ok) {
      tab?.close();
      return void toast(url.error, "error");
    }
    if (tab) tab.location.href = url.data;
  };
  const save = async (e: Entry) => {
    const url = await fileUrl(root, e.path, "attachment");
    if (url.ok) window.location.assign(url.data);
    else toast(url.error, "error");
  };
  /** Plays an audio file with the other files of its folder around it */
  const preview = (e: Entry) => {
    const folder = e.path.split("/").slice(0, -1).join("/");
    const read = listings.get(folder);
    const around = read ? entries(read, folder.split("/").filter(Boolean), sort.by, sort.dir) : [e];
    void player.play(
      fileTrack(root, e.path),
      around.filter((r) => r.kind === "file").map((r) => fileTrack(root, r.path))
    );
  };

  const remove = async (targets: Entry[]) => {
    const failed: string[] = [];
    for (const e of targets) {
      const encoded = encodePath(e.path);
      const outcome = await call(() =>
        e.kind === "directory"
          ? api.DELETE("/api/v0/files/{root}/directories/{path}", {
              params: { path: { root, path: encoded } }
            })
          : api.DELETE("/api/v0/files/{root}/files/{path}", {
              params: { path: { root, path: encoded } }
            })
      );
      if (!outcome.ok) {
        failed.push(
          `${e.name}: ${outcome.status === 403 ? "deleting files from the UI is turned off" : outcome.error}`
        );
      }
    }
    selected.clear();
    const shown = analyzing?.entry.path;
    if (targets.some((e) => e.path === shown)) analyzing = null;
    load();
    if (failed.length) toast(`Could not delete ${failed.join("; ")}`, "error");
    else
      toast(
        targets.length === 1
          ? `${targets[0].name} deleted`
          : `${plural(targets.length, "item", "items")} deleted`
      );
  };
  const askRemove = (targets: Entry[]) => {
    if (!targets.length || !canDelete) return;
    const one = targets[0];
    confirmation.ask(
      targets.length === 1
        ? {
            title: one.kind === "directory" ? "Delete folder?" : "Delete file?",
            body: `${one.path} will be deleted. This cannot be undone.`,
            confirm: "Delete",
            run: () => void remove(targets)
          }
        : {
            title: `Delete ${targets.length} items?`,
            body: "This cannot be undone.",
            confirm: "Delete",
            run: () => void remove(targets)
          }
    );
  };

  /** the selected entries, never the way up */
  const chosen = $derived(
    [...selected].flatMap((key) => {
      const e = byKey.get(key);
      return e && e.kind !== "up" ? [e] : [];
    })
  );
  const dirEntry = (node: FolderNode<Entry>) => byKey.get(`d:${node.path}`);

  /** Delete removes the selection, or the row it is pressed on when nothing is selected */
  const onkey = (e: KeyboardEvent, line: TreeLine<Entry>) => {
    if (e.key !== "Delete" || !canDelete) return false;
    const entry = line.kind === "item" ? line.item : dirEntry(line.node);
    if (!entry || (entry.kind === "up" && !chosen.length)) return false;
    askRemove(chosen.length ? chosen : [entry]);
    return true;
  };

  const sortBy = (by: SortBy) => {
    sort = { by, dir: sort.by === by && sort.dir === "asc" ? "desc" : "asc" };
  };
  // a phone leaves out the modified date and puts a file's size under its name, so a name keeps
  // the room it needs
  const COLUMNS = $derived(
    viewport.phone
      ? `${TICK_REM}rem minmax(0,1fr) 2.5rem`
      : `${TICK_REM}rem minmax(14rem,1fr) 10rem 7rem 3rem`
  );
</script>

{#snippet sortable(label: string, by: SortBy)}
  <button type="button" class="flex items-center gap-1" onclick={() => sortBy(by)}>
    {label}
    {#if sort.by === by}
      {#if sort.dir === "asc"}<ArrowUpIcon class="size-3" />{:else}<ArrowDownIcon
          class="size-3" />{/if}
    {/if}
  </button>
{/snippet}

<nav aria-label="Location" class="flex gap-1">
  {#each [{ id: "downloads", title: "Downloads" }, { id: "incomplete", title: "Incomplete" }] as t (t.id)}
    <a
      href={href(`/system/files?root=${t.id}`)}
      aria-current={root === t.id ? "page" : undefined}
      class="flex h-control items-center rounded-control px-3 text-sm text-fg-muted hover:bg-hover hover:text-fg aria-[current]:nav-current"
      >{t.title}</a>
  {/each}
</nav>

{#if !managed}
  <Notice kind="info">
    Remote file management is disabled. Enable <code>remote_file_management</code> to delete files here.
  </Notice>
{/if}

<div class="flex flex-wrap items-center gap-inline">
  <FolderOpenIcon class="size-4 text-fg-muted" />
  <nav aria-label="Path" class="flex flex-wrap items-center gap-0.5 font-mono text-sm">
    <a href={href(`/system/files?root=${root}`)} class="hover:underline">/{root}</a>
    {#each parts as part, i (`${i}/${part}`)}
      <span class="text-fg-subtle">/</span>
      <a
        href={href(
          `/system/files?root=${root}&path=${encodeURIComponent(joinPath(parts.slice(0, i + 1)))}`
        )}
        class="hover:underline">{part}</a>
    {/each}
  </nav>
  <div class="ml-auto flex max-w-full flex-wrap justify-end gap-inline">
    {#if analyzerOffered()}
      <Select
        bind:value={only}
        aria-label="Show"
        class="h-control-sm w-52 max-w-full"
        options={[
          { value: null, label: "All files" },
          ...FILE_FILTERS.map((f) => ({ value: f.id, label: f.label }))
        ]} />
    {/if}
    {#if canDelete && chosen.length}
      <Button variant="danger" size="sm" onclick={() => askRemove(chosen)}
        ><Trash2Icon />Delete selected ({chosen.length})</Button>
    {/if}
    <Button variant="outline" size="sm" disabled={loading} onclick={load}
      ><RefreshCwIcon />Refresh</Button>
  </div>
</div>

{#snippet menu(e: Entry)}
  <Menu.Root>
    <Menu.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="ghost" size="icon-sm" aria-label={`More for ${e.name}`}
          ><EllipsisIcon /></Button>
      {/snippet}
    </Menu.Trigger>
    <Menu.Content align="end">
      {#if e.kind === "file"}
        <Menu.Item onSelect={() => void open(e)}><ExternalLinkIcon />Open</Menu.Item>
        {#if isAudio(e.name)}
          <Menu.Item onSelect={() => preview(e)}><PlayIcon />Preview</Menu.Item>
        {/if}
        {#if analyzable(e.name)}
          <Menu.Item onSelect={() => (analyzing = { entry: e, start: write, large: shiftHeld() })}
            ><AudioLinesIcon />Analyze</Menu.Item>
        {/if}
        <Menu.Item onSelect={() => void save(e)}><SaveIcon />Download to device</Menu.Item>
      {:else}
        <Menu.Item onSelect={() => void open(e)}><FolderOpenIcon />Open</Menu.Item>
        <Menu.Item onSelect={() => window.location.assign(archiveUrl(root, e.path))}
          ><FileArchiveIcon />Download as zip</Menu.Item>
        {#if write && analyzerOffered()}
          <Menu.Item onSelect={() => void analyzeFolder(e)}
            ><AudioLinesIcon />Analyze folder</Menu.Item>
        {/if}
      {/if}
      <Menu.Item onSelect={() => void copyText(e.path)}><CopyIcon />Copy path</Menu.Item>
      {#if canDelete}
        <Menu.Separator />
        <Menu.Item onSelect={() => askRemove([e])}><Trash2Icon />Delete</Menu.Item>
      {/if}
    </Menu.Content>
  </Menu.Root>
{/snippet}

{#snippet tick(e: Entry)}
  <Checkbox
    aria-label={`Select ${e.name}`}
    checked={selected.has(e.key)}
    onCheckedChange={(on) => (on ? selected.add(e.key) : selected.delete(e.key))} />
{/snippet}

<AnalysisJobs />

{#if error}
  <Notice kind="error">
    {error}
    {#snippet action()}
      <Button variant="ghost" size="sm" onclick={load}>Retry</Button>
    {/snippet}
  </Notice>
{:else if !listing}
  <div class="flex flex-col gap-px overflow-hidden rounded-card border bg-surface shadow-card">
    {#each [0, 1, 2, 3, 4] as n (n)}
      <div class="flex h-9 items-center gap-3 px-3" aria-hidden="true">
        <Skeleton class="size-4" /><Skeleton class="h-3 flex-1" /><Skeleton class="h-3 w-28" />
      </div>
    {/each}
  </div>
{:else}
  <FolderTree
    class={[
      "overflow-x-auto rounded-card border bg-surface shadow-card",
      loading && "opacity-60"
    ].join(" ")}
    {lines}
    {expansion}
    label={`Files in /${root}/${path}`}
    columns={COLUMNS}
    {selected}
    idOf={(e) => e.key}
    folderId={(node) => `d:${node.path}`}
    onopen={(e) => void open(e)}
    onopenfolder={(node) => go(node.path.split("/"))}
    {onkey}
    ontoggle={(opened, paths) => {
      if (!opened) return;
      for (const at of paths) if (!listings.has(at)) void read(at);
    }}>
    {#snippet header()}
      <span role="columnheader"></span>
      <span role="columnheader">{@render sortable("Name", "name")}</span>
      <span role="columnheader" class="max-md:hidden">{@render sortable("Modified", "date")}</span>
      {#if !viewport.phone}<span role="columnheader" class="pr-3 text-right">Size</span>{/if}
      <span role="columnheader" class="sr-only">Actions</span>
    {/snippet}
    {#snippet folder(node)}
      {@const e = dirEntry(node)}
      {#if e}{@render tick(e)}{/if}
      <FolderIcon class="size-4 shrink-0 text-fg-muted" />
      <button
        type="button"
        class="min-w-0 truncate text-left hover:underline"
        title={node.path}
        onclick={() => go(node.path.split("/"))}>{node.label}</button>
      {#if listings.has(node.path)}
        <span class="shrink-0 text-xs text-fg-subtle tabular-nums">
          {node.count} · {formatBytes(node.bytes)}
        </span>
      {/if}
      <span class="ml-auto w-40 shrink-0 text-xs text-fg-muted max-md:hidden">
        {e?.modifiedAt ? dateTime(e.modifiedAt) : ""}
      </span>
      <span class="w-28 shrink-0 max-md:hidden"></span>
      {#if e}<span class="row-action max-md:ml-auto">{@render menu(e)}</span>{/if}
    {/snippet}
    {#snippet item(e)}
      <span role="gridcell" class="flex">
        {#if e.kind !== "up"}{@render tick(e)}{/if}
      </span>
      <span role="gridcell" class="flex min-w-0 items-center gap-1.5">
        {#if e.kind === "file"}
          <FileIcon class="size-4 shrink-0 text-fg-muted" />
          {#if viewport.phone}
            <span class="flex min-w-0 flex-col py-1">
              <span class="truncate" title={e.path}>{e.name}</span>
              <span class="text-xs whitespace-nowrap text-fg-muted tabular-nums"
                >{e.size ? formatBytes(e.size) : ""}</span>
            </span>
          {:else}
            <span class="truncate" title={e.path}>{e.name}</span>
          {/if}
          <AnalysisMark mark={marks.file(root, e.path)} />
        {:else}
          <FolderIcon class="size-4 shrink-0 text-fg-muted" />
          <button
            type="button"
            class="truncate text-left hover:underline"
            onclick={() => void open(e)}>{e.name}</button>
        {/if}
      </span>
      <span role="gridcell" class="text-xs text-fg-muted max-md:hidden"
        >{e.modifiedAt ? dateTime(e.modifiedAt) : ""}</span>
      {#if !viewport.phone}
        <span role="gridcell" class="pr-3 text-right tabular-nums"
          >{e.size ? formatBytes(e.size) : ""}</span>
      {/if}
      <span role="gridcell" class="flex justify-end"
        >{#if e.kind !== "up"}{@render menu(e)}{/if}</span>
    {/snippet}
  </FolderTree>
  {#if lines.every((l) => l.kind === "item" && l.item.kind === "up")}
    <p class="px-4 py-6 text-center text-sm text-fg-subtle">
      {only
        ? "No files here match the filter."
        : root === "downloads" && !path
          ? "No finished downloads yet."
          : "This folder is empty."}
    </p>
  {/if}
{/if}

{#snippet tool(label: string, Icon: Component, run: () => void)}
  <Tooltip text={label}>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" aria-label={label} onclick={run}
        ><Icon /></Button>
    {/snippet}
  </Tooltip>
{/snippet}

{#if analyzing}
  {@const e = analyzing.entry}
  <AnalysisDrawer
    target={{ kind: "file", root, path: e.path }}
    track={fileTrack(root, e.path)}
    name={e.name}
    start={analyzing.start}
    large={analyzing.large}
    onclose={() => (analyzing = null)}>
    {#snippet actions()}
      {#if isAudio(e.name)}
        {@render tool("Preview", PlayIcon, () => preview(e))}
      {/if}
      {@render tool("Download to device", SaveIcon, () => void save(e))}
      {#if canDelete}
        {@render tool("Delete", Trash2Icon, () => askRemove([e]))}
      {/if}
    {/snippet}
  </AnalysisDrawer>
{/if}
