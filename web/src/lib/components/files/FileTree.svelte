<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import { tick } from "svelte";
  import type { ClassValue } from "svelte/elements";
  import { SvelteSet } from "svelte/reactivity";

  import { Arrivals } from "#lib/lists/arrivals.svelte.js";
  import { freezesWhileUsed } from "#lib/lists/interaction.svelte.js";
  import { ListControl } from "#lib/lists/list.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { type Expansion, withSiblings } from "#lib/tree/expansion.svelte.js";
  import { type FolderNode, itemsUnder, type TreeLine } from "#lib/tree/folders.js";
  import { GAP_REM, lineStart, TOGGLE_REM } from "#lib/tree/indent.js";
  import { nesting, pins } from "#lib/tree/pins.js";
  import { inControl } from "#lib/ui/keys.js";
  import { rem, viewport } from "#lib/ui/viewport.svelte.js";

  import TreeToggle from "./TreeToggle.svelte";

  type Props = {
    /** the tree laid out, `treeLines` of what the screen shows */
    lines: TreeLine<T>[];
    expansion: Expansion;
    /** names the tree for assistive technology */
    label: string;
    /** the item rows' columns, as a grid template */
    columns: string;
    /** the selected items' ids */
    selected: SvelteSet<string>;
    /** an item's id in the selection */
    idOf: (item: T) => string;
    /** whether an item can be ticked; every item by default */
    tickable?: (item: T) => boolean;
    /** a line's name, which typing the start of finds; a folder's label by default */
    nameOf?: (line: TreeLine<T>) => string;
    /** Enter on an item */
    onopen?: (item: T) => void;
    /**
     * A folder's id in the selection, for a tree whose folders are picked like items; without it
     * a folder line is focused and walked with the keys but ticks the items under it
     */
    folderId?: (node: FolderNode<T>) => string;
    /** Enter on a folder; it opens or closes without this */
    onopenfolder?: (node: FolderNode<T>) => void;
    /** a key of the screen's own on a line, such as Delete; true when it acted */
    onkey?: (e: KeyboardEvent, line: TreeLine<T>) => boolean;
    /** a middle click on an item, which acts without leaving the page, as a new tab would */
    onmiddleclick?: (item: T) => void;
    /** folders opened or closed, by their keys, for a tree that reads a folder when it opens */
    ontoggle?: (open: boolean, keys: string[]) => void;
    /** a folder line after its toggle: its name, counts, actions */
    folder: Snippet<[FolderNode<T>, number]>;
    /**
     * An item line's cells, each a `gridcell`, set in so the first starts where the content after
     * a folder's toggle does at the same depth. The last argument ticks the item as its checkbox
     * does, with Shift for a range
     */
    item: Snippet<[T, number, (range: boolean) => void]>;
    /** the column headers over the item rows, each a `columnheader`, set in as a top item is */
    header?: Snippet;
    /** a line's height in rem, on a wider screen and on a phone; every line has the same */
    lineRem?: { wide: number; phone: number };
    /**
     * The pinned folder lines' slot for a depth, counted in lines from the top: a line per level
     * by default, so a folder's chain stacks in order, and on a phone one, the innermost folder's
     */
    slot?: (depth: number) => number;
    /** the scroll box's height, a CSS length */
    height?: string;
    /**
     * The item columns' least width, a CSS length; with the deepest line's inset added, the lines'
     * least width, past which the box scrolls sideways
     */
    width?: string;
    /** classes of a line of the screen's own, such as a tint for a failed transfer */
    lineClass?: (line: TreeLine<T>) => ClassValue;
    /**
     * Whether the tree draws each folder's toggle and sets each line in by its depth, and puts the
     * column headers in a row of its own; a screen whose snippets do all three passes false
     */
    framed?: boolean;
    /** a layer of the screen's own is open, such as a drawer, which Escape closes first */
    layer?: () => boolean;
    class?: ClassValue;
  };

  let {
    lines,
    expansion,
    label,
    columns,
    selected,
    idOf,
    tickable = () => true,
    nameOf = (line) => (line.kind === "folder" ? line.node.label : ""),
    onopen,
    folderId,
    onopenfolder,
    onkey,
    onmiddleclick,
    ontoggle,
    folder,
    item,
    header,
    lineRem = { wide: 2.25, phone: 3 },
    slot = (depth) => (viewport.phone ? 0 : depth),
    height = "max(20rem, calc(var(--content-h) - 14rem))",
    width,
    lineClass,
    framed = true,
    layer,
    class: className
  }: Props = $props();

  /** one line's height in pixels */
  const row = $derived(rem.px * lineRem[viewport.phone ? "phone" : "wide"]);
  /** how far the lines have scrolled, which decides the folder lines pinned at the top */
  let scrollTop = $state(0);
  /** where the lines start in the scrolled content, under the column header */
  let rowsTop = $state(0);
  let boxHeight = $state(0);

  const nest = $derived(
    nesting(
      lines,
      (line) => line.depth,
      (line) => line.kind === "folder"
    )
  );
  // the lines of the folders the top lines are in stay at the top while their lines scroll
  const pinned = $derived(
    pins(nest, scrollTop, row, slot).flatMap((pin) => {
      const line = lines[pin.index];
      return line.kind === "folder" ? [{ ...pin, line }] : [];
    })
  );

  // lines a folder showed as it opened, or new items, make an entrance
  const arrivals = new Arrivals();
  $effect(() => arrivals.see(lines.map((line) => line.key)));

  /**
   * How far a line at `depth` is set in. An item also passes the toggle and the gap after it that
   * start a folder line, so an item's cells line up under the content of a folder line at its depth
   */
  const inset = (depth: number, kind: "folder" | "item") =>
    framed ? `${lineStart(depth) + (kind === "item" ? TOGGLE_REM + GAP_REM : 0)}rem` : undefined;

  /** Opens or closes a folder from the keys, as its toggle would */
  const show = (node: FolderNode<T>, open: boolean) => {
    if (expansion.isOpen(node.key) === open) return;
    expansion.set(node.key, open);
    ontoggle?.(open, [node.key]);
  };

  const list = new ListControl<TreeLine<T>>({
    lines: () => lines,
    key: (line) => line.key,
    id: (line) => (line.kind === "item" ? idOf(line.item) : (folderId?.(line.node) ?? null)),
    // the items under the line's folder; a line with none there, as a flat list's, selects all
    group: (line) => {
      const ids = itemsUnder(line.kind === "folder" ? line.node : line.folder)
        .filter(tickable)
        .map(idOf);
      return ids.length ? ids : null;
    },
    onopen: (line) => {
      if (line.kind === "item") onopen?.(line.item);
      else if (onopenfolder) onopenfolder(line.node);
      else show(line.node, !expansion.isOpen(line.node.key));
    },
    // Right opens a folder and Left closes it, as in a file manager's tree
    onkey: (e, line) => {
      if (line.kind === "folder" && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
        show(line.node, e.key === "ArrowRight");
        return true;
      }
      return !!onkey?.(e, line);
    },
    headersFocus: true,
    layer: () => !!layer?.(),
    typeahead: (line) => nameOf(line),
    page: () => Math.max(1, Math.floor((boxHeight - rowsTop) / row) - 1),
    // the pinned copy of a folder line is rendered only while it is pinned
    rendered: (index, element) =>
      element?.dataset.pinned ? pinned.some((pin) => pin.index === index) : view.isDrawn(index),
    reveal: (index) => reveal(index),
    // the screen's selection, made once for the tree's life
    // svelte-ignore state_referenced_locally
    selected
  });

  const view: ListWindow<TreeLine<T>> = new ListWindow(
    () => lines,
    (line) => line.key,
    () => ({
      estimateSize: () => row,
      overscan: 10,
      scrollMargin: rowsTop,
      scrollPaddingStart: rowsTop,
      sizes: row,
      // a control in a line that scrolled out of the window gives focus back to the tree
      onchange: () => list.scrolled()
    })
  );

  /**
   * Brings the line at `index` into view, stopping a line brought down under the column header
   * and the folder lines that will be pinned over it there: one per level it is in
   */
  const reveal = (index: number) => {
    const start = view.startOf(index);
    if (start === undefined) return view.scrollTo(index);
    const depth = lines[index].depth;
    const covered = depth > 0 ? (slot(depth - 1) + 1) * row : 0;
    const highest = start - rowsTop - covered;
    const lowest = start + row - boxHeight;
    if (view.offset > highest) view.scrollToOffset(highest);
    else if (view.offset < lowest) view.scrollToOffset(lowest);
  };

  /**
   * A click on a folder line, outside its controls, opens or closes it as its toggle would, unless
   * it ends a drag across text; in a tree whose folders are picked like items it picks it instead
   */
  const toggleOnClick = (e: MouseEvent, line: TreeLine<T> & { kind: "folder" }) => {
    if (folderId || e.defaultPrevented || inControl(e) || e.ctrlKey || e.metaKey || e.shiftKey) {
      return;
    }
    if (!document.getSelection()?.isCollapsed) return;
    const key = line.node.key;
    const all = withSiblings(e);
    const open = !expansion.isOpen(key);
    expansion.toggle(key, line.siblings, all);
    ontoggle?.(open, all ? line.siblings : [key]);
  };

  /** Gives the tree DOM focus, without scrolling */
  export const focus = list.focus;

  /** Whether `element` is inside the tree */
  export const has = list.has;

  /** The line with focus, or none */
  export const focused = () => list.focused;

  /** The lines drawn now */
  export const drawn = () => view.drawn.map((d) => d.line);

  /**
   * Focuses the line with `key` once the lines hold it, its folders opened by the caller, and
   * scrolls it to the middle of the box
   */
  export const navigate = async (key: string) => {
    await tick();
    const index = lines.findIndex((line) => line.key === key);
    if (index === -1) return;
    list.focusedKey = key;
    view.scrollTo(index, "center");
  };

  /** the lines' least width: the columns' set in as far as the deepest item */
  const least = $derived(
    width && framed
      ? `calc(${width} + ${inset(lines.length ? nest.deepest + 1 : 0, "item")})`
      : width
  );

  const pinnedId = (index: number) => `${list.uid}-pinned-${index}`;
  const focusedPin = $derived(pinned.find((pin) => pin.line.key === list.focusedKey));
  const activeId = $derived(focusedPin ? pinnedId(focusedPin.index) : list.activeId);
  /** opaque tints, so the lines scrolling under a pinned folder line don't show through */
  const folderTint = (node: FolderNode<T>) =>
    folderId && selected.has(folderId(node))
      ? "bg-[color-mix(in_srgb,var(--color-accent-soft)_60%,var(--color-surface))]"
      : "bg-[color-mix(in_srgb,var(--color-surface-muted)_50%,var(--color-surface))]";
</script>

{#snippet folderLine(line: TreeLine<T> & { kind: "folder" })}
  {#if framed}
    <div
      role="gridcell"
      class="flex min-w-0 flex-1 items-center"
      style:column-gap={`${GAP_REM}rem`}>
      <TreeToggle
        {expansion}
        key={line.node.key}
        siblings={line.siblings}
        name={line.node.label}
        {ontoggle} />
      {@render folder(line.node, line.depth)}
    </div>
  {:else}
    <div class="h-full min-w-0 flex-1">{@render folder(line.node, line.depth)}</div>
  {/if}
{/snippet}

<!-- every line has the same height, so the window never measures one and the pinned folder lines
     stack by it -->
<div
  class={[
    "relative overflow-auto rounded-card border bg-surface text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent",
    className
  ]}
  style:height
  style:--line-h={`${row}px`}
  role="treegrid"
  aria-label={label}
  aria-rowcount={lines.length}
  aria-multiselectable="true"
  aria-activedescendant={activeId}
  tabindex="0"
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  bind:clientHeight={boxHeight}
  {@attach view.attach}
  {@attach list.attach}>
  <!-- the pinned folder lines are drawn under the column header, which a pushed one goes under,
       and over the lines -->
  <div class="sticky top-0 z-10" style:min-width={least} bind:offsetHeight={rowsTop}>
    {#if header && !framed}
      {@render header()}
    {:else if header}
      <div
        role="row"
        class="grid items-center border-b bg-surface-muted py-1.5 text-xs font-medium text-fg-muted"
        style:grid-template-columns={columns}
        style:padding-left={inset(0, "item")}>
        {@render header()}
      </div>
    {/if}
    {#each pinned as pin, n (pin.line.key)}
      <div
        role="row"
        class={[
          "group absolute inset-x-0 flex items-center list-row",
          framed && "pr-3",
          folderTint(pin.line.node)
        ]}
        id={pinnedId(pin.index)}
        style:top={`calc(100% + ${pin.top}px)`}
        style:height={`${row}px`}
        style:z-index={n - pinned.length}
        style:padding-left={inset(pin.line.depth, "folder")}
        aria-level={pin.line.depth + 1}
        aria-expanded={expansion.isOpen(pin.line.node.key)}
        data-line={pin.line.key}
        data-focused={list.isFocused(pin.line) || undefined}
        data-pinned="true">
        {@render folderLine(pin.line)}
      </div>
    {/each}
  </div>
  <div
    class="relative"
    style:height={`${view.total}px`}
    style:min-width={least}
    role="rowgroup"
    {@attach freezesWhileUsed}>
    {#each view.drawn as { line, index, start } (line.key)}
      {#if line.kind === "folder"}
        <div
          role="row"
          {...list.line(line, index)}
          class={[
            "group absolute inset-x-0 top-0 flex items-center list-row",
            framed && "pr-3",
            !folderId && "cursor-pointer",
            folderTint(line.node),
            arrivals.isFresh(line.key) && "animate-line-in",
            lineClass?.(line)
          ]}
          style:transform={`translateY(${start - rowsTop}px)`}
          style:height={`${row}px`}
          style:padding-left={inset(line.depth, "folder")}
          aria-level={line.depth + 1}
          aria-expanded={expansion.isOpen(line.node.key)}
          onclick={(e) => toggleOnClick(e, line)}>
          {@render folderLine(line)}
        </div>
      {:else}
        <div
          role="row"
          {...list.line(line, index)}
          class={[
            "group absolute inset-x-0 top-0 isolate grid items-center list-row hover:bg-hover",
            selected.has(idOf(line.item)) && "bg-accent-soft/60",
            arrivals.isFresh(line.key) && "animate-line-in",
            lineClass?.(line)
          ]}
          style:transform={`translateY(${start - rowsTop}px)`}
          style:height={`${row}px`}
          style:grid-template-columns={columns}
          style:padding-left={inset(line.depth, "item")}
          aria-level={line.depth + 1}
          onmousedown={(e) => {
            // the middle button scrolls the page by dragging, unless kept from it
            if (e.button === 1 && onmiddleclick) e.preventDefault();
          }}
          onauxclick={(e) => {
            if (e.button === 1 && onmiddleclick) onmiddleclick(line.item);
          }}>
          {@render item(line.item, line.depth, (range) => list.tick(idOf(line.item), range))}
        </div>
      {/if}
    {/each}
  </div>
</div>
