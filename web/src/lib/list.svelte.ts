import { on } from "svelte/events";
import { SvelteSet } from "svelte/reactivity";

import { inControl, keepsKey, layerOpen, withModifier } from "./keys";

/** What a list tells the controller about its lines */
export type ListSpec<L> = {
  /** the lines in the order they are shown, rows and group headers */
  lines: () => L[];
  /** a line's stable key, which its element carries as `data-line` */
  key: (line: L) => string;
  /** the id a row stands for in the selection; null for a group header, which is not a row */
  id: (line: L) => string | null;
  /**
   * The ids Ctrl/Cmd+A selects while `line` has focus, and Space toggles on a header: those of
   * the group it belongs to or heads. Null, or no function, for every row of the list
   */
  group?: (line: L) => string[] | null;
  /** Enter on a row */
  onopen?: (line: L) => void;
  /** a plain click on a row opens it too, for lists whose rows are what a pane shows */
  openOnClick?: boolean;
  /** a key of the list's own for the focused row, such as Delete; true when it acted */
  onkey?: (e: KeyboardEvent, line: L) => boolean;
  /** a layer of the list's own is open, such as a drawer, which Escape closes before the selection */
  layer?: () => boolean;
  /**
   * Whether the line at `index` is rendered now, `element` being the one a focused control is
   * in; every line by default. A virtualised list renders a window of its lines
   */
  rendered?: (index: number, element?: HTMLElement) => boolean;
  /** brings the line at `index` into view; by default its element scrolls to the nearest edge */
  reveal?: (index: number) => void;
  /** the selection, when the list's page keeps it; the controller makes its own otherwise */
  selected?: SvelteSet<string>;
};

let lists = 0;

/**
 * The selection and keyboard focus of a list, shared by every list in the app so they all
 * behave alike.
 *
 * The container (`{@attach list.attach}`, with `tabindex="0"`) keeps DOM focus; lines are not
 * focusable. Each line's element carries `list.line(line, index)`. A click, or a focus on a
 * control inside a line, makes that line the focused one. A control inside a line keeps focus
 * while its line is rendered; focus returns to the container when the line scrolls out of the
 * rendered range or leaves the list, where the browser would drop it to the page; not once the
 * user has put focus elsewhere.
 *
 * The focused line is named by its key and looked up in the lines whenever it is used, so
 * filters, regrouping and new data never leave it pointing at a stale copy. When it leaves the
 * lines, the row now in its place takes over: the first row at or after its index, else the
 * nearest row before it. Up and Down move by position to the previous and next row, skipping group
 * headers; `activeId` names the focused line for `aria-activedescendant` while it is rendered.
 *
 * A click on a row outside its controls selects that row alone; with Ctrl or Cmd it toggles
 * the row, and with Shift it selects the rows from the anchor to it (adding them with Ctrl or
 * Cmd as well). A click that ends a text selection selects nothing.
 *
 * Keys: a focused control keeps the keys it handles (`keepsKey`); the rest act on the line the
 * control is in. Ctrl/Cmd+A selects the focused line's group, or every row; any other key with
 * Ctrl, Cmd or Alt is the browser's. On a row Space toggles it, Enter opens it and the list's
 * own keys act; on a group header only Ctrl/Cmd+A and Space act. Escape closes what is open
 * first (a menu, a dialog, a card, the list's own layer) and otherwise clears the selection
 */
export class ListControl<L> {
  /** the selected rows' ids */
  readonly selected: SvelteSet<string>;
  /** where a Shift range starts: the row last ticked on its own */
  anchor: string | null = null;
  /** prefix of the lines' element ids */
  readonly uid = `list-${++lists}`;
  /** the focused line's key */
  focusedKey = $state<string | null>(null);

  #spec: ListSpec<L>;
  #element = $state<HTMLElement | null>(null);
  /** the focused line's index the last time it was in the lines, for when it leaves */
  #lastIndex = 0;
  /** the control in the list that last took DOM focus, null while the container has it */
  #held: Element | null = null;

  readonly lines = $derived.by(() => this.#spec.lines());
  /** the rows' ids in the order they are shown */
  readonly rows = $derived(
    this.lines.flatMap((line) => {
      const id = this.#spec.id(line);
      return id === null ? [] : [id];
    })
  );
  #found = $derived(
    this.focusedKey === null
      ? -1
      : this.lines.findIndex((l) => this.#spec.key(l) === this.focusedKey)
  );
  /** where the focused line is in the lines, -1 when none is */
  readonly focusIndex = $derived.by(() => {
    if (this.focusedKey === null || !this.lines.length) return -1;
    if (this.#found !== -1) return this.#found;
    const after = this.#nextRow(this.#lastIndex - 1, 1);
    return after !== -1 ? after : this.#nextRow(Math.min(this.#lastIndex, this.lines.length), -1);
  });
  /** the focused line */
  readonly focused = $derived(this.focusIndex === -1 ? null : this.lines[this.focusIndex]);
  /** the id of the focused line's element while it is rendered, for `aria-activedescendant` */
  readonly activeId = $derived(
    this.focusIndex !== -1 && this.#rendered(this.focusIndex)
      ? this.lineId(this.focusIndex)
      : undefined
  );

  constructor(spec: ListSpec<L>) {
    this.#spec = spec;
    this.selected = spec.selected ?? new SvelteSet<string>();
    // remembers where the focused line is, and hands the focus to the row in a lost line's place
    $effect(() => {
      if (this.#found !== -1) this.#lastIndex = this.#found;
      else if (this.focused) this.focusedKey = this.#spec.key(this.focused);
    });
    // a change that removed the line holding focus left the page focused; the list takes it back
    $effect(() => {
      void this.lines;
      const active = document.activeElement;
      if (this.#held && !this.#held.isConnected && (!active || active === document.body)) {
        this.focus();
      }
    });
  }

  /** The element id of the line at `index` */
  lineId(index: number) {
    return `${this.uid}-${index}`;
  }

  /** The attributes a line's element carries */
  line(line: L, index: number) {
    const id = this.#spec.id(line);
    return {
      "id": this.lineId(index),
      "data-line": this.#spec.key(line),
      "aria-selected": id === null ? undefined : this.selected.has(id)
    };
  }

  /** Whether the line is the focused one, for its focus ring */
  isFocused(line: L) {
    return this.#spec.key(line) === this.focusedKey;
  }

  /** Gives the container DOM focus, without scrolling */
  focus = () => {
    this.#held = null;
    this.#element?.focus({ preventScroll: true });
  };

  /** Whether `element` is inside the list */
  has = (element: Element | null) => !!element && !!this.#element?.contains(element);

  /** Ticks a row on its own, which starts the next Shift range there */
  toggle(id: string) {
    this.anchor = id;
    if (this.selected.has(id)) this.selected.delete(id);
    else this.selected.add(id);
  }

  /** Selects one row and nothing else */
  only(id: string) {
    this.selected.clear();
    this.toggle(id);
  }

  /** A row's checkbox: Shift adds the rows from the anchor to it, otherwise it toggles */
  tick(id: string, range: boolean) {
    if (range && this.#range(id, true)) return;
    this.toggle(id);
  }

  /** Selects or clears every row in `ids` */
  setAll(ids: string[], on: boolean) {
    for (const id of ids) {
      if (on) this.selected.add(id);
      else this.selected.delete(id);
    }
  }

  /** Clears the selection */
  clear() {
    this.selected.clear();
  }

  /**
   * Gives focus back to the container when the control that has it is in a line about to stop
   * being rendered. For a virtualised list's scroll; a control the browser scrolled into view
   * keeps it
   */
  scrolled() {
    const element = this.#element;
    const active = document.activeElement;
    if (!element || !active || active === element || !element.contains(active)) return;
    const line = active.closest<HTMLElement>("[data-line]");
    if (!line) return;
    const at = this.lines.findIndex((l) => this.#spec.key(l) === line.dataset.line);
    if (!this.#rendered(at, line)) element.focus({ preventScroll: true });
  }

  /** For `{@attach}` on the list's container */
  attach = (node: HTMLElement) => {
    this.#element = node;
    const stops = [
      on(node, "pointerdown", (e) => this.#take(e.target)),
      on(node, "mousedown", this.#mousedown),
      on(node, "focusin", (e) => {
        this.#held = e.target === node ? null : (e.target as Element);
        this.#take(e.target);
      }),
      // focus the user put elsewhere stays there when a row leaves later. A control taken out of
      // the page loses focus too, and that one the list still takes back
      on(node, "focusout", (e) => {
        const to = e.relatedTarget;
        if (!(e.target as Element).isConnected) return;
        if (!(to instanceof Node && node.contains(to))) this.#held = null;
      }),
      on(node, "keydown", this.#keydown),
      on(node, "click", this.#click)
    ];
    return () => {
      for (const stop of stops) stop();
      if (this.#element === node) this.#element = null;
    };
  };

  #rendered(index: number, element?: HTMLElement) {
    return index !== -1 && (this.#spec.rendered?.(index, element) ?? true);
  }

  /** Adds (or with `add` false, sets) the rows from the anchor to `id`; false without an anchor shown */
  #range(id: string, add: boolean): boolean {
    const from = this.anchor === null ? -1 : this.rows.indexOf(this.anchor);
    const to = this.rows.indexOf(id);
    if (from === -1 || to === -1) return false;
    if (!add) this.selected.clear();
    for (const row of this.rows.slice(Math.min(from, to), Math.max(from, to) + 1)) {
      this.selected.add(row);
    }
    return true;
  }

  /** Makes the line `target` is in the focused one */
  #take(target: EventTarget | null) {
    const key =
      target instanceof Element ? target.closest<HTMLElement>("[data-line]")?.dataset.line : null;
    if (key !== undefined && key !== null) this.focusedKey = key;
  }

  /** The index of the nearest row after (`step` 1) or before (-1) `from`, -1 when none */
  #nextRow(from: number, step: 1 | -1) {
    for (let i = from + step; i >= 0 && i < this.lines.length; i += step) {
      if (this.#spec.id(this.lines[i]) !== null) return i;
    }
    return -1;
  }

  /** Focuses the row at `index`, the container taking DOM focus, and brings it into view */
  #moveTo(index: number) {
    const line = this.lines[index];
    if (!line || this.#spec.id(line) === null || !this.#element) return;
    this.focusedKey = this.#spec.key(line);
    if (document.activeElement !== this.#element) this.#element.focus({ preventScroll: true });
    if (this.#spec.reveal) this.#spec.reveal(index);
    else document.getElementById(this.lineId(index))?.scrollIntoView({ block: "nearest" });
  }

  /**
   * A press with Shift, Ctrl or Cmd on a row is a selection gesture: the browser would extend
   * the page's text selection with it (and that click would then select nothing), so it is kept
   * from doing so and the container takes focus as a press would give it
   */
  #mousedown = (e: MouseEvent) => {
    if (e.button !== 0 || !(e.shiftKey || e.ctrlKey || e.metaKey) || inControl(e)) return;
    if (!(e.target as Element).closest("[data-line]")) return;
    e.preventDefault();
    this.#element?.focus({ preventScroll: true });
  };

  #click = (e: MouseEvent) => {
    if (e.defaultPrevented || inControl(e)) return;
    const toggle = e.ctrlKey || e.metaKey;
    // a plain click that ends a drag across text selects nothing; a modified one never selects
    // text, its press was kept from it
    const selection = document.getSelection();
    if (!toggle && !e.shiftKey && selection && !selection.isCollapsed) return;
    const key = (e.target as Element).closest<HTMLElement>("[data-line]")?.dataset.line;
    const line = this.lines.find((l) => this.#spec.key(l) === key);
    const id = line === undefined ? null : this.#spec.id(line);
    if (id === null) return;
    if (e.shiftKey && this.#range(id, toggle)) return;
    if (toggle) return this.toggle(id);
    this.only(id);
    if (this.#spec.openOnClick && line) this.#spec.onopen?.(line);
  };

  #keydown = (e: KeyboardEvent) => {
    if (keepsKey(e)) return;
    this.#take(e.target);
    const line = this.focused;
    const id = line === null ? null : this.#spec.id(line);
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "a") {
      this.setAll((line && this.#spec.group?.(line)) ?? this.rows, true);
    } else if (withModifier(e)) return;
    else if (e.key === "ArrowDown") {
      const to = this.#nextRow(this.focusIndex, 1);
      if (to !== -1) this.#moveTo(to);
    } else if (e.key === "ArrowUp") {
      const to = this.focusIndex === -1 ? this.#nextRow(-1, 1) : this.#nextRow(this.focusIndex, -1);
      if (to !== -1) this.#moveTo(to);
    } else if (e.key === "Escape") {
      if (this.#spec.layer?.() || layerOpen()) return;
      this.clear();
    } else if (line === null) return;
    else if (id === null) {
      if (e.key !== " ") return;
      const ids = this.#spec.group?.(line) ?? [];
      this.setAll(ids, !ids.every((one) => this.selected.has(one)));
    } else if (e.key === " ") this.toggle(id);
    else if (e.key === "Enter") {
      if (!this.#spec.onopen) return;
      this.#spec.onopen(line);
    } else if (!this.#spec.onkey?.(e, line)) return;
    e.preventDefault();
  };
}
