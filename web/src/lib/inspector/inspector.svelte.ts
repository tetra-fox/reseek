import type { Snippet } from "svelte";

import { fields, stored } from "#lib/stored.svelte.js";
import { viewport } from "#lib/ui/viewport.svelte.js";

/** The docked inspector's least width, in pixels */
export const PANE_MIN = 320;
/** The docked inspector's default width, in pixels */
export const PANE_WIDTH = 384;

/** The docked inspector's greatest width: half the window, the list keeping the rest */
export const paneMax = () => Math.max(PANE_MIN, Math.round(window.innerWidth * 0.5));

/**
 * How long focus rests on a file before the inspector asks the daemon about it, so holding an
 * arrow key down the list asks once, for the file it stops on
 */
export const SETTLE_MS = 150;

/** The folded sections and whether each starts open */
const SECTIONS = { details: true, analysis: true, history: true, technical: false };
export type SectionId = keyof typeof SECTIONS;

/** One line of a section, left out when it has no value */
export type Field = { label: string; value: string | null | undefined };

/** The inspector's open state, width and open sections as `saved` holds them */
export const readPrefs = (saved: unknown) => {
  const s = fields(saved);
  const sections = fields(s.sections);
  const open = (id: SectionId) => (typeof sections[id] === "boolean" ? sections[id] : SECTIONS[id]);
  return {
    open: s.open === true,
    width: typeof s.width === "number" && s.width >= PANE_MIN ? s.width : PANE_WIDTH,
    sections: {
      details: open("details"),
      analysis: open("analysis"),
      history: open("history"),
      technical: open("technical")
    }
  };
};

/** Whether the inspector is open, its width and which sections are open, kept in this browser */
export const inspectorPrefs = stored("reseek.inspector", readPrefs);

/**
 * The inspector the shell shows beside the content: docked and remembered on a wider screen, and
 * on a phone a sheet opened for one file at a time. What it shows is the screen's own
 */
class Inspector {
  /** what the screen on show puts in the inspector, null on a screen without one */
  content = $state.raw<Snippet | null>(null);
  /** whether the screen has a file to show */
  present = $state.raw<() => boolean>(() => false);
  /** gives DOM focus to the screen's list */
  focusList = $state.raw<() => void>(() => {});
  /** the docked pane's element */
  #pane: HTMLElement | null = null;
  /** whether a phone's sheet is up, which is not remembered: a screen never opens under one */
  sheet = $state(false);

  get open() {
    return !!this.content && (viewport.phone ? this.sheet : inspectorPrefs.open);
  }

  show() {
    if (viewport.phone) this.sheet = true;
    else inspectorPrefs.open = true;
  }

  #hide() {
    if (viewport.phone) this.sheet = false;
    else inspectorPrefs.open = false;
  }

  /**
   * Hides the inspector from a control that leaves with it, its own Close or the breadcrumb, so
   * focus goes to the list first. The phone's sheet is a dialog, which gives focus back itself
   */
  close() {
    if (!viewport.phone) this.focusList();
    this.#hide();
  }

  /** For `{@attach}` on the docked pane, which takes focus the list has no line to take */
  attachPane = (node: HTMLElement) => {
    this.#pane = node;
    return () => {
      if (this.#pane === node) this.#pane = null;
    };
  };

  /**
   * Gives focus that fell to the page, with what the pane showed, to the list, or to the pane
   * when the list has nothing left to take it
   */
  recover() {
    // a phone's sheet is a dialog, which keeps focus inside itself
    if (viewport.phone || document.activeElement !== document.body) return;
    this.focusList();
    if (document.activeElement === document.body) this.#pane?.focus({ preventScroll: true });
  }

  /** Shows or hides the inspector from a control that stays, which keeps focus */
  toggle() {
    if (this.open) this.#hide();
    else this.show();
  }
}

export const inspector = new Inspector();

/**
 * Puts `content` in the inspector for as long as the calling component is mounted; `present` says
 * whether it has a file to show, and `focus` gives the screen's list focus when the pane closes
 * under it. Call it while the component is being set up
 */
export const useInspector = (content: Snippet, present: () => boolean, focus: () => void) => {
  $effect(() => {
    inspector.content = content;
    inspector.present = present;
    inspector.focusList = focus;
    return () => {
      if (inspector.content !== content) return;
      inspector.content = null;
      inspector.focusList = () => {};
      inspector.sheet = false;
    };
  });
};
