<script lang="ts" generics="T">
  import DownloadIcon from "@lucide/svelte/icons/download";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import LockIcon from "@lucide/svelte/icons/lock";
  import type { Snippet } from "svelte";

  import Button from "#lib/components/ui/Button.svelte";
  import Checkbox from "#lib/components/ui/Checkbox.svelte";
  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { folderCaption } from "#lib/files/summary.js";
  import type { FolderNode } from "#lib/tree/folders.js";

  import { LOCKED_HINT } from "./locked";

  type Props = {
    node: FolderNode<T>;
    open: boolean;
    depth: number;
    /** the folder's tick over the files under it; none for a folder nothing in can be ticked */
    tick?: { checked: boolean; indeterminate: boolean; ontick: (on: boolean) => void };
    /** downloads every file under the folder */
    ondownload?: () => void;
    /** the folder's other buttons, shown before its menu */
    actions?: Snippet;
    /** the items of the folder's menu */
    menu?: Snippet;
  };

  let { node, open, depth, tick, ondownload, actions, menu }: Props = $props();
</script>

<!-- a folder's line after its toggle: its tick, icon, name and what it holds, then its actions -->
{#if tick && !node.locked}
  <Checkbox
    aria-label={`Select every file in ${node.label}`}
    checked={tick.checked}
    indeterminate={tick.indeterminate}
    onCheckedChange={tick.ontick} />
{/if}
<span
  class={["flex min-w-0 flex-1 items-center gap-1.5", node.locked && "opacity-50"]}
  title={node.locked ? LOCKED_HINT : undefined}>
  {#if node.locked}
    <LockIcon class="size-4 shrink-0 text-fg-muted" />
  {:else if open}
    <FolderOpenIcon class="size-4 shrink-0 text-fg-muted" />
  {:else}
    <FolderIcon class="size-4 shrink-0 text-fg-muted" />
  {/if}
  <span class="name-count gap-1.5">
    <span
      class={["truncate", depth === 0 ? "text-fg" : "text-fg-muted"]}
      title={node.locked ? undefined : node.path}>{node.label}</span>
    <span class="text-xs text-fg-subtle"><span>{folderCaption(node)}</span></span>
  </span>
</span>
<div class="flex shrink-0 items-center gap-0.5 row-action">
  {#if ondownload && !node.locked}
    <Button
      variant="ghost"
      size="sm"
      class="h-7"
      aria-label={`Download this folder, ${folderCaption(node)}`}
      onclick={ondownload}>
      <!-- a phone shows the icon alone, the line having no room for the words -->
      <DownloadIcon /><span class="max-md:hidden">Download this folder</span>
    </Button>
  {/if}
  {@render actions?.()}
  {#if menu}
    <Menu.LazyRoot>
      {#snippet trigger({ props })}
        <Button {...props} variant="ghost" size="icon-sm" aria-label={`More for ${node.label}`}>
          <EllipsisIcon />
        </Button>
      {/snippet}
      <Menu.Content align="end" class="min-w-48">
        {@render menu()}
      </Menu.Content>
    </Menu.LazyRoot>
  {/if}
</div>
