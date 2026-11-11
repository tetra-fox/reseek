import { LIST } from "#lib/ui/bindings.js";
import { isPressed } from "#lib/ui/keys.js";

/** How far a press moves, in px, before it is a drag and not a click */
const SLOP = 4;
/** How near the list's top or bottom edge, in px, the pointer scrolls the list */
const EDGE = 32;
/** How far the list scrolls in a frame there, in px */
const STEP = 6;

/** Where a row is in the list's content when the drag starts */
type Slot = { key: string; top: number; height: number };

/**
 * Dragging a row to another place among the rows of its group, by plain pointer events. The rows
 * are measured as the drag starts and drawn shifted from there, so nothing in the list's own order
 * changes until the drop. Near the list's edge the list scrolls.
 *
 * The list calls `press` from a row's or a handle's `pointerdown` and draws each row at
 * `shift(key)`. A touch reaches this only from a handle with `touch-action: none`, which leaves
 * the rest of a row to scroll the list. Escape puts the row back
 */
export class RowDrag {
  /** the row being dragged, null while none is */
  key = $state<string | null>(null);
  #slots = $state.raw<Slot[]>([]);
  /** how far the dragged row is from its place, in px */
  #offset = $state(0);
  /** the place it would drop at, among the rows */
  #to = $state(0);

  #box: () => HTMLElement | null;
  #ondrop: (key: string, before: string | null) => void;
  /** the pressed pointer, null while none is */
  #pointer: number | null = null;
  #mouse = false;
  #pressed = "";
  #rows: string[] = [];
  #target: Element | null = null;
  #startY = 0;
  #startScroll = 0;
  #y = 0;
  #frame = 0;

  /**
   * `box` gives the element that scrolls the rows. `ondrop` hears of a row dropped at a new place,
   * in front of `before`, or last without one
   */
  constructor(box: () => HTMLElement | null, ondrop: (key: string, before: string | null) => void) {
    this.#box = box;
    this.#ondrop = ondrop;
  }

  /** A press on the row `key` of the group `rows` starts a drag once it moves */
  press(e: PointerEvent, key: string, rows: string[]) {
    if (this.#pointer !== null || rows.length < 2) return;
    if (e.button !== 0 || e.shiftKey || e.ctrlKey || e.metaKey) return;
    this.#pointer = e.pointerId;
    this.#mouse = e.pointerType === "mouse";
    this.#pressed = key;
    this.#rows = rows;
    this.#target = e.target instanceof Element ? e.target : null;
    this.#startY = this.#y = e.clientY;
    window.addEventListener("pointermove", this.#moved);
    window.addEventListener("pointerup", this.#released);
    window.addEventListener("pointercancel", this.#canceled);
    window.addEventListener("keydown", this.#escaped, { capture: true });
  }

  /** The `translate` a row is drawn with, undefined when it stays where it is */
  shift(key: string) {
    if (this.key === null) return undefined;
    if (key === this.key) return `0 ${this.#offset}px`;
    const from = this.#slots.findIndex((slot) => slot.key === this.key);
    const at = this.#slots.findIndex((slot) => slot.key === key);
    const height = this.#slots[from]?.height ?? 0;
    if (at > from && at <= this.#to) return `0 ${-height}px`;
    if (at < from && at >= this.#to) return `0 ${height}px`;
    return undefined;
  }

  #moved = (e: PointerEvent) => {
    if (e.pointerId !== this.#pointer) return;
    this.#y = e.clientY;
    if (this.key === null) {
      if (Math.abs(this.#y - this.#startY) < SLOP) return;
      this.#begin();
    }
    this.#place();
  };

  #released = (e: PointerEvent) => {
    if (e.pointerId !== this.#pointer) return;
    const key = this.key;
    const others = this.#slots.filter((slot) => slot.key !== key);
    const from = this.#slots.findIndex((slot) => slot.key === key);
    const to = this.#to;
    this.#end();
    if (key !== null && to !== from) this.#ondrop(key, others[to]?.key ?? null);
  };

  #canceled = (e: PointerEvent) => {
    if (e.pointerId === this.#pointer) this.#end();
  };

  #escaped = (e: KeyboardEvent) => {
    if (!isPressed(LIST.cancelDrag, e) || this.key === null) return;
    e.stopPropagation();
    e.preventDefault();
    this.#end();
  };

  #begin() {
    const box = this.#box();
    if (!box) return this.#end();
    const scroll = box.scrollTop;
    const elements = new Map(
      [...box.querySelectorAll<HTMLElement>("[data-line]")].map((e) => [e.dataset.line, e])
    );
    // by where they are, as the list may be holding an older order than the group's
    this.#slots = this.#rows
      .flatMap((key) => {
        const rect = elements.get(key)?.getBoundingClientRect();
        return rect ? [{ key, top: rect.top + scroll, height: rect.height }] : [];
      })
      .toSorted((a, b) => a.top - b.top);
    this.#startScroll = scroll;
    this.#to = this.#slots.findIndex((slot) => slot.key === this.#pressed);
    this.key = this.#pressed;
    if (this.#pointer !== null) this.#target?.setPointerCapture(this.#pointer);
    this.#frame = requestAnimationFrame(this.#scroll);
  }

  /** Follows the pointer, within the first and the last row's places */
  #place() {
    const box = this.#box();
    const me = this.#slots.find((slot) => slot.key === this.key);
    const first = this.#slots[0];
    const last = this.#slots.at(-1);
    if (!box || !me || !first || !last) return;
    const moved = this.#y - this.#startY + box.scrollTop - this.#startScroll;
    const lowest = last.top + last.height - me.height - me.top;
    this.#offset = Math.min(Math.max(moved, first.top - me.top), lowest);
    // a row gives way once the dragged row's leading edge is past its middle
    const from = this.#slots.indexOf(me);
    const top = me.top + this.#offset;
    const middle = (slot: Slot) => slot.top + slot.height / 2;
    const below = this.#slots.slice(from + 1).filter((s) => middle(s) < top + me.height);
    const above = this.#slots.slice(0, from).filter((s) => middle(s) > top);
    this.#to = from + below.length - above.length;
  }

  #scroll = () => {
    const box = this.#box();
    if (this.key === null || !box) return;
    const { top, bottom } = box.getBoundingClientRect();
    const by = this.#y < top + EDGE ? -STEP : this.#y > bottom - EDGE ? STEP : 0;
    if (by) {
      box.scrollTop += by;
      this.#place();
    }
    this.#frame = requestAnimationFrame(this.#scroll);
  };

  #end() {
    window.removeEventListener("pointermove", this.#moved);
    window.removeEventListener("pointerup", this.#released);
    window.removeEventListener("pointercancel", this.#canceled);
    window.removeEventListener("keydown", this.#escaped, { capture: true });
    cancelAnimationFrame(this.#frame);
    // the mouse's release comes with a click, which must not open the row. A touch that moved
    // brings none
    if (this.key !== null && this.#mouse) swallowClick();
    this.key = null;
    this.#slots = [];
    this.#offset = 0;
    this.#pointer = null;
  }
}

/** Takes the next click away from the page, until the next press, which brings a click of its own */
const swallowClick = () => {
  const swallow = (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
  };
  window.addEventListener("click", swallow, { capture: true, once: true });
  window.addEventListener(
    "pointerdown",
    () => window.removeEventListener("click", swallow, { capture: true }),
    { capture: true, once: true }
  );
};
