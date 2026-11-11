import {
  elementScroll,
  observeElementOffset,
  observeElementRect,
  type VirtualItem,
  Virtualizer,
  type VirtualizerOptions
} from "@tanstack/virtual-core";
import { untrack } from "svelte";

import { landing, markOf, Return, Saver } from "./scroll.svelte";

type Options = VirtualizerOptions<HTMLElement, HTMLElement>;

/** What a list tells its window: its lines, their sizes, and how far to draw past the view */
export type WindowSpec<L> = Pick<Options, "estimateSize"> &
  Partial<
    Pick<
      Options,
      | "overscan"
      | "scrollMargin"
      | "scrollPaddingStart"
      | "scrollPaddingEnd"
      | "anchorTo"
      | "followOnAppend"
      | "scrollEndThreshold"
      | "rangeExtractor"
      | "paddingEnd"
    >
  > & {
    /**
     * Changes whenever every line's size may have changed together, such as a new text size or
     * density, so the sizes guessed and measured so far are dropped
     */
    sizes?: unknown;
    /** called when the range of lines drawn changes, or a scroll starts or stops */
    onchange?: () => void;
    /**
     * The lines the view may be kept on. When lines arrive above, the first of these in view
     * stays where it was on screen; without it, what is in view moves down with them
     */
    anchor?: (line: L) => boolean;
    /**
     * The name the box is remembered under: where it is left is kept, and it starts there again,
     * on the same line even where lines came or went
     */
    place?: string;
  };

/** A line in the window: the list's line with where it is drawn */
export type Drawn<L> = { line: L; key: string; index: number; start: number; size: number };

/**
 * The window of a long list that is in the page, from TanStack Virtual. The scroll box carries
 * `{@attach window.attach}`; the list draws `window.drawn`, keyed by `key`, each line placed at
 * its `start` in a box `window.total` tall. Lines whose height depends on their content carry
 * `{@attach window.measure}` and `data-index`, and are measured as they are drawn; other lists
 * give every line its size in `estimateSize`. The window follows the box's scroll and size by
 * itself and only changes the drawn lines when the range of lines in view changes.
 *
 * A drawn line is found by its key, so while the lines change ahead of the window, each drawn
 * line is still its own and a line that left is not drawn
 */
export class ListWindow<L> {
  #virtualizer: Virtualizer<HTMLElement, HTMLElement>;
  #element = $state<HTMLElement | null>(null);
  /** bumped once for the changes the virtualizer reported, which the derived values below follow */
  #version = $state(0);
  /** bumped at the first change the virtualizer reports since the last was taken in */
  #reported = $state(0);
  /** a change was reported and not yet taken in */
  #pending = false;
  /** bumped whenever the options change, after which the virtualizer reads the DOM again */
  #updates = $state(0);
  /** the return to where the box was left, while one is under way */
  #back: Return | null = null;

  #lines: () => L[];
  #key: (line: L) => string;
  /** where each line is in the lines, by key */
  #at = $derived.by(() => new Map(this.#lines().map((line, i) => [this.#key(line), i])));
  /** the window's items, by index and key */
  #items: VirtualItem[] = $derived.by(() => {
    void this.#version;
    return this.#virtualizer.getVirtualItems();
  });

  /** the lines to draw now, in order */
  readonly drawn: Drawn<L>[] = $derived.by(() => {
    const lines = this.#lines();
    return this.#items.flatMap((item) => {
      const index = this.#at.get(item.key as string);
      return index === undefined
        ? []
        : [
            {
              line: lines[index],
              key: item.key as string,
              index,
              start: item.start,
              size: item.size
            }
          ];
    });
  });
  /** the height of every line together, which the box holding them takes */
  readonly total: number = $derived.by(() => {
    void this.#version;
    return this.#virtualizer.getTotalSize();
  });

  constructor(lines: () => L[], key: (line: L) => string, spec: () => WindowSpec<L>) {
    this.#lines = lines;
    this.#key = key;
    this.#virtualizer = new Virtualizer<HTMLElement, HTMLElement>({
      count: 0,
      estimateSize: () => 0,
      getScrollElement: () => this.#element,
      observeElementRect,
      observeElementOffset,
      scrollToFn: elementScroll,
      onChange: () => this.#report()
    });
    let sizes: unknown;
    let onchange: (() => void) | undefined;
    let shown: L[] = [];
    /** where the box scrolls to once the DOM holds the lines that keep the view still */
    let kept: number | null = null;
    let place: string | undefined;
    let saver: Saver | null = null;
    // before the DOM updates, so the lines drawn are those of the new count and keys
    $effect.pre(() => {
      const { sizes: next, onchange: changed, anchor, place: name, ...options } = spec();
      const all = lines();
      const element = this.#element;
      untrack(() => {
        const held =
          anchor && shown.length && all.length && key(all[0]) !== key(shown[0])
            ? this.#inView(shown, anchor)
            : null;
        this.#virtualizer.setOptions({
          ...this.#virtualizer.options,
          ...options,
          count: all.length,
          getItemKey: (index) => key(all[index]),
          getScrollElement: () => element
        });
        onchange = changed;
        shown = all;
        if (next !== sizes) {
          sizes = next;
          this.#virtualizer.measure();
        }
        if (name !== place) {
          saver?.flush();
          place = name;
          saver = name ? new Saver(name) : null;
          this.#back = name ? new Return(markOf(name)) : null;
        }
        // a box scrolled since the return last put it ends the return, before it puts it again
        if (element) this.#back?.holds(element);
        const mark = this.#back?.mark;
        const at = mark
          ? landing(
              mark,
              all.map((line) => key(line))
            )
          : held && { index: all.findIndex((line) => key(line) === held.key), delta: held.delta };
        if (at) {
          // reading the items makes the virtualizer measure the new lines
          this.#virtualizer.getVirtualItems();
          const start = this.#virtualizer.measurementsCache[at.index]?.start;
          if (start !== undefined) {
            kept = Math.max(0, start - at.delta);
            this.#virtualizer.scrollOffset = kept;
          }
        }
        this.#version++;
        this.#updates++;
      });
    });
    // once the DOM holds the new lines: the virtualizer follows the box, and a view kept on a
    // line is scrolled to it, which the box can only take now that it is tall enough
    $effect(() => {
      void this.#updates;
      void this.#element;
      untrack(() => {
        const element = this.#element;
        // asked before the virtualizer moves the box, which may already take it to `kept`
        const back = this.#back?.mark ? this.#back : null;
        const going = !!element && !!back && back.holds(element);
        this.#virtualizer._willUpdate();
        // a box not attached yet takes it once it is
        if (kept === null || !element) return;
        if (back && going) back.put(element, kept);
        else if (!back) element.scrollTop = kept;
        kept = null;
      });
    });
    // every line drawn in one update is measured as it is attached, each a change; taken in at
    // once, the lines are laid out again once rather than once per line
    $effect(() => {
      void this.#reported;
      untrack(() => {
        if (!this.#pending) return;
        this.#pending = false;
        this.#version++;
        onchange?.();
        if (saver && !this.#back?.mark) {
          const top = this.#virtualizer.scrollOffset ?? 0;
          const at = this.#inView(shown, () => true);
          const index = at ? (this.#at.get(at.key) ?? 0) : 0;
          // a list left at its top is forgotten, so it opens at the top over rows that came since
          saver.note(
            top === 0 ? null : { key: at?.key ?? null, index, delta: at?.delta ?? 0, top }
          );
        }
      });
    });
    $effect(() => this.#virtualizer._didMount());
    $effect(() => () => saver?.flush());
  }

  /** Notes a change the virtualizer reported, for the effect above to take in */
  #report() {
    if (this.#pending) return;
    this.#pending = true;
    this.#reported++;
  }

  /** The first line in view that `anchor` accepts, by key, and how far below the view's top it starts */
  #inView(lines: L[], anchor: (line: L) => boolean) {
    const top = this.#virtualizer.scrollOffset ?? 0;
    for (const item of this.#virtualizer.getVirtualItems()) {
      const line = lines[item.index];
      if (item.end > top && line !== undefined && anchor(line)) {
        return { key: this.#key(line), delta: item.start - top };
      }
    }
    return null;
  }

  /** For `{@attach}` on the scroll box */
  attach = (node: HTMLElement) => {
    this.#element = node;
    // a scroll the return did not make ends it
    const scrolled = () => this.#back?.holds(node);
    node.addEventListener("scroll", scrolled, { passive: true });
    return () => {
      node.removeEventListener("scroll", scrolled);
      if (this.#element === node) this.#element = null;
    };
  };

  /** For `{@attach}` on a line whose size is read from the page; the line carries `data-index` */
  measure = (node: HTMLElement) => {
    this.#virtualizer.measureElement(node);
  };

  /** Whether the line at `index` is drawn now */
  isDrawn(index: number) {
    const items = this.#items;
    return items.length > 0 && index >= items[0].index && index <= items.at(-1)!.index;
  }

  /** Where the line at `index` starts in the box, from the sizes known so far */
  startOf(index: number): number | undefined {
    void this.#version;
    return this.#virtualizer.measurementsCache[index]?.start;
  }

  /** The box's scroll offset as the window last read it */
  get offset(): number {
    void this.#version;
    return this.#virtualizer.scrollOffset ?? 0;
  }

  /** Brings the line at `index` into view: to the nearest edge, or as `align` says */
  scrollTo(index: number, align: "auto" | "start" | "center" | "end" = "auto") {
    this.#virtualizer.scrollToIndex(index, { align });
  }

  /** Scrolls the box to `offset`, in pixels from its top */
  scrollToOffset(offset: number) {
    this.#virtualizer.scrollToOffset(offset);
  }

  /** Scrolls to the end of the list */
  scrollToEnd() {
    this.#virtualizer.scrollToEnd();
  }

  /** Whether the view is within `threshold` pixels of the end */
  atEnd(threshold?: number) {
    return this.#virtualizer.isAtEnd(threshold);
  }
}
