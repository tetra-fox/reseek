<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { SvelteSet } from "svelte/reactivity";

  import Button from "#lib/components/ui/Button.svelte";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";
  import type { Expansion } from "#lib/tree/expansion.svelte.js";
  import type { FolderNode, TreeLine } from "#lib/tree/folders.js";

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
     * An item line's cells, each a `gridcell`, its depth as `--depth` on the row for the first
     * cell's indent
     */
    item: Snippet<[T, number]>;
    /** the column headers over the item rows */
    header?: Snippet;
    /** lines drawn before a "Show all", so a folder of thousands doesn't stall the page */
    cap?: number;
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
    class: className
  }: Props = $props();

  let all = $state(false);
  let more = $state(0);
  const shown = $derived(all ? lines : lines.slice(0, cap + more));
  const hidden = $derived(lines.length - shown.length);

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

<div
  class={["text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent", className]}
  role="treegrid"
  aria-label={label}
  aria-multiselectable="true"
  aria-activedescendant={list.activeId}
  tabindex="0"
  {@attach list.attach}
  {@attach freezesWhileUsed}>
  {#if header}{@render header()}{/if}
  {#each shown as line, i (line.key)}
    {#if line.kind === "folder"}
      <div
        role="row"
        {...list.line(line, i)}
        class={[
          "group flex min-h-9 items-center gap-1.5 border-b pr-3",
          folderId && selected.has(folderId(line.node))
            ? "bg-accent-soft/60"
            : "bg-surface-muted/50",
          list.isFocused(line) && "ring-1 ring-accent ring-inset"
        ]}
        aria-level={line.depth + 1}
        aria-expanded={expansion.isOpen(line.node.path)}
        style:padding-left={`calc(${line.depth} * 1.25rem + 0.5rem)`}>
        <div role="gridcell" class="flex min-w-0 flex-1 items-center gap-1.5">
          <TreeToggle
            {expansion}
            key={line.node.path}
            siblings={line.siblings}
            name={line.node.label}
            {ontoggle} />
          {@render folder(line.node, line.depth)}
        </div>
      </div>
    {:else}
      <div
        role="row"
        {...list.line(line, i)}
        class={[
          "group grid min-h-9 items-center border-b hover:bg-hover",
          selected.has(idOf(line.item)) && "bg-accent-soft/60",
          list.isFocused(line) && "ring-1 ring-accent ring-inset"
        ]}
        aria-level={line.depth + 1}
        style:grid-template-columns={columns}
        style:--depth={line.depth}
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
