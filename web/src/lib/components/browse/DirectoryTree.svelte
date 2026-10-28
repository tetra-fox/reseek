<script lang="ts" module>
  /** one row's height; the list draws only the rows in view and a margin around them */
  const ROW = 33;
  const OVERSCAN = 15;
  const MIN_HEIGHT = ROW;
  const HEIGHT_KEY = "reseek-browse-tree-height";

  /** The tree's height saved in this browser, 300 px when none or nothing usable is */
  const savedHeight = () => {
    try {
      const height = Number.parseInt(localStorage.getItem(HEIGHT_KEY) ?? "", 10);
      return Number.isFinite(height) && height >= MIN_HEIGHT ? height : 300;
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
  import { formatBytes } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { keepsKey, withModifier } from "#lib/keys.js";
  import { Expansion, withSiblings } from "#lib/tree/expansion.svelte.js";

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
  let scrollTop = $state(0);
  let height = $state(savedHeight());

  // a new listing starts with its first folder open, a hint that folders open
  $effect(() => {
    const first = tree.roots[0];
    untrack(() => {
      expansion.reset(false);
      if (first?.children.length) expansion.set(first.name, true);
      scrollTop = 0;
      if (scroller) scroller.scrollTop = 0;
    });
  });

  const rows = $derived(flatten(tree.roots, expansion.isOpen, filter?.folder));
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));

  const caption = (files: number, folders: number) =>
    [
      files ? `${files} file${files === 1 ? "" : "s"}` : "",
      folders ? `${folders} director${folders === 1 ? "y" : "ies"}` : ""
    ]
      .filter(Boolean)
      .join(", ");

  /** The folders beside a folder, itself among them, which Alt-click opens or closes with it */
  const siblings = (node: TreeNode) =>
    (tree.byName.get(parentOf(node.name, separator))?.children ?? tree.roots).map((n) => n.name);

  export const expandAll = () => expansion.reset(true);
  export const collapseAll = () => expansion.reset(false);

  /** Scrolls the row at `index` to the middle of the list */
  const centre = (index: number) => {
    if (scroller && index !== -1) {
      scroller.scrollTop = Math.max(0, index * ROW - height / 2 + ROW / 2);
    }
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
    centre(at);
  };

  /** Opens every folder above `name`, selects it and scrolls it to the middle */
  export const navigate = async (name: string) => {
    for (const above of ancestors(name, tree, separator)) expansion.set(above, true);
    onselect(name);
    await tick();
    centre(rows.findIndex((r) => r.node.name === name));
  };
</script>

<div
  bind:this={scroller}
  class="relative overflow-y-auto outline-none [overflow-anchor:auto] focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
  style:height={`${height}px`}
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  role="tree"
  aria-label="Shared folders"
  tabindex="0"
  title="Type a folder's first letters to go to it"
  onkeydown={typeToFind}
  {@attach freezesWhileUsed}>
  <div style:height={`${rows.length * ROW}px`} class="relative">
    {#each rows.slice(start, end) as { node, depth }, i (node.name)}
      {@const open = !!filter || expansion.isOpen(node.name)}
      <div
        role="treeitem"
        aria-selected={node.name === selected}
        aria-expanded={node.children.length ? open : undefined}
        tabindex="-1"
        class={[
          "absolute inset-x-0 flex items-center gap-1.5 pr-3 text-sm hover:bg-hover",
          node.locked && "opacity-50",
          node.name === selected && "font-semibold text-accent"
        ]}
        style:top={`${(start + i) * ROW}px`}
        style:height={`${ROW}px`}
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
          title={node.locked ? LOCKED_HINT : "Alt-click to open or close the folders beside it too"}
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
        <span class="truncate">{leaf(node.name)}</span>
        <span class="ml-auto shrink-0 text-xs font-normal text-fg-muted">
          {caption(node.totalFileCount, node.totalDirectoryCount)}
          {#if node.totalBytes}· {formatBytes(node.totalBytes)}{/if}
        </span>
      </div>
    {/each}
  </div>
</div>
<ResizeHandle
  bind:value={height}
  min={MIN_HEIGHT}
  edge="bottom"
  label="Height of the folder list"
  onchange={saveHeight}
  class="flex h-4 items-center justify-center border-t">
  <span class="h-1 w-8 rounded-full bg-border"></span>
</ResizeHandle>
