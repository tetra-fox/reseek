import { on } from "svelte/events";

import { inControl } from "#lib/ui/keys.js";
import { LONG_PRESS_MS } from "#lib/users/hover.svelte.js";

import { interaction } from "./interaction.svelte";

/** Where a line's menu hangs: a point, or an element for the keys */
export type MenuAnchor = HTMLElement | { getBoundingClientRect: () => DOMRect } | null;

/** What a list tells its line menu */
export type LineMenuSpec<L> = {
  /** the lines in the order they are shown */
  lines: () => L[];
  /** a line's stable key, which its element carries as `data-line` */
  key: (line: L) => string;
  /** whether the list has a menu at all */
  enabled: () => boolean;
  /** whether a line has anything in its menu; a line without opens none */
  menuable?: (line: L) => boolean;
  /** the line with keyboard focus, which the menu key opens the menu of */
  focused: () => L | null;
  /** gives the line keyboard focus, as opening its menu does */
  focus: (line: L) => void;
  /** the element the focused line is drawn as, which a menu opened from the keys hangs under */
  anchor: () => HTMLElement | null;
};

/**
 * The menu of a list's lines, opened by a right-click or a long press on a line, at the pointer,
 * or by the menu key or Shift+F10, under the focused line. A link and selected text keep the
 * browser's own menu. While a menu is open the lines hold still. Put `attach` on the list's
 * container, pass its keys to `key`, and render the menu while `line` is set
 */
export class LineMenu<L> {
  #spec: LineMenuSpec<L>;
  #at = $state.raw<{ key: string; anchor: MenuAnchor } | null>(null);
  #press: ReturnType<typeof setTimeout> | undefined;
  /** the press that ends in a click opened the menu, so the click selects nothing */
  #pressed = false;

  /** the line whose menu is open */
  readonly line: L | null = $derived.by(() => {
    const at = this.#at;
    return at ? (this.#spec.lines().find((line) => this.#spec.key(line) === at.key) ?? null) : null;
  });

  constructor(spec: LineMenuSpec<L>) {
    this.#spec = spec;
    // the hold follows whether a menu is open, not the line, which is a new object each time the
    // lines are made again
    const open = $derived(this.line !== null);
    $effect(() => (open ? interaction.hold() : undefined));
    $effect(() => () => this.#released());
  }

  /** where the open menu hangs */
  get anchor(): MenuAnchor {
    return this.#at?.anchor ?? null;
  }

  /** Opens a line's menu at the point `at`, or under the line */
  open(line: L, at?: { x: number; y: number }) {
    this.#spec.focus(line);
    if (!(this.#spec.menuable?.(line) ?? true)) return;
    const anchor = at
      ? { getBoundingClientRect: () => new DOMRect(at.x, at.y, 0, 0) }
      : this.#spec.anchor();
    this.#at = { key: this.#spec.key(line), anchor };
  }

  /** Closes the menu */
  close = () => {
    this.#at = null;
  };

  /** The menu key and Shift+F10 on a line open its menu; true when one of them was pressed */
  key(e: KeyboardEvent, line: L): boolean {
    if (!this.#spec.enabled() || !(e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey))) {
      return false;
    }
    this.open(line);
    return true;
  }

  /** For `{@attach}` on the list's container */
  attach = (node: HTMLElement) => {
    const stops = [
      on(node, "contextmenu", this.#contextmenu),
      on(node, "pointerdown", this.#pointerdown),
      on(node, "pointerup", this.#released),
      on(node, "pointercancel", this.#released),
      on(
        node,
        "click",
        (e) => {
          if (this.#pressed) e.preventDefault();
          this.#pressed = false;
        },
        { capture: true }
      )
    ];
    return () => {
      for (const stop of stops) stop();
    };
  };

  #lineOf(target: EventTarget | null) {
    const key =
      target instanceof Element ? target.closest<HTMLElement>("[data-line]")?.dataset.line : null;
    return key ? this.#spec.lines().find((line) => this.#spec.key(line) === key) : undefined;
  }

  #released = () => clearTimeout(this.#press);

  /** A touch held on a line opens its menu, a touch having no right-click */
  #pointerdown = (e: PointerEvent) => {
    this.#pressed = false;
    const line = this.#lineOf(e.target);
    if (!this.#spec.enabled() || e.pointerType !== "touch" || inControl(e) || !line) return;
    const at = { x: e.clientX, y: e.clientY };
    this.#press = setTimeout(() => {
      this.#pressed = true;
      if (this.#at?.key !== this.#spec.key(line)) this.open(line, at);
    }, LONG_PRESS_MS);
  };

  /**
   * A right-click on a line opens its menu at the pointer. The menu key's event, which comes to
   * the container itself, opens the focused line's
   */
  #contextmenu = (e: MouseEvent) => {
    this.#released();
    if (!this.#spec.enabled() || e.defaultPrevented || !document.getSelection()?.isCollapsed) {
      return;
    }
    if (e.target instanceof Element && e.target.closest("a[href]")) return;
    const hit = this.#lineOf(e.target);
    const line = hit ?? this.#spec.focused();
    if (!line) return;
    e.preventDefault();
    // the keys opened it already, before their own event came
    if (this.#at?.key === this.#spec.key(line)) return;
    this.open(line, hit ? { x: e.clientX, y: e.clientY } : undefined);
  };
}
