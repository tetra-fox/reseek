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
  import { SvelteSet } from "svelte/reactivity";

  import type { BrowseFilter } from "#lib/browse/filter.js";
  import { ancestors, flatten, leaf, type Separator, type Tree } from "#lib/browse/tree.js";
  import { formatBytes } from "#lib/format.js";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { withModifier } from "#lib/keys.js";

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

  const expanded = new SvelteSet<string>();
  let scroller = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let height = $state(savedHeight());

  // a new listing starts with its first folder open, a hint that folders open
  $effect(() => {
    const first = tree.roots[0];
    untrack(() => {
      expanded.clear();
      if (first?.children.length) expanded.add(first.name);
      scrollTop = 0;
      if (scroller) scroller.scrollTop = 0;
    });
  });

  const rows = $derived(flatten(tree.roots, expanded, filter?.folder));
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));

  const caption = (files: number, folders: number) =>
    [
      files ? `${files} file${files === 1 ? "" : "s"}` : "",
      folders ? `${folders} director${folders === 1 ? "y" : "ies"}` : ""
    ]
      .filter(Boolean)
      .join(", ");

  const toggle = (name: string) => {
    if (expanded.has(name)) expanded.delete(name);
    else expanded.add(name);
  };

  export const expandAll = () => {
    for (const name of tree.byName.keys()) expanded.add(name);
  };
  export const collapseAll = () => expanded.clear();

  /** Opens every folder above `name`, selects it and scrolls it to the middle */
  export const navigate = async (name: string) => {
    for (const above of ancestors(name, tree, separator)) expanded.add(above);
    onselect(name);
    await tick();
    const index = rows.findIndex((r) => r.node.name === name);
    if (scroller && index !== -1) {
      scroller.scrollTop = Math.max(0, index * ROW - height / 2 + ROW / 2);
    }
  };

  let dragging: { y: number; height: number } | null = null;
</script>

<div
  bind:this={scroller}
  class="relative overflow-y-auto [overflow-anchor:auto]"
  style:height={`${height}px`}
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  role="tree"
  aria-label="Shared folders"
  {@attach freezesWhileUsed}>
  <div style:height={`${rows.length * ROW}px`} class="relative">
    {#each rows.slice(start, end) as { node, depth }, i (node.name)}
      {@const open = !!filter || expanded.has(node.name)}
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
        onkeydown={(e) => e.key === "Enter" && !withModifier(e) && onselect(node.name)}>
        <button
          type="button"
          class="shrink-0"
          aria-label={open ? "Collapse" : "Expand"}
          title={node.locked ? LOCKED_HINT : undefined}
          onclick={(e) => {
            e.stopPropagation();
            toggle(node.name);
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
<div
  role="slider"
  tabindex="0"
  aria-label="Height of the folder list"
  aria-valuemin={MIN_HEIGHT}
  aria-valuenow={height}
  title="Drag or use arrow keys to resize"
  class="flex h-4 cursor-ns-resize items-center justify-center border-t"
  onpointerdown={(e) => {
    dragging = { y: e.clientY, height: height };
    e.currentTarget.setPointerCapture(e.pointerId);
  }}
  onpointermove={(e) => {
    if (dragging) height = Math.max(MIN_HEIGHT, dragging.height + e.clientY - dragging.y);
  }}
  onpointerup={() => {
    if (dragging) saveHeight(height);
    dragging = null;
  }}
  onkeydown={(e) => {
    if (withModifier(e)) return;
    if (e.key === "ArrowUp") height = Math.max(MIN_HEIGHT, height - 20);
    else if (e.key === "ArrowDown") height += 20;
    else return;
    e.preventDefault();
    saveHeight(height);
  }}>
  <span class="h-1 w-8 rounded-full bg-border"></span>
</div>
