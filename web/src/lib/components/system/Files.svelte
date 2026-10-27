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

  import { api, type Schemas } from "#lib/api/client.js";
  import { app } from "#lib/app.svelte.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import { isAudio } from "#lib/components/files/links.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { confirmation } from "#lib/confirm.svelte.js";
  import { dateTime, formatBytes } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import { archiveUrl, fileTrack, fileUrl, joinPath, type Root } from "#lib/localfiles.js";
  import { href } from "#lib/nav.js";
  import { player } from "#lib/player.svelte.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { fields } from "#lib/stored.svelte.js";
  import { toast } from "#lib/toast.svelte.js";
  import { encodePath } from "#lib/transfers/content.js";

  import { type Entry, entries, type SortBy } from "./explorer";

  // the downloads or incomplete directory as a file explorer: browse, open, play, delete
  let { root, path }: { root: Root; path: string } = $props();

  const parts = $derived(path.split("/").filter(Boolean));
  let listing = $state.raw<Schemas["FilesystemDirectory"] | null>(null);
  let error = $state("");
  let loading = $state(false);
  let sort = $state<{ by: SortBy; dir: "asc" | "desc" }>({ by: "name", dir: "asc" });

  const load = async () => {
    loading = true;
    const at = path;
    const outcome = await call(() =>
      at
        ? api.GET("/api/v0/files/{root}/directories/{path}", {
            params: { path: { root, path: encodePath(at) }, query: {} }
          })
        : api.GET("/api/v0/files/{root}/directories", { params: { path: { root }, query: {} } })
    );
    if (at !== path) return;
    loading = false;
    if (outcome.ok) {
      listing = outcome.data;
      error = "";
    } else {
      error = outcome.status === 404 ? `/${root}/${path} doesn't exist` : outcome.error;
      listing = null;
    }
  };
  $effect(() => {
    void root;
    void path;
    untrack(() => {
      list.clear();
      void load();
    });
  });

  const rows = $derived(listing ? entries(listing, parts, sort.by, sort.dir) : []);
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
  const preview = (e: Entry) =>
    void player.play(
      fileTrack(root, e.path),
      rows.filter((r) => r.kind === "file").map((r) => fileTrack(root, r.path))
    );

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
    list.clear();
    void load();
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

  const list = new ListControl<Entry>({
    lines: () => rows,
    key: (e) => e.key,
    id: (e) => e.key,
    onopen: (e) => void open(e),
    onkey: (e, entry) => {
      if (e.key !== "Delete" || !canDelete) return false;
      if (entry.kind === "up" && !chosen.length) return false;
      askRemove(chosen.length ? chosen : [entry]);
      return true;
    }
  });
  /** the selected entries, never the way up */
  const chosen = $derived(rows.filter((r) => r.kind !== "up" && list.selected.has(r.key)));

  const sortBy = (by: SortBy) => {
    sort = { by, dir: sort.by === by && sort.dir === "asc" ? "desc" : "asc" };
  };
  const COLUMNS = "2rem minmax(14rem,1fr) 10rem 7rem 5rem";
</script>

{#snippet header(label: string, by: SortBy)}
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
    <Button variant="outline" size="sm" disabled={loading} onclick={() => void load()}
      ><RefreshCwIcon />Refresh</Button>
  </div>
</div>

{#if error}
  <Notice kind="error">{error}</Notice>
{:else if !listing}
  <Skeleton class="h-64" />
{:else}
  <div
    class={[
      "overflow-x-auto rounded-card border bg-surface text-sm shadow-card outline-none focus-visible:ring-2 focus-visible:ring-accent",
      loading && "opacity-60"
    ]}
    role="grid"
    aria-label={`Files in /${root}/${path}`}
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    aria-busy={loading}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    <div
      role="row"
      class="grid min-w-max items-center gap-2 border-b bg-surface-muted px-2 py-1.5 text-xs font-medium text-fg-muted"
      style:grid-template-columns={COLUMNS}>
      <span role="columnheader"></span>
      <span role="columnheader">{@render header("Name", "name")}</span>
      <span role="columnheader">{@render header("Date Modified", "date")}</span>
      <span role="columnheader" class="text-right">Size</span>
      <span role="columnheader" class="sr-only">Actions</span>
    </div>
    {#each rows as e, i (e.key)}
      <div
        role="row"
        {...list.line(e, i)}
        class={[
          "grid min-w-max items-center gap-2 border-b px-2 py-1 last:border-b-0 hover:bg-hover",
          list.selected.has(e.key) && "bg-accent-soft/60",
          list.isFocused(e) && "ring-1 ring-accent ring-inset"
        ]}
        style:grid-template-columns={COLUMNS}>
        <span role="gridcell">
          {#if e.kind !== "up"}
            <Checkbox
              aria-label={`Select ${e.name}`}
              checked={list.selected.has(e.key)}
              onclick={(ev) => {
                ev.preventDefault();
                list.tick(e.key, ev.shiftKey);
              }}
              onCheckedChange={(on) => on !== list.selected.has(e.key) && list.toggle(e.key)} />
          {/if}
        </span>
        <span role="gridcell" class="flex min-w-0 items-center gap-1.5">
          {#if e.kind === "file"}<FileIcon
              class="size-4 shrink-0 text-fg-muted" />{:else}<FolderIcon
              class="size-4 shrink-0 text-fg-muted" />{/if}
          {#if e.kind === "file"}
            <span class="truncate" title={e.path}>{e.name}</span>
          {:else}
            <button
              type="button"
              class="truncate text-left hover:underline"
              onclick={() => void open(e)}>{e.name}</button>
          {/if}
        </span>
        <span role="gridcell" class="text-xs text-fg-muted"
          >{e.modifiedAt ? dateTime(e.modifiedAt) : ""}</span>
        <span role="gridcell" class="text-right tabular-nums"
          >{e.size ? formatBytes(e.size) : ""}</span>
        <span role="gridcell" class="flex justify-end">
          {#if e.kind !== "up"}
            <Menu.Root>
              <Menu.Trigger>
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`More for ${e.name}`}><EllipsisIcon /></Button>
                {/snippet}
              </Menu.Trigger>
              <Menu.Content align="end">
                {#if e.kind === "file"}
                  <Menu.Item onSelect={() => void open(e)}><ExternalLinkIcon />Open</Menu.Item>
                  {#if isAudio(e.name)}
                    <Menu.Item onSelect={() => preview(e)}><PlayIcon />Preview</Menu.Item>
                  {/if}
                  <Menu.Item onSelect={() => void save(e)}
                    ><SaveIcon />Download to device</Menu.Item>
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
          {/if}
        </span>
      </div>
    {:else}
      <p class="px-4 py-6 text-center text-sm text-fg-subtle">No files or directories</p>
    {/each}
  </div>
{/if}
