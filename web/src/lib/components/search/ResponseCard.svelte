<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import ListIcon from "@lucide/svelte/icons/list";
  import LockIcon from "@lucide/svelte/icons/lock";
  import SearchIcon from "@lucide/svelte/icons/search";
  import XIcon from "@lucide/svelte/icons/x";
  import { SvelteSet } from "svelte/reactivity";

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
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { fetchFolder } from "#lib/folders.js";
  import {
    columnWidth,
    formatAttributes,
    formatBytes,
    formatSeconds,
    getFileName,
    plural,
    WIDEST
  } from "#lib/format.js";
  import { href } from "#lib/nav.js";
  import { type Result, type ResultFile, resultFiles } from "#lib/search/results.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import { type FolderNode, folderTree, treeLines } from "#lib/tree/folders.js";
  import { TICK_REM } from "#lib/tree/indent.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";
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

  const response = $derived(result.response);
  const name = $derived(response.username);
  const user = $derived(users.get(name));
  const open = $derived(cards.isOpen(result.key));

  const selected = new SvelteSet<string>();
  const folders = new Expansion(true);
  let asking = $state<string | null>(null);
  let downloading = $state(false);
  let status = $state<Enqueued | null>(null);
  let flat = $state<{ folder: string; files: Schemas["File"][] } | null>(null);
  let flatOpen = $state(false);
  /** the height of the card's header, which stays in view on a wider screen */
  let headerHeight = $state(0);
  /** the height of the user's line, the part of the header that stays in view on a phone */
  let userHeight = $state(0);

  $effect(() => void users.load(name));

  const files = $derived(resultFiles(response));
  const attributed = $derived(files.some((f) => f.suspect || formatAttributes(f)));

  /** the file table's columns: tick, name, size, attributes, length, menu */
  // the figures hold their widest value in the rows' own font, which the cells share, and the name
  // takes what is left, so a large text size never pushes the menu out of the card. A card whose
  // files say nothing of their quality has no attributes column. A phone's rows show the tick, the
  // name with its size under it, and the menu
  const columns = $derived(
    `${TICK_REM}rem minmax(0, 1fr) ${columnWidth(WIDEST.size)} ${attributed ? "9rem " : ""}${columnWidth(WIDEST.length)} 2.5rem`
  );
  const PHONE_COLUMNS = `${TICK_REM}rem minmax(0, 1fr) 2.5rem`;
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

  /** Lists every file under the folder, asked of the user so files the search missed are in */
  const showFolder = async (folder: string) => {
    asking = folder;
    const outcome = await fetchFolder(name, folder);
    asking = null;
    if (!outcome.ok) {
      toast(`Could not load the folder: ${outcome.error}`, "error");
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

<!-- the header stays in view under the results toolbar while the card's files scroll, and the
     folders' lines stack under it; a phone keeps only the user's line of it, the figures and
     actions scrolling away. The card clips rather than hides its overflow, since a box that hides
     it is one its pinned lines would stay within -->
<section
  class="flex flex-col overflow-clip rounded-card border bg-surface shadow-card [contain-intrinsic-size:auto_12rem] [content-visibility:auto]"
  data-key={result.key}>
  <header
    class={[
      "max-md:contents md:sticky md:z-[1] md:flex md:flex-wrap md:items-center md:gap-2 md:bg-surface md:px-3 md:py-2.5",
      open && "md:border-b"
    ]}
    style:top="var(--results-top, 0px)"
    bind:offsetHeight={headerHeight}>
    <div
      class="flex min-w-0 flex-wrap items-center gap-2 max-md:sticky max-md:z-[1] max-md:bg-surface max-md:px-3 max-md:py-2.5"
      style:top="var(--results-top, 0px)"
      bind:offsetHeight={userHeight}>
      <TreeToggle expansion={cards} key={result.key} {siblings} name={`${name}'s results`} />
      <span
        class={["size-2.5 rounded-full", response.hasFreeUploadSlot ? "bg-success" : "bg-warning"]}
        title={response.hasFreeUploadSlot ? "Has a free upload slot" : "No free upload slot"}
      ></span>
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
    </div>
    <div
      class={[
        "flex flex-wrap items-center gap-2 max-md:px-3 max-md:pb-2.5 md:contents",
        open && "max-md:border-b"
      ]}>
      <span class="text-xs text-fg-muted tabular-nums">
        {formatBytes(response.uploadSpeed)}/s · {response.hasFreeUploadSlot
          ? "free slot"
          : "no free slot"} · {response.queueLength} queued · {plural(
          files.length,
          "file",
          "files"
        )}
      </span>
      <div class="ml-auto flex flex-wrap items-center justify-end gap-1">
        <Button variant="ghost" size="sm" href={href(`/browse?user=${encodeURIComponent(name)}`)}>
          <FolderOpenIcon />Browse
        </Button>
        {#if canWrite}
          <Button
            variant="ghost"
            size="sm"
            href={href(`/searches?user=${encodeURIComponent(name)}`)}>
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
    </div>
  </header>

  {#if open}
    <FolderTree
      {lines}
      pin={`calc(var(--results-top, 0px) + ${viewport.phone ? userHeight : headerHeight}px)`}
      expansion={folders}
      label={`Files from ${name}`}
      columns={viewport.phone ? PHONE_COLUMNS : columns}
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
        {#if locked}<LockIcon class="size-3.5 shrink-0 text-fg-muted" />{/if}
        <div class="name-count gap-1.5">
          <span
            class={["truncate font-mono text-xs", depth === 0 ? "text-fg" : "text-fg-muted"]}
            title={node.path}>{node.label}</span>
          <span class="text-xs text-fg-subtle"
            ><span>{plural(node.count, "file", "files")} · {formatBytes(node.bytes)}</span></span>
        </div>
        <div class="ml-auto flex shrink-0 items-center gap-0.5 row-action">
          {#if canWrite}
            <Button
              variant="ghost"
              size="sm"
              class="h-7"
              disabled={!connected || downloading}
              aria-label={`Download folder, ${plural(node.count, "file", "files")}, ${formatBytes(node.bytes)}`}
              onclick={() => void download(inside)}>
              <!-- a phone shows the icon alone, the row having no room for the words -->
              <DownloadIcon /><span class="max-md:hidden">Download folder</span>
              <span class="font-normal text-fg-muted max-md:hidden">
                {node.count}, {formatBytes(node.bytes)}
              </span>
            </Button>
          {/if}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Show whole folder"
            title="Show whole folder"
            disabled={asking !== null}
            onclick={() => void showFolder(node.path)}>
            {#if asking === node.path}<Spinner class="size-4" />{:else}<ListIcon />{/if}
          </Button>
        </div>
      {/snippet}
      {#snippet item(file)}
        <span role="gridcell" class="flex">
          <Checkbox
            aria-label={`Select ${getFileName(file.filename)}`}
            checked={selected.has(file.filename)}
            onCheckedChange={(on) => tick([file.filename], on)} />
        </span>
        <span role="gridcell" class="flex min-w-0 items-center gap-1 pr-2" title={file.filename}>
          {#if file.locked}<LockIcon class="size-3.5 shrink-0 text-fg-muted" />{/if}
          {#if viewport.phone}
            <!-- a phone gives the name the row's width, with the size under it -->
            <span class="flex min-w-0 flex-col py-1">
              <span class="truncate">{getFileName(file.filename)}</span>
              <span class="text-xs whitespace-nowrap text-fg-muted tabular-nums"
                >{formatBytes(file.size)}</span>
            </span>
          {:else}
            <span class="truncate">{getFileName(file.filename)}</span>
          {/if}
        </span>
        {#if !viewport.phone}
          <span role="gridcell" class="pr-3 text-right whitespace-nowrap tabular-nums"
            >{formatBytes(file.size)}</span>
        {/if}
        {#if attributed}
          <span
            role="gridcell"
            class="flex min-w-0 items-center gap-1 truncate text-fg-muted max-md:hidden">
            {#if file.suspect}<Suspect suspect={file.suspect} />{/if}
            {formatAttributes(file)}
          </span>
        {/if}
        <span
          role="gridcell"
          class="pr-3 text-right whitespace-nowrap text-fg-muted tabular-nums max-md:hidden"
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
