<script lang="ts">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import { SvelteSet } from "svelte/reactivity";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import { toast } from "#lib/feedback/toast.svelte.js";
  import { fileColumns, fileTemplate, fileWidth } from "#lib/files/columns.js";
  import { asItem, type Enqueued, enqueue } from "#lib/files/downloads.js";
  import { fileMatches, parseFilters } from "#lib/files/filters.js";
  import { session } from "#lib/session.svelte.js";
  import { Expansion } from "#lib/tree/expansion.svelte.js";
  import {
    folderTree,
    itemsUnder,
    separatorOf,
    tickedCounts,
    treeLines
  } from "#lib/tree/folders.js";
  import { viewport } from "#lib/ui/viewport.svelte.js";

  import FileCells from "./FileCells.svelte";
  import FileHeader from "./FileHeader.svelte";
  import FileTree from "./FileTree.svelte";
  import FolderLine from "./FolderLine.svelte";
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
  };

  let { open = $bindable(), username, folder, files, connected }: Props = $props();

  let text = $state("");
  const selected = new SvelteSet<string>();
  /** every folder opens, so the dialog shows every file */
  const expansion = new Expansion(true);
  const write = $derived(session.can("ReadWrite") && connected);

  const shown = $derived.by(() => {
    const filters = parseFilters(text);
    return files.filter((f) => fileMatches(filters, f));
  });
  const lines = $derived(
    treeLines(
      folderTree(
        shown,
        (f) => f.filename,
        (f) => f.size
      ),
      expansion.isOpen,
      (f) => f.filename
    )
  );
  const columns = $derived(fileColumns({ quality: true, phone: viewport.phone }));
  const chosen = $derived(shown.filter((f) => selected.has(f.filename)));
  const ticked = $derived(tickedCounts(selected));
  /** the downloads land in a folder named like the remote one */
  const destination = $derived(folder.split(separatorOf(folder)).filter(Boolean).pop());

  const download = (list: Schemas["File"][]): Promise<Enqueued> =>
    enqueue({ username, files: list.map((f) => asItem(f)), destination });
  const downloadNow = async (list: Schemas["File"][]) => {
    const result = await download(list);
    toast(result.text, result.kind === "done" ? "info" : "error");
  };
</script>

<Dialog
  bind:open
  title={folder}
  description={`Every file in this folder and its subfolders, from ${username}`}
  class="w-[min(56rem,calc(100vw-2rem))]">
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
    onopen={write ? (f) => void downloadNow([f]) : undefined}>
    {#snippet header()}
      <FileHeader {columns} />
    {/snippet}
    {#snippet folder(node, depth)}
      <FolderLine
        {node}
        open={expansion.isOpen(node.key)}
        {depth}
        tick={{
          checked: (ticked.get(node.path) ?? 0) === node.count,
          indeterminate:
            (ticked.get(node.path) ?? 0) > 0 && (ticked.get(node.path) ?? 0) < node.count,
          ontick: (on) => {
            for (const f of itemsUnder(node)) {
              if (on) selected.add(f.filename);
              else selected.delete(f.filename);
            }
          }
        }} />
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
  </FileTree>
  {#if write && chosen.length}
    <SelectionBar
      files={chosen}
      ondownload={() => download(chosen)}
      onclear={() => selected.clear()} />
  {/if}
  {#snippet footer()}
    {#if session.can("ReadWrite")}
      <Button disabled={!connected || !files.length} onclick={() => void downloadNow(files)}>
        <DownloadIcon />Download all
      </Button>
    {/if}
  {/snippet}
</Dialog>
