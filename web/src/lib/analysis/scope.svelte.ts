import { SvelteSet } from "svelte/reactivity";

import { motionSlow } from "#lib/motion.js";

import type { Layout, Report, Target } from "./analysis";
import { Pictures } from "./pictures.svelte";
import {
  centerOn,
  clamp,
  follow,
  panBy,
  timeAt,
  type View,
  whole,
  windowAt,
  zoomAround
} from "./timeview";

/**
 * An eased change of zoom: the view it is headed to, the point `across` the view that stays in
 * place, how quickly it closes in, and when it last moved
 */
type Zooming = { to: View; across: number; tau: number; last: number };

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
  #zooming: Zooming | null = null;
  #frame = 0;

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
    this.#stop();
    this.view = clamp(view, this.duration);
  }

  /** Shows the whole file */
  showAll() {
    this.#ease(whole(this.duration), 0.5);
  }

  /**
   * Zooms by `factor` around the point `across` the view (0 at its left, 1 at its right), eased
   * unless `eased` is false, as for a pinch that follows the fingers. Zooms in quick succession
   * add up from where the last one is headed
   */
  zoom(factor: number, across: number, eased = true) {
    const base = eased ? (this.#zooming?.to ?? this.view) : this.view;
    const to = zoomAround(base, factor, across, this.duration);
    // zooming onto a stretch the playhead isn't in is the user looking elsewhere, so stop
    // following rather than page the view away from what they zoomed to
    if (this.following && (this.cursor < to.start || this.cursor > to.start + to.span)) {
      this.following = false;
    }
    if (!eased) {
      this.#stop();
      this.view = to;
      return;
    }
    this.#ease(to, across);
  }

  /**
   * Turns following the playhead on or off; turned on with the playhead out of sight, the view
   * moves to it
   */
  setFollowing(on: boolean) {
    this.following = on;
    if (!on) return;
    const v = this.#zooming?.to ?? this.view;
    if (this.cursor < v.start || this.cursor > v.start + v.span) {
      this.#ease(centerOn(v, this.cursor, this.duration), 0.5);
    }
  }

  /** Zooms by `factor` around the cursor, or the middle when the cursor is out of sight */
  zoomAtCursor(factor: number) {
    const across = (this.cursor - this.view.start) / this.view.span;
    this.zoom(factor, across >= 0 && across <= 1 ? across : 0.5);
  }

  /**
   * Moves the view toward `to`, closing a fixed share of the distance each moment (eased out,
   * settled within about the slow motion token): the span changes by equal ratios and the time
   * `across` the view moves to where `to` has it. A new target while moving carries on from
   * where the view is, so quick zooms in a row neither restart nor fall behind
   */
  #ease(to: View, across: number) {
    const duration = motionSlow();
    if (duration <= 0 || this.view.span <= 0) {
      this.#stop();
      this.view = to;
      return;
    }
    const last = this.#zooming?.last ?? performance.now();
    this.#zooming = { to, across, tau: duration / 4, last };
    if (!this.#frame) this.#frame = requestAnimationFrame(this.#tick);
  }

  #tick = (now: number) => {
    this.#frame = 0;
    const z = this.#zooming;
    if (!z) return;
    const k = 1 - Math.exp(-Math.max(0, now - z.last) / z.tau);
    z.last = now;
    const v = this.view;
    const span = v.span * (z.to.span / v.span) ** k;
    const at = timeAt(v, z.across);
    const anchor = at + (timeAt(z.to, z.across) - at) * k;
    const settled =
      Math.abs(Math.log(z.to.span / span)) < 1e-3 && Math.abs(z.to.start - v.start) < span * 1e-3;
    if (settled) {
      this.#zooming = null;
      this.view = z.to;
      return;
    }
    this.view = clamp({ start: anchor - z.across * span, span }, this.duration);
    this.#frame = requestAnimationFrame(this.#tick);
  };

  /** Ends an eased zoom where it is, for a change the user makes directly */
  #stop() {
    this.#zooming = null;
    if (this.#frame) cancelAnimationFrame(this.#frame);
    this.#frame = 0;
  }

  /** Moves the view by `seconds`; the user moved it, so it stops following the cursor */
  pan(seconds: number) {
    this.#stop();
    this.view = panBy(this.view, seconds, this.duration);
    this.following = false;
  }

  /** Centers the view on `t`; the user moved it, so it stops following the cursor */
  centerOn(t: number) {
    this.#stop();
    this.view = centerOn(this.view, t, this.duration);
    this.following = false;
  }

  /** Puts the cursor at `t` and the view follows it again */
  place(t: number) {
    this.cursor = Math.min(this.duration, Math.max(0, t));
    this.following = true;
    // a cursor sent out of sight (Home, End, a step past the edge) slides the view to it
    const base = this.#zooming?.to ?? this.view;
    const next = follow(base, this.cursor, this.duration);
    if (next !== base) this.#ease(next, 0.5);
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
    // an eased zoom in flight owns the view until it lands
    if (this.following && !this.#zooming) this.view = follow(this.view, this.cursor, this.duration);
  }
}
