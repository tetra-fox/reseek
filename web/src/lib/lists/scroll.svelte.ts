import { untrack } from "svelte";

import { route } from "#lib/nav.js";
import { fields, stored } from "#lib/stored.svelte.js";

/**
 * Where a scroll box was left: the line at its top by key and by index, how far that line's
 * top was below the box's top (negative once scrolled past), and the offset in pixels, for a
 * box without lines
 */
export type ScrollMark = { key: string | null; index: number; delta: number; top: number };

/** A mark as it is kept: with the query of the address it was left at */
type Kept = ScrollMark & { query: string };

/** The query of the address in view, its parameters in one order */
const addressQuery = () => {
  const query = route(location).query;
  query.sort();
  return query.toString();
};

/** how many boxes are remembered; the one left longest ago is dropped first */
const KEPT = 100;
/** how long a box being brought back waits for the line it was left at, in ms */
const WAIT_MS = 2000;
/** how long scrolling rests before where a box is gets saved, in ms */
const SAVE_MS = 300;

const readMark = (saved: unknown): Kept | null => {
  const m = fields(saved);
  const numbers = [m.index, m.delta, m.top].every((n) => typeof n === "number" && isFinite(n));
  if (!numbers) return null;
  const key = typeof m.key === "string" ? m.key : null;
  const query = typeof m.query === "string" ? m.query : "";
  return { key, index: m.index as number, delta: m.delta as number, top: m.top as number, query };
};

/** Where each remembered box was left, by its name, the one left most recently last */
const marks = stored("reseek.scroll", (saved): Record<string, Kept> => {
  const kept: Record<string, Kept> = {};
  for (const [name, value] of Object.entries(fields(saved)).slice(-KEPT)) {
    const mark = readMark(value);
    if (mark) kept[name] = mark;
  }
  return kept;
});

/**
 * Where the box named `name` was left, if it was left at the address in view: the same screen
 * under another filter or page starts at its top
 */
export const markOf = (name: string): ScrollMark | null =>
  untrack(() => {
    const mark = marks[name];
    return mark && mark.query === addressQuery() ? mark : null;
  });

/** Remembers where the box named `name` is left; null forgets it, for a box that starts as new */
export const remember = (name: string, mark: Kept | null) => {
  delete marks[name];
  if (mark === null) return;
  marks[name] = mark;
  const names = Object.keys(marks);
  for (const old of names.slice(0, Math.max(0, names.length - KEPT))) delete marks[old];
};

/**
 * Where a box goes back to among lines with these keys: the line it was left at, wherever it
 * is now, or while that line is not there (yet), the line now at its old index. Null while there
 * are fewer lines than that
 */
export const landing = (mark: ScrollMark, keys: string[]) => {
  const at = mark.key === null ? -1 : keys.indexOf(mark.key);
  if (at !== -1) return { index: at, delta: mark.delta };
  return keys.length > mark.index ? { index: mark.index, delta: mark.delta } : null;
};

/**
 * A box being brought back to where it was left. It is put there again each time its lines
 * change, as when a list read again brings rows above it, until something else scrolls it or a
 * short wait is over; then it stays where it is
 */
export class Return {
  #mark: ScrollMark | null;
  #until = performance.now() + WAIT_MS;
  /** the offset the return last put the box at, null before it has put it anywhere */
  #at: number | null = null;

  constructor(mark: ScrollMark | null) {
    this.#mark = mark;
  }

  /** the mark still to go back to, or null once done */
  get mark(): ScrollMark | null {
    if (this.#mark && performance.now() > this.#until) this.#mark = null;
    return this.#mark;
  }

  /**
   * Whether the return goes on. It ends once `box` is not where the return last put it: a scroll
   * the return did not make, by the user or by the page, as to a row a link names. The browser
   * holding the offset within content that shrank under it does not end it. Asked before every
   * put as well as at every scroll event, since the page may scroll the box and add lines before
   * the event comes
   */
  holds(box: HTMLElement): boolean {
    if (!this.mark) return false;
    if (this.#at === null) return true;
    const top = box.scrollTop;
    const clamped = top < this.#at && top >= box.scrollHeight - box.clientHeight - 1;
    if (Math.abs(top - this.#at) > 1 && !clamped) {
      this.#mark = null;
      return false;
    }
    this.#at = top;
    return true;
  }

  /** Scrolls `box` to `top`, as the return's own scroll; asked once `holds` says it goes on */
  put(box: HTMLElement, top: number) {
    box.scrollTop = top;
    this.#at = box.scrollTop;
  }
}

/** Saves a box's mark a moment after its scrolling rests, and at once when asked */
export class Saver {
  #name: string;
  /** what to save next, null to forget the box, undefined for nothing */
  #last: Kept | null | undefined;
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(name: string) {
    this.#name = name;
  }

  /**
   * Where the box is now, saved once the scrolling rests, with the address it is at now, as the
   * address has moved on by the time the box is left; null to forget it
   */
  note(mark: ScrollMark | null) {
    this.#last = mark && { ...mark, query: addressQuery() };
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.flush(), SAVE_MS);
  }

  flush() {
    clearTimeout(this.#timer);
    if (this.#last !== undefined) remember(this.#name, this.#last);
    this.#last = undefined;
  }
}

/** Whether `line` moves with `box`'s scroll, not with that of a box inside it that scrolls */
const scrollsWith = (line: Element, box: Element) => {
  for (let at = line.parentElement; at && at !== box; at = at.parentElement) {
    const { overflowY } = getComputedStyle(at);
    if ((overflowY === "auto" || overflowY === "scroll") && at.scrollHeight > at.clientHeight) {
      return false;
    }
  }
  return true;
};

/** The line at the box's top, of those carrying `data-line`, and where it is */
const markIn = (box: HTMLElement): ScrollMark => {
  const top = box.getBoundingClientRect().top;
  const lines = box.querySelectorAll<HTMLElement>("[data-line]");
  for (const [index, line] of [...lines].entries()) {
    const rect = line.getBoundingClientRect();
    if (rect.bottom > top && scrollsWith(line, box)) {
      return { key: line.dataset.line ?? null, index, delta: rect.top - top, top: box.scrollTop };
    }
  }
  return { key: null, index: 0, delta: 0, top: box.scrollTop };
};

/** Where `box` goes for `mark`: to the mark's line while it is in the page, else its offset */
const placeIn = (box: HTMLElement, mark: ScrollMark) => {
  const line =
    mark.key === null ? null : box.querySelector(`[data-line="${CSS.escape(mark.key)}"]`);
  if (!line) return mark.top;
  return (
    box.scrollTop + line.getBoundingClientRect().top - box.getBoundingClientRect().top - mark.delta
  );
};

/**
 * For `{@attach}` on a scroll box whose lines are all in the page: the box is remembered under
 * `name`, by the line at its top where its lines carry `data-line`, else by its offset. It starts
 * where it was left, on the same line even where lines came or went, and at the top otherwise.
 * A box left at its top is forgotten, so it opens at the top, over rows that came meanwhile
 */
export const keepsScroll = (name: string) => (box: HTMLElement) => {
  const back = new Return(markOf(name));
  const saver = new Saver(name);
  let frame = 0;
  const scrolled = () => {
    back.holds(box);
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      if (!back.mark) saver.note(box.scrollTop === 0 ? null : markIn(box));
    });
  };
  // the lines arrive after the box, as their data is read
  const grown = new MutationObserver(() => retry());
  const retry = () => {
    const mark = back.mark;
    if (mark && back.holds(box)) back.put(box, placeIn(box, mark));
    else if (!back.mark) grown.disconnect();
  };
  if (back.mark) {
    grown.observe(box, { childList: true, subtree: true });
    retry();
  } else box.scrollTop = 0;
  box.addEventListener("scroll", scrolled, { passive: true });
  return () => {
    cancelAnimationFrame(frame);
    grown.disconnect();
    box.removeEventListener("scroll", scrolled);
    saver.flush();
  };
};
