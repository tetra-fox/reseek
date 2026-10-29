import { SvelteSet } from "svelte/reactivity";

import type { Layout, Report, Target } from "./analysis";
import { Pictures } from "./pictures.svelte";
import { centerOn, clamp, follow, panBy, type View, whole, windowAt, zoomAround } from "./timeview";

/**
 * What one report's viewer shows, shared by the drawer and the large view so both agree: the
 * stretch of time in sight, the cursor (the moment picked, which is also the playhead while the
 * file plays), the guide lines hidden, and the pictures loaded
 */
export class Scope {
  readonly report: Report;
  readonly layout: Layout;
  readonly pictures: Pictures;
  /** where each analyzed window starts, in seconds */
  readonly starts: number[];
  view = $state.raw<View>({ start: 0, span: 0 });
  /** the cursor's time, in seconds */
  cursor = $state(0);
  /** whether the view keeps the cursor in sight as it moves, until the user pans away */
  following = $state(true);
  /** whether the user is dragging the cursor, which playback leaves alone until they let go */
  dragging = $state(false);
  readonly hidden = new SvelteSet<string>();

  constructor(target: Target, report: Report, layout: Layout) {
    this.report = report;
    this.layout = layout;
    this.pictures = new Pictures(target, layout);
    this.starts = report.windows?.map((w) => w.t) ?? [];
    this.view = whole(layout.durationSeconds);
  }

  get duration() {
    return this.layout.durationSeconds;
  }

  /** The analyzed window the cursor is in, -1 for a report without windows */
  get window(): number {
    return windowAt(this.starts, this.cursor);
  }

  /** Shows `view`, kept inside the file */
  show(view: View) {
    this.view = clamp(view, this.duration);
  }

  /** Shows the whole file */
  showAll() {
    this.view = whole(this.duration);
  }

  /** Zooms by `factor` around the point `across` the view (0 at its left, 1 at its right) */
  zoom(factor: number, across: number) {
    this.view = zoomAround(this.view, factor, across, this.duration);
  }

  /** Zooms by `factor` around the cursor, or the middle when the cursor is out of sight */
  zoomAtCursor(factor: number) {
    const across = (this.cursor - this.view.start) / this.view.span;
    this.zoom(factor, across >= 0 && across <= 1 ? across : 0.5);
  }

  /** Moves the view by `seconds`; the user moved it, so it stops following the cursor */
  pan(seconds: number) {
    this.view = panBy(this.view, seconds, this.duration);
    this.following = false;
  }

  /** Centers the view on `t`; the user moved it, so it stops following the cursor */
  centerOn(t: number) {
    this.view = centerOn(this.view, t, this.duration);
    this.following = false;
  }

  /** Puts the cursor at `t` and the view follows it again */
  place(t: number) {
    this.cursor = Math.min(this.duration, Math.max(0, t));
    this.following = true;
    this.view = follow(this.view, this.cursor, this.duration);
  }

  /** Moves the cursor to `t` under the user's pointer, which keeps it in sight itself */
  drag(t: number) {
    this.cursor = Math.min(this.duration, Math.max(0, t));
    this.following = true;
  }

  /** Puts the cursor on the window `steps` windows from the one it is in */
  step(steps: number) {
    if (!this.starts.length) return;
    const at = Math.min(this.starts.length - 1, Math.max(0, this.window + steps));
    this.place(this.starts[at]);
  }

  /** Puts the cursor on the first window, or with `last` on the last one */
  toEnd(last: boolean) {
    if (!this.starts.length) return;
    this.place(this.starts[last ? this.starts.length - 1 : 0]);
  }

  /** The cursor moved with playback to `t`; the view keeps it in sight unless the user panned */
  played(t: number) {
    if (this.dragging) return;
    this.cursor = Math.min(this.duration, Math.max(0, t));
    if (this.following) this.view = follow(this.view, this.cursor, this.duration);
  }
}
