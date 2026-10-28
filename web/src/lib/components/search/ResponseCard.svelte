<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import ListIcon from "@lucide/svelte/icons/list";
  import LockIcon from "@lucide/svelte/icons/lock";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import FileMenu from "#lib/components/files/FileMenu.svelte";
  import FolderFiles from "#lib/components/files/FolderFiles.svelte";
  import Suspect from "#lib/components/files/Suspect.svelte";
  import FolderTree from "#lib/components/tree/FolderTree.svelte";
  import TreeToggle from "#lib/components/tree/TreeToggle.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { asItem, type Enqueued, enqueue } from "#lib/downloads.js";
  import { fetchFolder } from "#lib/folders.js";
  import {
    formatAttributes,
    formatBytes,
    formatSeconds,
    getDirectoryName,
    getFileName,
    plural
  } from "#lib/format.js";
  import { href } from "#lib/nav.js";
  import { type Result, type ResultFile, resultFiles } from "#lib/search/results.js";
  import { toast } from "#lib/toast.svelte.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { type FolderNode, folderTree, treeLines } from "#lib/tree/folders.js";
  import { users } from "#lib/users.svelte.js";

  import Country from "./Country.svelte";

  type Props = {
    result: Result;
    searchId: string;
    /** which cards are open, Alt-click on one opening or closing every card shown */
    cards: Expansion;
    /** the keys of the cards shown */
    siblings: string[];
    /** downloads need the Soulseek server */
    connected: boolean;
    canWrite: boolean;
    ondismiss: () => void;
  };

  let { result, searchId, cards, siblings, connected, canWrite, ondismiss }: Props = $props();

  /** the file table's columns: tick, name, size, attributes, length, menu */
  const COLUMNS = "2.25rem minmax(12rem, 1fr) 6rem 9rem 4.5rem 2.5rem";

  const response = $derived(result.response);
  const name = $derived(response.username);
  const user = $derived(users.get(name));
  const open = $derived(cards.isOpen(result.key));

  const selected = new SvelteSet<string>();
  const folders = new Expansion(true);
  /** folders replaced by what the user sent when asked for the whole folder */
  const fetched = new SvelteMap<string, ResultFile[]>();
  let asking = $state<string | null>(null);
  let downloading = $state(false);
  let status = $state<Enqueued | null>(null);
  let flat = $state<{ folder: string; files: Schemas["File"][] } | null>(null);
  let flatOpen = $state(false);

  $effect(() => void users.load(name));

  // a folder the user was asked for again lists what they sent, where its first file was
  const files = $derived.by(() => {
    const list: ResultFile[] = [];
    const placed = new Set<string>();
    for (const f of resultFiles(response)) {
      const folder = getDirectoryName(f.filename);
      const replaced = fetched.get(folder);
      if (!replaced) list.push(f);
      else if (!placed.has(folder)) {
        placed.add(folder);
        list.push(...replaced);
      }
    }
    return list;
  });
  const roots = $derived(
    folderTree(
      files,
      (f) => f.filename,
      (f) => f.size
    )
  );
  const lines = $derived(treeLines(roots, folders.isOpen, (f) => f.filename));
  const byName = $derived(new Map(files.map((f) => [f.filename, f])));
  const chosen = $derived([...selected].flatMap((n) => byName.get(n) ?? []));
  const allBytes = $derived(files.reduce((sum, f) => sum + f.size, 0));
  const chosenBytes = $derived(chosen.reduce((sum, f) => sum + f.size, 0));

  /** every file in a folder and below it */
  const under = (node: FolderNode<ResultFile>): ResultFile[] => [
    ...node.items,
    ...node.children.flatMap(under)
  ];

  const download = async (list: Schemas["File"][]) => {
    downloading = true;
    status = null;
    status = await enqueue({ username: name, files: list.map((f) => asItem(f)), searchId });
    downloading = false;
    toast(status.text, status.kind === "done" ? "info" : "error");
  };

  /** Asks the user for the whole folder and lists what they send in place of the results */
  const askFolder = async (folder: string) => {
    asking = folder;
    const outcome = await fetchFolder(name, folder);
    asking = null;
    if (!outcome.ok) {
      toast(`Couldn't load the folder: ${outcome.error}`, "error");
      return;
    }
    fetched.set(
      folder,
      outcome.data[0].files.map((f) => ({ ...f, locked: false }))
    );
  };

  /** Lists every file under the folder, asked of the user so files the search missed are in */
  const viewAll = async (folder: string) => {
    asking = folder;
    const outcome = await fetchFolder(name, folder);
    asking = null;
    if (!outcome.ok) {
      toast(outcome.error, "error");
      return;
    }
    flat = { folder, files: outcome.data.flatMap((d) => d.files) };
    flatOpen = true;
  };

  const tick = (filenames: string[], on: boolean) => {
    for (const filename of filenames) {
      if (on) selected.add(filename);
      else selected.delete(filename);
    }
  };
  const write = $derived(canWrite && connected);
</script>

<section
  class="flex flex-col overflow-hidden rounded-card border bg-surface shadow-card [contain-intrinsic-size:auto_12rem] [content-visibility:auto]"
  data-key={result.key}>
  <header class="flex flex-wrap items-center gap-2 px-3 py-2.5">
    <TreeToggle expansion={cards} key={result.key} {siblings} name={`${name}'s results`} />
    <span
      class={["size-2.5 rounded-full", response.hasFreeUploadSlot ? "bg-success" : "bg-warning"]}
      title={response.hasFreeUploadSlot ? "Has a free upload slot" : "No free upload slot"}></span>
    <Username {name} />
    {#if user}
      <span
        class={[
          "size-2 rounded-full",
          user.presence === "Online"
            ? "bg-success"
            : user.presence === "Away"
              ? "bg-warning"
              : "bg-fg-subtle/50"
        ]}
        title={user.presence ?? "Unknown"}></span>
    {/if}
    {#if user?.group.color}
      <span
        class="size-2 rounded-full"
        style:background-color={user.group.color}
        title={`Group ${user.group.name}`}></span>
    {/if}
    {#if response.country}<Country code={response.country} />{/if}
    {#if user?.block}<Badge tone="danger">blocked</Badge>{/if}
    <span class="text-xs text-fg-muted tabular-nums">
      {formatBytes(response.uploadSpeed)}/s · {response.hasFreeUploadSlot
        ? "free slot"
        : "no free slot"} · {response.queueLength} queued · {plural(files.length, "file", "files")}
    </span>
    <div class="ml-auto flex items-center gap-1">
      <Button variant="ghost" size="sm" href={href(`/browse?user=${encodeURIComponent(name)}`)}>
        <FolderOpenIcon />Browse
      </Button>
      {#if canWrite}
        <Button variant="ghost" size="sm" href={href(`/searches?user=${encodeURIComponent(name)}`)}>
          <SearchIcon />More from this user
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={!connected || downloading}
          title={`${plural(files.length, "file", "files")}, ${formatBytes(allBytes)}`}
          onclick={() => void download(files)}>
          <DownloadIcon />Download all
        </Button>
      {/if}
      <Button variant="ghost" size="icon-sm" aria-label="Hide this result" onclick={ondismiss}>
        <XIcon class="text-danger" />
      </Button>
    </div>
  </header>

  {#if open}
    <FolderTree
      class="border-t"
      {lines}
      expansion={folders}
      label={`Files from ${name}`}
      columns={COLUMNS}
      {selected}
      idOf={(f) => f.filename}
      onopen={write ? (f) => void download([f]) : undefined}
      onmiddleclick={write ? (f) => void download([f]) : undefined}>
      {#snippet folder(node, depth)}
        {@const inside = under(node)}
        {@const locked = inside.every((f) => f.locked)}
        <Checkbox
          aria-label={`Select every file in ${node.label}`}
          checked={inside.every((f) => selected.has(f.filename))}
          indeterminate={inside.some((f) => selected.has(f.filename)) &&
            !inside.every((f) => selected.has(f.filename))}
          onCheckedChange={(on) =>
            tick(
              inside.map((f) => f.filename),
              on
            )} />
        {#if locked}<LockIcon class="size-3.5 text-fg-muted" />{/if}
        <span
          class={["min-w-0 truncate font-mono text-xs", depth === 0 ? "text-fg" : "text-fg-muted"]}
          title={node.path}>{node.label}</span>
        <span class="shrink-0 text-xs text-fg-subtle tabular-nums">
          {plural(node.count, "file", "files")} · {formatBytes(node.bytes)}
        </span>
        <div class="ml-auto flex shrink-0 items-center gap-0.5 row-action">
          {#if canWrite}
            <Button
              variant="ghost"
              size="sm"
              class="h-7"
              disabled={!connected || downloading}
              onclick={() => void download(inside)}>
              <DownloadIcon />Download folder
              <span class="font-normal text-fg-muted">
                {node.count}, {formatBytes(node.bytes)}
              </span>
            </Button>
          {/if}
          {#if node.items.length}
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Add the folder's other files to this result"
              title="Add the folder's other files to this result"
              disabled={asking !== null}
              onclick={() => void askFolder(node.path)}>
              {#if asking === node.path}<Spinner class="size-4" />{:else}<SearchIcon />{/if}
            </Button>
          {/if}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Open the folder's full file list"
            title="Open the folder's full file list"
            disabled={asking !== null}
            onclick={() => void viewAll(node.path)}>
            <ListIcon />
          </Button>
        </div>
      {/snippet}
      {#snippet item(file)}
        <span role="gridcell" class="flex justify-center">
          <Checkbox
            aria-label={`Select ${getFileName(file.filename)}`}
            checked={selected.has(file.filename)}
            onCheckedChange={(on) => tick([file.filename], on)} />
        </span>
        <span
          role="gridcell"
          class="flex min-w-0 items-center gap-1 pr-2"
          style:padding-left="calc((var(--depth) - 1) * 1.25rem + 0.25rem)"
          title={file.filename}>
          {#if file.locked}<LockIcon class="size-3.5 shrink-0 text-fg-muted" />{/if}
          <span class="truncate">{getFileName(file.filename)}</span>
        </span>
        <span role="gridcell" class="pr-3 text-right tabular-nums">{formatBytes(file.size)}</span>
        <span role="gridcell" class="flex min-w-0 items-center gap-1 truncate text-fg-muted">
          {#if file.suspect}<Suspect suspect={file.suspect} />{/if}
          {formatAttributes(file)}
        </span>
        <span role="gridcell" class="pr-3 text-right text-fg-muted tabular-nums"
          >{formatSeconds(file.length)}</span>
        <span role="gridcell" class="flex justify-end pr-1">
          <FileMenu
            path={file.filename}
            username={name}
            ondownload={write ? () => void download([file]) : undefined}>
            {#snippet trigger({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-sm"
                class="row-action"
                aria-label={`Actions for ${getFileName(file.filename)}`}>
                <EllipsisIcon />
              </Button>
            {/snippet}
          </FileMenu>
        </span>
      {/snippet}
    </FolderTree>
  {/if}
  {#if canWrite && chosen.length}
    <footer class="flex flex-wrap items-center gap-inline border-t px-4 py-3">
      <Button disabled={!connected || downloading} onclick={() => void download(chosen)}>
        <DownloadIcon />Download
        <span class="font-normal opacity-80">
          {plural(chosen.length, "file", "files")}, {formatBytes(chosenBytes)}
        </span>
      </Button>
      {#if downloading}
        <Spinner class="size-4" label="Queueing" />
      {:else if status?.kind === "done"}
        <CheckIcon class="size-4 text-success" aria-label={status.text} />
      {:else if status}
        <span class="flex items-center gap-1 text-sm text-danger">
          <XIcon class="size-4" />{status.text}
        </span>
      {/if}
      <Button variant="ghost" size="sm" onclick={() => selected.clear()}>Clear</Button>
    </footer>
  {/if}
</section>

{#if flat}
  <FolderFiles
    bind:open={flatOpen}
    username={name}
    folder={flat.folder}
    files={flat.files}
    {connected} />
{/if}
