<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import CopyIcon from "@lucide/svelte/icons/copy";
  import CornerDownRightIcon from "@lucide/svelte/icons/corner-down-right";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileAudioIcon from "@lucide/svelte/icons/file-audio";
  import FileVideoIcon from "@lucide/svelte/icons/file-video";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderClosedIcon from "@lucide/svelte/icons/folder-closed";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import LinkIcon from "@lucide/svelte/icons/link";
  import ListIcon from "@lucide/svelte/icons/list";
  import LockIcon from "@lucide/svelte/icons/lock";
  import XIcon from "@lucide/svelte/icons/x";

  import type { Schemas } from "#lib/api/client.js";
  import { browser } from "#lib/browse/browse.svelte.js";
  import type { BrowseFilter } from "#lib/browse/filter.js";
  import { attributesOf, kindOf, lengthOf, sizeOf } from "#lib/browse/summary.js";
  import { downloadGroups, filesUnder, leaf, type TreeNode } from "#lib/browse/tree.js";
  import { copyText } from "#lib/clipboard.svelte.js";
  import FileMenu from "#lib/components/files/FileMenu.svelte";
  import FolderFiles from "#lib/components/files/FolderFiles.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import { asItem, type Enqueued, enqueue } from "#lib/downloads.js";
  import {
    formatAttributes,
    formatBytes,
    formatSeconds,
    getFileName,
    plural
  } from "#lib/format.js";
  import { slskLink } from "#lib/slsk.js";

  import { LOCKED_HINT } from "./locked";

  type Props = {
    node: TreeNode;
    username: string;
    filter: BrowseFilter | null;
    canWrite: boolean;
    /** downloads need the Soulseek server */
    connected: boolean;
    /** selects a folder of the tree and scrolls the tree to it */
    onnavigate: (name: string) => void;
  };

  let { node, username, filter, canWrite, connected, onnavigate }: Props = $props();

  const sep = $derived(browser.separator);
  let folded = $state(false);
  let downloading = $state(false);
  let status = $state<Enqueued | null>(null);
  let flatOpen = $state(false);

  const kept = (files: Schemas["File"][]) =>
    filter ? files.filter((f) => filter.file(f, f.filename)) : files;
  const byName = (a: Schemas["File"], b: Schemas["File"]) =>
    getFileName(a.filename).localeCompare(getFileName(b.filename));

  /** every file in the folder and under it, with full paths: what the header box ticks */
  const candidates = $derived(kept(filesUnder(node, sep)));
  const own = $derived(
    kept(node.files.map((f) => ({ ...f, filename: node.name + sep + f.filename }))).toSorted(byName)
  );
  const children = $derived(
    node.children.map((child) => ({ child, files: kept(filesUnder(child, sep)) }))
  );
  const ticked = $derived(candidates.filter((f) => browser.selected.has(f.filename)));
  const tickedBytes = $derived(sizeOf(ticked));

  const allTicked = (files: Schemas["File"][]) =>
    files.length > 0 && files.every((f) => browser.selected.has(f.filename));
  const someTicked = (files: Schemas["File"][]) =>
    files.some((f) => browser.selected.has(f.filename)) && !allTicked(files);
  const tick = (files: Schemas["File"][], on: boolean) =>
    browser.tick(
      files.map((f) => f.filename),
      on
    );

  const icons = { audio: FileAudioIcon, video: FileVideoIcon, other: FileIcon };

  /**
   * Downloads the ticked files: those in the folder itself with the usual pattern, and each
   * subfolder's as a batch of its own that keeps the subfolder. The first answer that is not a
   * full success stops the rest
   */
  const download = async (files: Schemas["File"][]) => {
    downloading = true;
    status = null;
    let last: Enqueued | null = null;
    for (const group of downloadGroups(node.name, files, sep)) {
      last = await enqueue({
        username,
        files: group.files.map((f) => asItem(f)),
        destination: group.destination ?? undefined
      });
      if (last.kind !== "done") break;
    }
    status = last;
    downloading = false;
  };

  /** Every file under the folder, filtered or not, as one batch in a folder named like it */
  const downloadFolder = async () => {
    downloading = true;
    status = await enqueue({
      username,
      files: filesUnder(node, sep).map((f) => asItem(f)),
      destination: leaf(node.name)
    });
    downloading = false;
  };
</script>

<section
  class={["flex flex-col rounded-card border bg-surface shadow-card", node.locked && "opacity-50"]}>
  <!-- the folder's name and its download stay in view while its files scroll -->
  <header
    class="sticky top-14 z-[4] flex flex-wrap items-center gap-2 rounded-t-card border-b bg-surface/95 px-4 py-3 backdrop-blur-md">
    {#if node.locked}
      <span title={LOCKED_HINT}><LockIcon class="size-5 text-fg-muted" /></span>
    {:else}
      <button
        type="button"
        aria-label={folded ? "Unfold" : "Fold"}
        aria-expanded={!folded}
        onclick={() => (folded = !folded)}>
        {#if folded}<FolderIcon class="size-5" />{:else}<FolderOpenIcon class="size-5" />{/if}
      </button>
    {/if}
    <h3 class="min-w-0 flex-1 font-mono text-sm wrap-anywhere">{node.name}</h3>
    <div class="flex flex-wrap items-center gap-1">
      {#if canWrite}
        <Button
          variant="ghost"
          size="sm"
          disabled={!connected || downloading || !node.totalFileCount}
          onclick={() => void downloadFolder()}>
          <DownloadIcon />Download this folder
          <span class="font-normal text-fg-muted">
            {plural(node.totalFileCount, "file", "files")}, {formatBytes(node.totalBytes)}
          </span>
        </Button>
      {/if}
      <Button variant="ghost" size="sm" onclick={() => (flatOpen = true)}>
        <ListIcon />View all files
      </Button>
      <Button variant="ghost" size="sm" onclick={() => void copyText(node.name)}>
        <CopyIcon />Copy path
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Copy the slsk:// link"
        title="Copy the slsk:// link"
        onclick={() => void copyText(slskLink(username, node.name, true))}>
        <LinkIcon />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Close the folder"
        onclick={() => browser.select(null)}>
        <XIcon class="text-danger" />
      </Button>
    </div>
  </header>
  {#if !folded}
    <div class="overflow-x-auto border-t">
      <table class="data-table w-full text-sm">
        <thead>
          <tr>
            <th class="w-10">
              <Checkbox
                aria-label="Select every file in this folder and its subfolders"
                checked={allTicked(candidates)}
                indeterminate={someTicked(candidates)}
                onCheckedChange={(on) => tick(candidates, on)} />
            </th>
            <th>{node.children.length ? "Name" : "File"}</th>
            <th class="w-24 text-right">Size</th>
            <th class="w-28">Attributes</th>
            <th class="w-20 text-right">Length</th>
            <th class="w-10"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each children as { child, files } (child.name)}
            {@const open = browser.subdirectory === child.name}
            <tr
              class="group cursor-pointer"
              onclick={() => browser.expand(open ? null : child.name)}>
              <td>
                <Checkbox
                  aria-label={`Select every file in ${leaf(child.name)}`}
                  checked={allTicked(files)}
                  indeterminate={someTicked(files)}
                  onclick={(e) => e.stopPropagation()}
                  onCheckedChange={(on) => tick(files, on)} />
              </td>
              <td class="max-w-0 min-w-48">
                <span class="flex items-center gap-1.5">
                  {#if open}<FolderOpenIcon class="size-4 shrink-0" />{:else}<FolderIcon
                      class="size-4 shrink-0" />{/if}
                  <span class="truncate">{leaf(child.name)}</span>
                  <span class="shrink-0 text-xs text-fg-muted">
                    {child.totalFileCount
                      ? plural(child.totalFileCount, "file", "files")
                      : "0 files"}{child.totalDirectoryCount
                      ? `, ${plural(child.totalDirectoryCount, "folder", "folders")}`
                      : ""}
                  </span>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    class="ml-auto row-action"
                    aria-label="Open this folder"
                    onclick={(e) => {
                      e.stopPropagation();
                      onnavigate(child.name);
                    }}>
                    <CornerDownRightIcon />
                  </Button>
                </span>
              </td>
              <td class="text-right tabular-nums">{formatBytes(sizeOf(files))}</td>
              <td class="text-fg-muted">{attributesOf(files)}</td>
              <td class="text-right text-fg-muted tabular-nums">
                {lengthOf(files) ? formatSeconds(lengthOf(files)) : ""}
              </td>
              <td></td>
            </tr>
            {#if open}
              {#each child.children as grandchild (grandchild.name)}
                {@const under = kept(filesUnder(grandchild, sep))}
                <tr class="text-fg-muted">
                  <td></td>
                  <td class="pl-10">
                    <span class="flex items-center gap-1.5">
                      <FolderClosedIcon class="size-4 shrink-0" />
                      <span class="truncate">{leaf(grandchild.name)}</span>
                      <span class="text-xs"
                        >{plural(grandchild.totalFileCount, "file", "files")}</span>
                    </span>
                  </td>
                  <td class="text-right tabular-nums">{formatBytes(sizeOf(under))}</td>
                  <td>{attributesOf(under)}</td>
                  <td class="text-right tabular-nums">
                    {lengthOf(under) ? formatSeconds(lengthOf(under)) : ""}
                  </td>
                  <td></td>
                </tr>
              {/each}
              {#each kept(child.files.map( (f) => ({ ...f, filename: child.name + sep + f.filename }) )).toSorted(byName) as file (file.filename)}
                {@const Icon = icons[kindOf(file.filename)]}
                <tr class="text-fg-muted">
                  <td></td>
                  <td class="max-w-0 min-w-48 pl-10">
                    <span class="flex items-center gap-1.5">
                      <Icon class="size-4 shrink-0" />
                      <span class="truncate">{getFileName(file.filename)}</span>
                    </span>
                  </td>
                  <td class="text-right tabular-nums">{formatBytes(file.size)}</td>
                  <td>{formatAttributes(file)}</td>
                  <td class="text-right tabular-nums">{formatSeconds(file.length)}</td>
                  <td></td>
                </tr>
              {/each}
            {/if}
          {/each}
          {#each own as file (file.filename)}
            {@const Icon = node.locked ? LockIcon : icons[kindOf(file.filename)]}
            <tr class="group">
              <td>
                <Checkbox
                  aria-label={`Select ${getFileName(file.filename)}`}
                  checked={browser.selected.has(file.filename)}
                  onCheckedChange={(on) => tick([file], on)} />
              </td>
              <td class="max-w-0 min-w-48" title={file.filename}>
                <span class="flex items-center gap-1.5">
                  <Icon class="size-4 shrink-0 text-fg-muted" />
                  <span class="truncate">{getFileName(file.filename)}</span>
                </span>
              </td>
              <td class="text-right tabular-nums">{formatBytes(file.size)}</td>
              <td class="text-fg-muted">{formatAttributes(file)}</td>
              <td class="text-right text-fg-muted tabular-nums">{formatSeconds(file.length)}</td>
              <td class="text-right">
                <FileMenu
                  path={file.filename}
                  {username}
                  ondownload={canWrite && connected ? () => void download([file]) : undefined}>
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
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  {#if canWrite && ticked.length}
    <footer class="flex flex-wrap items-center gap-inline border-t px-4 py-3">
      <Button disabled={!connected || downloading} onclick={() => void download(ticked)}>
        <DownloadIcon />Download
        <span class="font-normal opacity-80">
          {plural(ticked.length, "file", "files")}, {formatBytes(tickedBytes)}
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
    </footer>
  {/if}
</section>

<FolderFiles bind:open={flatOpen} {username} folder={node.name} files={candidates} {connected} />
