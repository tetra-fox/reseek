<script lang="ts" module>
  const HEIGHT_KEY = "reseek-browse-tree-height";

  /** The tree's height saved in this browser, 300 px when none or nothing usable is */
  const savedHeight = (min: number) => {
    try {
      const height = Number.parseInt(localStorage.getItem(HEIGHT_KEY) ?? "", 10);
      return Number.isFinite(height) && height >= min ? height : 300;
    } catch {
      return 300;
    }
  };

  const saveHeight = (height: number) => {
    try {
      localStorage.setItem(HEIGHT_KEY, String(Math.round(height)));
    } catch {
      // not kept: the next visit starts at the saved or default height
    }
  };
</script>

<script lang="ts">
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderClosedIcon from "@lucide/svelte/icons/folder-closed";
  import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
  import LockIcon from "@lucide/svelte/icons/lock";
  import { tick, untrack } from "svelte";

  import type { BrowseFilter } from "#lib/browse/filter.js";
  import {
    ancestors,
    flatten,
    leaf,
    parentOf,
    type Separator,
    type Tree,
    type TreeNode
  } from "#lib/browse/tree.js";
  import { TYPE_AHEAD_MS, typeAhead } from "#lib/browse/typeahead.js";
  import ResizeHandle from "#lib/components/ui/ResizeHandle.svelte";
  import { formatBytes, plural } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { keepsKey, withModifier } from "#lib/keys.js";
  import { Expansion, withSiblings } from "#lib/tree/expansion.svelte.js";
  import { rem } from "#lib/viewport.svelte.js";
  import { ListWindow } from "#lib/virtual.svelte.js";

  import { LOCKED_HINT } from "./locked";

  type Props = {
    tree: Tree;
    separator: Separator;
    /** the folder shown in the selection card */
    selected: string | null;
    onselect: (name: string) => void;
    filter: BrowseFilter | null;
  };

  let { tree, separator, selected, onselect, filter }: Props = $props();

  const expansion = new Expansion(false);
  let scroller = $state<HTMLDivElement | null>(null);
  /** one row's height, 2.0625rem in pixels; the tree is never shorter than a row */
  const row = $derived(2.0625 * rem.px);
  // svelte-ignore state_referenced_locally
  let height = $state(savedHeight(row));

  // a new listing starts with its first folder open, a hint that folders open
  $effect(() => {
    const first = tree.roots[0];
    untrack(() => {
      expansion.reset(false);
      if (first?.children.length) expansion.set(first.name, true);
      if (scroller) scroller.scrollTop = 0;
    });
  });

  const rows = $derived(flatten(tree.roots, expansion.isOpen, filter?.folder));
  const lines = new ListWindow(
    () => rows,
    (r) => r.node.name,
    () => ({ estimateSize: () => row, overscan: 15, sizes: row })
  );

  const caption = (files: number, folders: number) =>
    [
      files ? plural(files, "file", "files") : "",
      folders ? plural(folders, "folder", "folders") : ""
    ]
      .filter(Boolean)
      .join(", ");

  /** The folders beside a folder, itself among them, which Alt-click opens or closes with it */
  const siblings = (node: TreeNode) =>
    (tree.byName.get(parentOf(node.name, separator))?.children ?? tree.roots).map((n) => n.name);

  export const expandAll = () => expansion.reset(true);
  export const collapseAll = () => expansion.reset(false);

  /** Scrolls the row at `index` to the middle of the list */
  const center = (index: number) => {
    if (index !== -1) lines.scrollTo(index, "center");
  };

  // letters typed with the list focused select the next folder starting with them
  let typed = "";
  let typedAt = 0;
  const typeToFind = (e: KeyboardEvent) => {
    if (e.key.length !== 1 || withModifier(e) || keepsKey(e) || e.key === " ") return;
    const now = Date.now();
    typed = now - typedAt > TYPE_AHEAD_MS ? e.key : typed + e.key;
    typedAt = now;
    const from = rows.findIndex((r) => r.node.name === selected);
    const at = typeAhead(
      rows.map((r) => leaf(r.node.name)),
      from,
      typed
    );
    if (at === -1) return;
    e.preventDefault();
    onselect(rows[at].node.name);
    center(at);
  };

  /** Opens every folder above `name`, selects it and scrolls it to the middle */
  export const navigate = async (name: string) => {
    for (const above of ancestors(name, tree, separator)) expansion.set(above, true);
    onselect(name);
    await tick();
    center(rows.findIndex((r) => r.node.name === name));
  };
</script>

<div
  bind:this={scroller}
  class="relative overflow-y-auto outline-none [overflow-anchor:auto] focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
  style:height={`${height}px`}
  role="tree"
  aria-label="Shared folders"
  tabindex="0"
  title="Type the first letters of a folder name to go to it"
  onkeydown={typeToFind}
  {@attach lines.attach}
  {@attach freezesWhileUsed}>
  <div style:height={`${lines.total}px`} class="relative">
    {#each lines.drawn as { line: { node, depth }, start, size } (node.name)}
      {@const open = !!filter || expansion.isOpen(node.name)}
      <div
        role="treeitem"
        aria-selected={node.name === selected}
        aria-expanded={node.children.length ? open : undefined}
        tabindex="-1"
        class={[
          "absolute inset-x-0 top-0 flex items-center gap-1.5 pr-3 text-sm hover:bg-hover",
          node.locked && "opacity-50",
          node.name === selected && "font-semibold text-accent"
        ]}
        style:transform={`translateY(${start}px)`}
        style:height={`${size}px`}
        style:padding-left={`calc(${depth * 2}em + 0.75rem)`}
        onclick={() => onselect(node.name)}
        onkeydown={(e) => {
          // Enter on the folder's toggle is the toggle's
          if (e.key === "Enter" && !withModifier(e) && !keepsKey(e)) onselect(node.name);
        }}>
        <button
          type="button"
          class="shrink-0"
          aria-label={`${open ? "Collapse" : "Expand"} ${leaf(node.name)}`}
          title={node.locked
            ? LOCKED_HINT
            : "Alt-click to open or close the sibling folders as well"}
          onclick={(e) => {
            e.stopPropagation();
            expansion.toggle(node.name, siblings(node), withSiblings(e));
          }}>
          {#if node.locked}
            <LockIcon class="size-4 text-fg-muted" />
          {:else if !node.children.length}
            <FolderClosedIcon class="size-4 text-fg-muted" />
          {:else if open}
            <FolderOpenIcon class="size-4 text-fg-muted" />
          {:else}
            <FolderIcon class="size-4 text-fg-muted" />
          {/if}
        </button>
        <span class="name-count flex-1 gap-1.5">
          <span class="truncate">{leaf(node.name)}</span>
          <span class="text-right text-xs font-normal text-fg-muted"
            ><span
              >{caption(node.totalFileCount, node.totalDirectoryCount)}
              {#if node.totalBytes}· {formatBytes(node.totalBytes)}{/if}</span
            ></span>
        </span>
      </div>
    {/each}
  </div>
</div>
<ResizeHandle
  bind:value={height}
  min={row}
  edge="bottom"
  label="Height of the folder list"
  onchange={saveHeight}
  class="flex h-4 items-center justify-center border-t">
  <span class="h-1 w-8 rounded-full bg-border"></span>
</ResizeHandle>
