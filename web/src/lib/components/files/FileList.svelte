<script lang="ts">
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import LockIcon from "@lucide/svelte/icons/lock";
  import XIcon from "@lucide/svelte/icons/x";
  import type { Snippet } from "svelte";

  import type { Schemas } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import { formatAttributes, formatBytes, formatSeconds, getFileName } from "#lib/format.js";

  import FileMenu from "./FileMenu.svelte";
  import Suspect from "./Suspect.svelte";

  type ListedFile = Schemas["File"] & { locked?: boolean };

  type Props = {
    /** the folder's remote path */
    name: string;
    /** its files, each `filename` the full remote path */
    files: ListedFile[];
    /** whose share it is */
    username: string;
    /** full filenames of the files ticked */
    selected: Set<string>;
    /** ticks or unticks files; the header box hands every file at once */
    onselect: (filenames: string[], on: boolean) => void;
    /** the whole folder is locked */
    locked?: boolean;
    /** the folder starts folded */
    folded?: boolean;
    /** enqueues one file, from its row's menu */
    ondownload?: (file: Schemas["File"]) => void;
    onclose?: () => void;
    /** under the table, such as the button that asks the user for the rest of the folder */
    footer?: Snippet;
  };

  let {
    name,
    files,
    username,
    selected,
    onselect,
    locked = false,
    folded = false,
    ondownload,
    onclose,
    footer
  }: Props = $props();

  /** rows shown before "show all", so a peer listing thousands doesn't stall the page */
  const CAP = 500;

  // folds with the page's "fold results" and can be toggled on its own until that changes
  let fold = $derived(folded);
  let all = $state(false);

  const sorted = $derived(
    files.toSorted((a, b) => getFileName(a.filename).localeCompare(getFileName(b.filename)))
  );
  const rows = $derived(all ? sorted : sorted.slice(0, CAP));
  const ticked = $derived(files.filter((f) => selected.has(f.filename)).length);
</script>

<div class={["flex flex-col", locked && "opacity-50"]}>
  <div class="flex items-center gap-2 px-5 py-2 text-sm">
    {#if locked}
      <LockIcon class="size-4 shrink-0 text-fg-muted" />
    {:else}
      <button
        type="button"
        aria-label={fold ? "Unfold the folder" : "Fold the folder"}
        aria-expanded={!fold}
        onclick={() => (fold = !fold)}>
        {#if fold}<FolderIcon class="size-4 text-fg-muted" />{:else}<FolderOpenIcon
            class="size-4 text-fg-muted" />{/if}
      </button>
    {/if}
    <span class="min-w-0 flex-1 font-mono text-xs wrap-anywhere text-fg-muted">{name}</span>
    {#if onclose}
      <Button variant="ghost" size="icon-sm" aria-label="Close" onclick={onclose}>
        <XIcon class="text-danger" />
      </Button>
    {/if}
  </div>
  {#if !fold && files.length}
    <div class="overflow-x-auto">
      <table class="data-table w-full text-sm">
        <thead>
          <tr>
            <th class="w-10">
              <Checkbox
                aria-label="Select every file in the folder"
                checked={ticked === files.length}
                indeterminate={ticked > 0 && ticked < files.length}
                onCheckedChange={(on) =>
                  onselect(
                    files.map((f) => f.filename),
                    on
                  )} />
            </th>
            <th>File</th>
            <th class="w-24 text-right">Size</th>
            <th class="w-32">Attributes</th>
            <th class="w-20 text-right">Length</th>
            <th class="w-10"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as file (file.filename)}
            <tr class="group">
              <td>
                <Checkbox
                  aria-label={`Select ${getFileName(file.filename)}`}
                  checked={selected.has(file.filename)}
                  onCheckedChange={(on) => onselect([file.filename], on)} />
              </td>
              <td class="max-w-0 min-w-48 whitespace-normal" title={file.filename}>
                {#if file.locked}<LockIcon class="mr-1 inline size-3.5 text-fg-muted" />{/if}
                <span class="wrap-anywhere">{getFileName(file.filename)}</span>
              </td>
              <td class="text-right tabular-nums">{formatBytes(file.size)}</td>
              <td class="text-fg-muted">
                <span class="inline-flex items-center gap-1">
                  {#if file.suspect}<Suspect suspect={file.suspect} />{/if}
                  {formatAttributes(file)}
                </span>
              </td>
              <td class="text-right text-fg-muted tabular-nums">{formatSeconds(file.length)}</td>
              <td class="text-right">
                <FileMenu
                  path={file.filename}
                  {username}
                  ondownload={ondownload ? () => ondownload(file) : undefined}>
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
    {#if sorted.length > CAP && !all}
      <p class="px-5 py-2 text-sm text-fg-muted">
        {CAP} of {sorted.length} files shown.
        <Button variant="link" onclick={() => (all = true)}>Show all</Button>
      </p>
    {/if}
  {/if}
  {#if footer}<div class="flex flex-wrap items-center gap-inline px-5 py-2">
      {@render footer()}
    </div>{/if}
</div>
