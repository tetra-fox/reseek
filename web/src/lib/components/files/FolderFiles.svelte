<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import { SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { fileColumns, fileTemplate, fileWidth } from "#lib/files/columns.js";
  import {
    downloadFolderPlan,
    downloadPlan,
    type Enqueued,
    enqueuePlan
  } from "#lib/files/downloads.js";
  import { fileMatches, parseFilters } from "#lib/files/filters.js";
  import { getFileName } from "#lib/format.js";
  import { session } from "#lib/session.svelte.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import {
    type FolderNode,
    folderTree,
    itemsUnder,
    separatorOf,
    tickedCounts,
    tickedWhole,
    treeLines
  } from "#lib/tree/folders.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  import FileCells from "./FileCells.svelte";
  import FileHeader from "./FileHeader.svelte";
  import FileTree from "./FileTree.svelte";
  import FolderLine from "./FolderLine.svelte";
  import RemoteFileItems from "./RemoteFileItems.svelte";
  import RemoteFolderItems from "./RemoteFolderItems.svelte";
  import SelectionBar from "./SelectionBar.svelte";

  type Props = {
    open: boolean;
    username: string;
    /** the remote folder, whose path the files' paths start with */
    folder: string;
    /** every file under it, each `filename` the full remote path */
    files: Schemas["File"][];
    /** the app is connected to the Soulseek server, which a download needs */
    connected: boolean;
    /** the search the folder was found by */
    searchId?: string;
  };

  let { open = $bindable(), username, folder, files, connected, searchId }: Props = $props();

  let text = $state("");
  const selected = new SvelteSet<string>();
  /** every folder opens, so the dialog shows every file */
  const expansion = new Expansion(true);
  const write = $derived(session.can("ReadWrite") && connected);

  const shown = $derived.by(() => {
    const filters = parseFilters(text);
    return files.filter((f) => fileMatches(filters, f));
  });
  const tree = $derived(
    folderTree(
      shown,
      (f) => f.filename,
      (f) => f.size
    )
  );
  const lines = $derived(treeLines(tree, expansion.isOpen, (f) => f.filename));
  /** each folder of the tree by its path */
  const byPath = $derived.by(() => {
    const found = new Map<string, FolderNode<Schemas["File"]>>();
    const stack = [...tree];
    while (stack.length) {
      const node = stack.pop()!;
      found.set(node.path, node);
      stack.push(...node.children);
    }
    return found;
  });
  const columns = $derived(fileColumns({ quality: true, phone: viewport.phone }));
  const chosen = $derived(shown.filter((f) => selected.has(f.filename)));
  /** the ticked files under each folder, of those the filter keeps */
  const ticked = $derived(tickedCounts(chosen.map((f) => f.filename)));
  const separator = $derived(separatorOf(folder));

  const tell = (result: Enqueued) => toast(result.text, result.kind === "done" ? "info" : "error");

  /** A folder line's tick: how many of the files under it are ticked, of how many are shown */
  const tickOf = (node: FolderNode<Schemas["File"]>) => ({
    on: ticked.get(node.path) ?? 0,
    of: node.count
  });

  /** Whether the line of the folder `path` is ticked */
  const lineTicked = (path: string) => {
    const node = byPath.get(path);
    return !!node && tickedWhole(tickOf(node));
  };

  /**
   * Downloads files as one job that keeps their folders below the deepest one holding them, or
   * that folder itself when `whole` says its line is ticked
   */
  const download = (
    list: Schemas["File"][],
    whole?: (folder: string) => boolean
  ): Promise<Enqueued> =>
    enqueuePlan(
      username,
      downloadPlan(list, () => folder, separator, whole),
      searchId
    );
  const downloadNow = async (list: Schemas["File"][]) => tell(await download(list));
  /** Every file under a folder as one job that keeps the folders below it */
  const downloadFolder = async (path: string, list: Schemas["File"][]) =>
    tell(await enqueuePlan(username, downloadFolderPlan(path, list, separator), searchId));
</script>

{#snippet folderItems(node: FolderNode<Schemas["File"]>)}
  <RemoteFolderItems
    folder={node.path}
    {username}
    ondownload={write ? () => void downloadFolder(node.path, itemsUnder(node)) : undefined} />
{/snippet}

<Dialog bind:open title={folder} class="w-[min(56rem,calc(100vw-2rem))]">
  {#snippet description()}
    Every file in this folder and its subfolders, from <Username name={username} />
  {/snippet}
  <Input bind:value={text} type="search" placeholder="Filter files" aria-label="Filter" />
  <FileTree
    {lines}
    {expansion}
    label="Files"
    columns={fileTemplate(columns)}
    width={fileWidth(columns)}
    height="min(55vh, 32rem)"
    {selected}
    idOf={(f) => f.filename}
    nameOf={(line) => (line.kind === "folder" ? line.node.label : getFileName(line.item.filename))}
    onopen={write ? (f) => void downloadNow([f]) : undefined}>
    {#snippet header(all)}
      <FileHeader {columns} {all} />
    {/snippet}
    {#snippet folder(node, depth)}
      {@const state = tickOf(node)}
      <FolderLine
        {node}
        open={expansion.isOpen(node.key)}
        {depth}
        tick={{
          checked: tickedWhole(state),
          indeterminate: state.on > 0 && state.on < state.of,
          ontick: (on) => {
            for (const f of itemsUnder(node)) {
              if (on) selected.add(f.filename);
              else selected.delete(f.filename);
            }
          }
        }}>
        {#snippet menu()}
          {@render folderItems(node)}
        {/snippet}
      </FolderLine>
    {/snippet}
    {#snippet item(file, _depth, tick)}
      <FileCells
        {file}
        {username}
        {columns}
        selected={selected.has(file.filename)}
        ontick={tick}
        ondownload={write ? () => void downloadNow([file]) : undefined} />
    {/snippet}
    <!-- a right-click, a long press or the menu key opens what the line's ... button offers -->
    {#snippet menu(line)}
      {#if line.kind === "item"}
        <RemoteFileItems
          path={line.item.filename}
          {username}
          ondownload={write ? () => void downloadNow([line.item]) : undefined} />
      {:else}
        {@render folderItems(line.node)}
      {/if}
    {/snippet}
  </FileTree>
  {#if write && chosen.length}
    <SelectionBar
      files={chosen}
      ondownload={() => download(chosen, lineTicked)}
      onclear={() => selected.clear()} />
  {/if}
  {#snippet footer()}
    {#if session.can("ReadWrite")}
      <Button
        disabled={!connected || !files.length}
        onclick={() => void downloadFolder(folder, files)}>
        <DownloadIcon />Download all
      </Button>
    {/if}
  {/snippet}
</Dialog>
