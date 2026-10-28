<script lang="ts">
  import { goto } from "$app/navigation";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
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
  import { untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  import { api, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import { isAudio } from "#lib/components/files/links.js";
  import FolderTree from "#lib/components/tree/FolderTree.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { confirmation } from "#lib/confirm.svelte.js";
  import { dateTime, formatBytes } from "#lib/format.js";
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
      error = outcome.status === 404 ? `/${root}/${path} doesn't exist` : outcome.error;
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
    const items = inside.filter((e) => e.kind === "file");
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
    items: top.filter((e) => e.kind !== "directory"),
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
      .filter((e) => e.kind === "file")
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
          `${e.name}: ${outcome.status === 403 ? "Remote file management is disabled" : outcome.error}`
        );
      }
    }
    selected.clear();
    load();
    if (failed.length) toast(`${failed.length} not deleted: ${failed.join("; ")}`, "error");
    else toast(targets.length === 1 ? `${targets[0].name} deleted` : `${targets.length} deleted`);
  };
  const askRemove = (targets: Entry[]) => {
    if (!targets.length || !canDelete) return;
    const one = targets[0];
    confirmation.ask(
      targets.length === 1
        ? {
            title: one.kind === "directory" ? "Confirm Directory Delete" : "Confirm File Delete",
            body: `Are you sure you want to delete ${one.kind === "directory" ? "directory" : "file"} '${one.path}'?`,
            confirm: "Delete",
            run: () => void remove(targets)
          }
        : {
            title: "Delete items",
            body: `Delete ${targets.length} items? This cannot be undone.`,
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
  const COLUMNS = "2.5rem minmax(14rem,1fr) 10rem 7rem 3rem";
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

<nav aria-label="Directory" class="flex gap-1">
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
    Remote file management is disabled; enable <code>remote_file_management</code> to delete files from
    here.
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
  <div class="ml-auto flex gap-inline">
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
        <Menu.Item onSelect={() => void save(e)}><SaveIcon />Download to device</Menu.Item>
      {:else}
        <Menu.Item onSelect={() => void open(e)}><FolderOpenIcon />Open</Menu.Item>
        <Menu.Item onSelect={() => window.location.assign(archiveUrl(root, e.path))}
          ><FileArchiveIcon />Download as zip</Menu.Item>
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
      <div
        role="row"
        class="grid min-w-max items-center border-b bg-surface-muted py-1.5 text-xs font-medium text-fg-muted"
        style:grid-template-columns={COLUMNS}>
        <span role="columnheader"></span>
        <span role="columnheader">{@render sortable("Name", "name")}</span>
        <span role="columnheader">{@render sortable("Date Modified", "date")}</span>
        <span role="columnheader" class="pr-3 text-right">Size</span>
        <span role="columnheader" class="sr-only">Actions</span>
      </div>
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
      <span class="ml-auto w-40 shrink-0 text-xs text-fg-muted">
        {e?.modifiedAt ? dateTime(e.modifiedAt) : ""}
      </span>
      <span class="w-28 shrink-0"></span>
      {#if e}<span class="row-action">{@render menu(e)}</span>{/if}
    {/snippet}
    {#snippet item(e)}
      <span role="gridcell" class="flex justify-center">
        {#if e.kind !== "up"}{@render tick(e)}{/if}
      </span>
      <span
        role="gridcell"
        class="flex min-w-0 items-center gap-1.5"
        style:padding-left="calc(var(--depth) * 1.25rem)">
        {#if e.kind === "file"}
          <FileIcon class="size-4 shrink-0 text-fg-muted" />
          <span class="truncate" title={e.path}>{e.name}</span>
        {:else}
          <FolderIcon class="size-4 shrink-0 text-fg-muted" />
          <button
            type="button"
            class="truncate text-left hover:underline"
            onclick={() => void open(e)}>{e.name}</button>
        {/if}
      </span>
      <span role="gridcell" class="text-xs text-fg-muted"
        >{e.modifiedAt ? dateTime(e.modifiedAt) : ""}</span>
      <span role="gridcell" class="pr-3 text-right tabular-nums"
        >{e.size ? formatBytes(e.size) : ""}</span>
      <span role="gridcell" class="flex justify-end"
        >{#if e.kind !== "up"}{@render menu(e)}{/if}</span>
    {/snippet}
  </FolderTree>
  {#if !lines.length}
    <p class="px-4 py-6 text-center text-sm text-fg-subtle">
      {root === "downloads" && !path
        ? "Nothing here yet; finished downloads land in this folder."
        : "This folder is empty."}
    </p>
  {/if}
{/if}
