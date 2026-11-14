import { untrack } from "svelte";
import { on } from "svelte/events";
import { SvelteSet } from "svelte/reactivity";

import { inspector } from "#lib/inspector/inspector.svelte.js";
import { DETAILS, LIST } from "#lib/ui/bindings.js";
import { inControl, isPressed, keepsKey, keyStep, layerOpen, withModifier } from "#lib/ui/keys.js";

import { TYPE_AHEAD_MS, typeAhead } from "./typeahead";

/** The header's Select all: whether every row it stands for is ticked, or some are */
export type AllTick = {
  checked: boolean;
  indeterminate: boolean;
  empty: boolean;
  toggle: () => void;
};

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
  /**
   * The ids the header's Select all ticks, and Ctrl/Cmd+A outside a group: every row the list
   * shows by default, more for a tree whose closed folders hold rows too
   */
  all?: () => string[];
  /** Enter on a row */
  onopen?: (line: L) => void;
  /** a plain click on a row opens it too, for lists whose rows are what a pane shows */
  openOnClick?: boolean;
  /** a key of the list's own for the focused row, such as Delete; true when it acted */
  onkey?: (e: KeyboardEvent, line: L) => boolean;
  /**
   * The same for a key held with Ctrl, Cmd or Alt, for the focused line, which is the browser's
   * unless this acts on it
   */
  onchord?: (e: KeyboardEvent, line: L) => boolean;
  /**
   * Whether the inspector can show a line. On such a line Space and `i` open or close it, and
   * Shift+Space ticks the row, which Space does in a list without an inspector
   */
  inspects?: (line: L) => boolean;
  /** focus came to a line, for the inspector to follow it */
  onfocus?: (line: L) => void;
  /**
   * Whether the line at `index` is rendered now, `element` being the one a focused control is
   * in; every line by default. A virtualized list renders a window of its lines
   */
  rendered?: (index: number, element?: HTMLElement) => boolean;
  /** brings the line at `index` into view; by default its element scrolls to the nearest edge */
  reveal?: (index: number) => void;
  /** the selection, when the list's page keeps it; the controller makes its own otherwise */
  selected?: SvelteSet<string>;
  /**
   * Group headers take focus as rows do: Up and Down stop on them, and Enter and the list's own
   * keys act on them. They stay out of the selection; Space ticks their group
   */
  headersFocus?: boolean;
  /** a line's name, for finding a line by typing the start of it */
  typeahead?: (line: L) => string;
  /** how many lines Page Up and Page Down move by: about a screenful */
  page?: () => number;
};

let lists = 0;

/** Whether `target` is inside a list's container */
export const inList = (target: EventTarget | null) =>
  target instanceof Element && !!target.closest("[data-list]");

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
 * The focused line shows its ring while focus is in the list and got there by the keys, as
 * `:focus-visible` would: a press anywhere in the list hides it until the next key.
 *
 * Keys: a focused control keeps the keys it handles (`keepsKey`); the rest act on the line the
 * control is in. Ctrl/Cmd+A selects the focused line's group, or what Select all stands for; any other key with
 * Ctrl, Cmd or Alt is the browser's, unless the list takes it as its own. Home, End, Page Up and
 * Page Down move as Up and Down do, further. On a row Space toggles it, Enter opens it and the list's own keys act; on a group
 * header only Ctrl/Cmd+A and Space act, unless headers take focus. In a list with an inspector,
 * Space and `i` on a line it shows open or close it instead, and Shift+Space ticks. Letters typed
 * one after another go to the next line whose name starts with them, in a list that names its
 * lines; an `i` in the middle of those is one of them. Escape, whatever is held with it, closes
 * what is open first (a menu, a dialog, a card) and otherwise clears the selection
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
  /** whether the focused line shows its ring: focus is in the list and came from the keys */
  #ringed = $state(false);

  #spec: ListSpec<L>;
  #element = $state<HTMLElement | null>(null);
  /** the focused line's index the last time it was in the lines, for when it leaves */
  #lastIndex = 0;
  /** the control in the list that last took DOM focus, null while the container has it */
  #held: Element | null = null;
  /** what has been typed to find a line, and when the last letter came */
  #typed = "";
  #typedAt = 0;

  readonly lines = $derived.by(() => this.#spec.lines());
  /** the rows' ids in the order they are shown */
  readonly rows = $derived(
    this.lines.flatMap((line) => {
      const id = this.#spec.id(line);
      return id === null ? [] : [id];
    })
  );
  /** the ids Select all stands for */
  readonly everything = $derived.by(() => this.#spec.all?.() ?? this.rows);
  /** the header's Select all, which ticks every row it stands for or, once all are, clears them */
  readonly allTick: AllTick = $derived.by(() => {
    const ids = this.everything;
    const on = ids.filter((id) => this.selected.has(id)).length;
    return {
      checked: on > 0 && on === ids.length,
      indeterminate: on > 0 && on < ids.length,
      empty: !ids.length,
      toggle: () => this.setAll(ids, on < ids.length)
    };
  });
  #found = $derived(
    this.focusedKey === null
      ? -1
      : this.lines.findIndex((l) => this.#spec.key(l) === this.focusedKey)
  );
  /** where the focused line is in the lines, -1 when none is */
  readonly focusIndex = $derived.by(() => {
    if (this.focusedKey === null || !this.lines.length) return -1;
    if (this.#found !== -1) return this.#found;
    const after = this.#next(this.#lastIndex - 1, 1);
    return after !== -1 ? after : this.#next(Math.min(this.#lastIndex, this.lines.length), -1);
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
    // the inspector shows the line with focus, which the mouse and the keys both move
    $effect(() => {
      const line = this.focused;
      if (line !== null) untrack(() => this.#spec.onfocus?.(line));
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

  /**
   * The attributes a line's element carries: `data-focused` draws the focus ring, `data-inspected`
   * marks the line the open inspector shows, and `data-last` leaves the last line's divider off
   */
  line(line: L, index: number) {
    const id = this.#spec.id(line);
    return {
      "id": this.lineId(index),
      "data-line": this.#spec.key(line),
      "data-focused": this.isFocused(line) || undefined,
      "data-inspected": this.#inspected(line) || undefined,
      "data-last": index === this.lines.length - 1 || undefined,
      "aria-selected": id === null ? undefined : this.selected.has(id)
    };
  }

  /** Whether the line shows the focus ring: it is the focused one and focus came from the keys */
  isFocused(line: L) {
    return this.#ringed && this.#spec.key(line) === this.focusedKey;
  }

  /** Whether the inspector is open on the line */
  #inspected(line: L) {
    return (
      inspector.open && this.#spec.key(line) === this.focusedKey && !!this.#spec.inspects?.(line)
    );
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
   * being rendered. For a virtualized list's scroll; a control the browser scrolled into view
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
    node.dataset.list = "";
    const stops = [
      on(node, "pointerdown", (e) => {
        this.#ringed = false;
        this.#take(e.target);
      }),
      on(node, "mousedown", this.#mousedown),
      on(node, "focusin", (e) => {
        const target = e.target as Element;
        this.#held = target === node ? null : target;
        // Tab into the list shows the ring; a press already hid it, and a key already showed it
        if (target.matches(":focus-visible")) this.#ringed = true;
        this.#take(target);
      }),
      // focus the user put elsewhere stays there when a row leaves later. A control taken out of
      // the page loses focus too, and that one the list still takes back
      on(node, "focusout", (e) => {
        const to = e.relatedTarget;
        if (!(e.target as Element).isConnected) return;
        if (!(to instanceof Node && node.contains(to))) {
          this.#held = null;
          this.#ringed = false;
        }
      }),
      on(node, "keydown", this.#keydown),
      on(node, "click", this.#click)
    ];
    return () => {
      for (const stop of stops) stop();
      delete node.dataset.list;
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

  /** Whether focus stops on a line: every row, and headers when they take focus */
  #stops(line: L) {
    return this.#spec.headersFocus || this.#spec.id(line) !== null;
  }

  /**
   * The index of the nearest line focus stops on after (`step` 1) or before (-1) `from`, -1 when
   * none
   */
  #next(from: number, step: 1 | -1) {
    for (let i = from + step; i >= 0 && i < this.lines.length; i += step) {
      if (this.#stops(this.lines[i])) return i;
    }
    return -1;
  }

  /** Focuses the line at `index`, the container taking DOM focus, and brings it into view */
  #moveTo(index: number) {
    const line = this.lines[index];
    if (!line || !this.#stops(line) || !this.#element) return;
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
    // Shift alone is the start of a Shift-click as often as of a key
    if (!withModifier(e) && e.key !== "Shift") this.#ringed = true;
    if (keepsKey(e)) return;
    this.#take(e.target);
    const line = this.focused;
    const id = line === null ? null : this.#spec.id(line);
    if (isPressed(LIST.selectAll, e)) {
      this.setAll((line && this.#spec.group?.(line)) ?? this.everything, true);
    } else if (isPressed(LIST.clear, e)) {
      if (layerOpen()) return;
      this.clear();
    } else if (withModifier(e)) {
      if (line === null || !this.#spec.onchord?.(e, line)) return;
      this.#ringed = true;
    } else if (keyStep(LIST.move, e) === 1) {
      const to = this.#next(this.focusIndex, 1);
      if (to !== -1) this.#moveTo(to);
    } else if (keyStep(LIST.move, e) === -1) {
      const to = this.focusIndex === -1 ? this.#next(-1, 1) : this.#next(this.focusIndex, -1);
      if (to !== -1) this.#moveTo(to);
    } else if (isPressed(LIST.ends, e)) {
      const to =
        keyStep(LIST.ends, e) === -1 ? this.#next(-1, 1) : this.#next(this.lines.length, -1);
      if (to !== -1) this.#moveTo(to);
    } else if (isPressed(LIST.page, e)) {
      const way = keyStep(LIST.page, e) === 1 ? 1 : -1;
      const by = Math.max(1, this.#spec.page?.() ?? 10);
      const want = Math.min(Math.max(this.focusIndex + way * by, 0), this.lines.length - 1);
      // the line a page away, or the nearest one focus stops on past it, else before it
      const past = this.#next(want - way, way);
      const to = past !== -1 ? past : this.#next(want, way === 1 ? -1 : 1);
      if (to !== -1) this.#moveTo(to);
    } else if (this.#togglesInspector(e, line)) {
      inspector.toggle();
    } else if (
      id !== null &&
      line !== null &&
      this.#spec.inspects?.(line) &&
      isPressed(DETAILS.select, e)
    ) {
      // Space shows the line in the inspector, so Shift+Space ticks it
      this.toggle(id);
    } else if (this.#findTyped(e)) {
      // a letter went to the line named by what was typed
    } else if (line === null) return;
    else if (id === null && isPressed(LIST.select, e)) {
      const ids = this.#spec.group?.(line) ?? [];
      this.setAll(ids, !ids.every((one) => this.selected.has(one)));
    } else if (id === null && !this.#spec.headersFocus) return;
    else if (id !== null && isPressed(LIST.select, e)) this.toggle(id);
    else if (isPressed(LIST.open, e)) {
      if (!this.#spec.onopen) return;
      this.#spec.onopen(line);
    } else if (!this.#spec.onkey?.(e, line)) return;
    e.preventDefault();
  };

  /** Whether a key opens or closes the inspector: Space, or `i` unless a name is being typed */
  #togglesInspector(e: KeyboardEvent, line: L | null) {
    const inspects = this.#spec.inspects;
    if (!inspects || (line !== null && !inspects(line))) return false;
    if (!isPressed(DETAILS.details, e)) return false;
    // an `i` in the middle of a name being typed is part of the name
    return !isPressed(LIST.find, e) || Date.now() - this.#typedAt > TYPE_AHEAD_MS;
  }

  /**
   * A letter typed while the list names its lines: what was typed in the last moment, with it,
   * goes to the next line whose name starts with that. True when a line was found
   */
  #findTyped(e: KeyboardEvent) {
    const name = this.#spec.typeahead;
    if (!name || !isPressed(LIST.find, e)) return false;
    const now = Date.now();
    this.#typed = now - this.#typedAt > TYPE_AHEAD_MS ? e.key : this.#typed + e.key;
    this.#typedAt = now;
    const at = typeAhead(this.lines.map(name), this.focusIndex, this.#typed);
    if (at === -1) return false;
    this.#moveTo(at);
    return true;
  }
}
