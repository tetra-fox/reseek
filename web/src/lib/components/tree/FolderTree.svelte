<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import Button from "#lib/components/ui/Button.svelte";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import type { FolderNode, TreeLine } from "#lib/tree/folders.js";
  import { FOLDER_LINE_REM, GAP_REM, lineStart, TOGGLE_REM } from "#lib/tree/indent.js";
  import { viewport } from "#lib/viewport.svelte.js";

  import TreeToggle from "./TreeToggle.svelte";

  type Props = {
    /** the tree laid out, `treeLines` of what the screen shows */
    lines: TreeLine<T>[];
    expansion: Expansion;
    /** names the grid for assistive technology */
    label: string;
    /** the item rows' columns, as a grid template */
    columns: string;
    /** the selected items' ids */
    selected: SvelteSet<string>;
    /** an item's id in the selection */
    idOf: (item: T) => string;
    /** Enter on an item */
    onopen?: (item: T) => void;
    /**
     * A folder's id in the selection, for a tree whose folders are rows too, picked and walked
     * with the keys like items; without it a folder line only heads its items
     */
    folderId?: (node: FolderNode<T>) => string;
    /** Enter on a folder that is a row */
    onopenfolder?: (node: FolderNode<T>) => void;
    /** a key of the screen's own on a row, such as Delete; true when it acted */
    onkey?: (e: KeyboardEvent, line: TreeLine<T>) => boolean;
    /** a middle click on an item, which acts without leaving the page, as a new tab would */
    onmiddleclick?: (item: T) => void;
    /** a folder opened or closed, for a tree that reads a folder when it opens */
    ontoggle?: (open: boolean, paths: string[]) => void;
    /** a folder line after its toggle: its name, counts, actions */
    folder: Snippet<[FolderNode<T>, number]>;
    /**
     * An item line's cells, each a `gridcell`, set in so the first starts where the content after
     * a folder's toggle does at the same depth
     */
    item: Snippet<[T, number]>;
    /** the column headers over the item rows, each a `columnheader`, set in as a top item is */
    header?: Snippet;
    /** lines drawn before a "Show all", so a folder of thousands doesn't stall the page */
    cap?: number;
    /**
     * Where folder lines stop as the tree scrolls under what is pinned above it, a CSS length.
     * Each folder's line then stays under the line of the folder it is in while its own lines
     * scroll, as nested section headers do; without it folder lines scroll away with the rest
     */
    pin?: string;
    class?: string;
  };

  let {
    lines,
    expansion,
    label,
    columns,
    selected,
    idOf,
    onopen,
    folderId,
    onopenfolder,
    onkey,
    onmiddleclick,
    ontoggle,
    folder,
    item,
    header,
    cap = 500,
    pin,
    class: className
  }: Props = $props();

  let all = $state(false);
  let more = $state(0);
  const shown = $derived(all ? lines : lines.slice(0, cap + more));
  const hidden = $derived(lines.length - shown.length);

  /** A line with the lines inside it when it is a folder, and where it is among those shown */
  type Block = { line: TreeLine<T>; index: number; inside: Block[] };

  /**
   * The lines shown, nested: each folder's lines are drawn in a box of their own, which a pinned
   * folder line stays within, so it scrolls away once the last of them has
   */
  const blocks = $derived.by(() => {
    const top: Block[] = [];
    const open: Block[] = [];
    shown.forEach((line, index) => {
      while (open.length && open.at(-1)!.line.depth >= line.depth) open.pop();
      const block: Block = { line, index, inside: [] };
      (open.at(-1)?.inside ?? top).push(block);
      if (line.kind === "folder") open.push(block);
    });
    return top;
  });

  /**
   * The slot a pinned folder line at `depth` takes, counted in lines under `pin`: a line per level,
   * so a folder's chain stacks in order. A phone has one, which the innermost folder takes
   */
  const slot = (depth: number) => (viewport.phone ? 0 : depth);

  /** `levels` folder lines under `pin` */
  const below = (levels: number) => `calc(${pin} + ${levels * FOLDER_LINE_REM}rem)`;

  /**
   * How far under `pin` the folder lines over a line at `depth` reach, once it is scrolled up to
   * them: where keyboard focus brings a line into view
   */
  const covered = (depth: number) => below(depth === 0 ? 0 : slot(depth - 1) + 1);

  /**
   * A pinned folder line's place in the painting order: under the folder lines of the folders it
   * is in, so its end pushes it up under them, but over them on a phone, where it covers them
   */
  const layer = (depth: number) => (viewport.phone ? 1 + depth : Math.max(1, 100 - depth));

  /**
   * How far a line at `depth` is set in. An item also passes the toggle and the gap after it that
   * start a folder line, so an item's cells line up under the content of a folder line at its depth
   */
  const inset = (depth: number, kind: "folder" | "item") =>
    `${lineStart(depth) + (kind === "item" ? TOGGLE_REM + GAP_REM : 0)}rem`;

  /** the ids of every item in a folder and the folders below it */
  const under = (node: FolderNode<T>): string[] => [
    ...node.items.map(idOf),
    ...node.children.flatMap(under)
  ];

  /** Opens or closes a folder from the keys, as its toggle would */
  const show = (node: FolderNode<T>, open: boolean) => {
    if (expansion.isOpen(node.path) === open) return;
    expansion.set(node.path, open);
    ontoggle?.(open, [node.path]);
  };

  const list = new ListControl<TreeLine<T>>({
    lines: () => shown,
    key: (line) => line.key,
    id: (line) => (line.kind === "item" ? idOf(line.item) : (folderId?.(line.node) ?? null)),
    group: (line) => under(line.kind === "folder" ? line.node : line.folder),
    onopen: (line) => {
      if (line.kind === "item") onopen?.(line.item);
      else onopenfolder?.(line.node);
    },
    // Right opens a folder and Left closes it, as in a file manager's tree
    onkey: (e, line) => {
      if (line.kind === "folder" && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
        show(line.node, e.key === "ArrowRight");
        return true;
      }
      return !!onkey?.(e, line);
    },
    // the screen's selection, made once for the tree's life
    // svelte-ignore state_referenced_locally
    selected
  });

  /** Gives the grid DOM focus, without scrolling */
  export const focus = list.focus;
</script>

<!-- a folder's line, then its lines in a box of their own, which its pinned line stays within -->
{#snippet blocksOf(level: Block[])}
  {#each level as { line, index, inside } (line.key)}
    {#if line.kind === "folder"}
      <div>
        <div
          role="row"
          {...list.line(line, index)}
          class={[
            "group flex items-center border-b pr-3",
            // opaque tints, so the lines scrolling under a pinned folder line don't show through
            folderId && selected.has(folderId(line.node))
              ? "bg-[color-mix(in_srgb,var(--color-accent-soft)_60%,var(--color-surface))]"
              : "bg-[color-mix(in_srgb,var(--color-surface-muted)_50%,var(--color-surface))]",
            pin && "sticky",
            list.isFocused(line) && "ring-1 ring-accent ring-inset"
          ]}
          aria-level={line.depth + 1}
          aria-expanded={expansion.isOpen(line.node.path)}
          style:height={`${FOLDER_LINE_REM}rem`}
          style:top={pin ? below(slot(line.depth)) : undefined}
          style:z-index={pin ? layer(line.depth) : undefined}
          style:scroll-margin-top={pin ? below(slot(line.depth)) : undefined}
          style:padding-left={inset(line.depth, "folder")}>
          <div
            role="gridcell"
            class="flex min-w-0 flex-1 items-center"
            style:column-gap={`${GAP_REM}rem`}>
            <TreeToggle
              {expansion}
              key={line.node.path}
              siblings={line.siblings}
              name={line.node.label}
              {ontoggle} />
            {@render folder(line.node, line.depth)}
          </div>
        </div>
        {@render blocksOf(inside)}
      </div>
    {:else}
      <div
        role="row"
        {...list.line(line, index)}
        class={[
          "group grid min-h-9 items-center border-b hover:bg-hover",
          selected.has(idOf(line.item)) && "bg-accent-soft/60",
          list.isFocused(line) && "ring-1 ring-accent ring-inset"
        ]}
        aria-level={line.depth + 1}
        style:grid-template-columns={columns}
        style:scroll-margin-top={pin ? covered(line.depth) : undefined}
        style:padding-left={inset(line.depth, "item")}
        onmousedown={(e) => {
          // the middle button scrolls the page by dragging, unless kept from it
          if (e.button === 1 && onmiddleclick) e.preventDefault();
        }}
        onauxclick={(e) => {
          if (e.button === 1 && onmiddleclick) onmiddleclick(line.item);
        }}>
        {@render item(line.item, line.depth)}
      </div>
    {/if}
  {/each}
{/snippet}

<!-- a tree that pins its folder lines keeps their layers to itself, under what is pinned above -->
<div
  class={[
    "text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent",
    pin && "isolate",
    className
  ]}
  role="treegrid"
  aria-label={label}
  aria-multiselectable="true"
  aria-activedescendant={list.activeId}
  tabindex="0"
  {@attach list.attach}
  {@attach freezesWhileUsed}>
  {#if header}
    <div
      role="row"
      class="grid min-w-max items-center border-b bg-surface-muted py-1.5 text-xs font-medium text-fg-muted"
      style:grid-template-columns={columns}
      style:padding-left={inset(0, "item")}>
      {@render header()}
    </div>
  {/if}
  {@render blocksOf(blocks)}
  {#if hidden > 0}
    <p class="flex flex-wrap items-center gap-2 px-4 py-2 text-sm text-fg-muted">
      Showing {shown.length.toLocaleString()} of {lines.length.toLocaleString()} rows.
      <Button variant="link" onclick={() => (more += cap)}>
        Show {Math.min(cap, hidden).toLocaleString()} more
      </Button>
      <Button variant="link" onclick={() => (all = true)}>Show all</Button>
    </p>
  {/if}
</div>
