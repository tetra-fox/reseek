import { cubicInOut } from "svelte/easing";

import { motionSlow } from "#lib/motion.js";

import type { Scale } from "./analysis";
import { type Curve, mix, pure } from "./frequency";

/** The frequency axis on its way from one curve to a scale, over `duration` ms from `start` */
export type Move = { from: Curve; to: Scale; start: number; duration: number };

/** Where `move` has the axis at `now`, eased in and out, the target scale once it is over */
export const curveAt = (move: Move, now: number): Curve => {
  const t = move.duration > 0 ? (now - move.start) / move.duration : 1;
  if (t >= 1) return pure(move.to);
  return mix(move.from, pure(move.to), cubicInOut(Math.max(0, t)));
};

/** A move to `scale` from `now`, starting where `move` has the axis then */
export const moveTo = (move: Move, scale: Scale, now: number, duration: number): Move => ({
  from: curveAt(move, now),
  to: scale,
  start: now,
  duration
});

/**
 * The frequency axis a spectrogram is drawn on, which moves from one scale to the next over the
 * slow motion token, or at once when reduced motion is asked for. A canvas reads the axis for the
 * time of the frame it draws, so it moves on every frame it draws; `curve` follows it a frame at
 * a time for what is drawn as page elements
 */
export class ScaleMotion {
  /** the scale the axis is on or moving to */
  target: Scale;
  /** the axis now, a frame at a time */
  curve: Curve;
  #move: Move;
  #frame = 0;

  constructor(scale: Scale) {
    this.#move = { from: pure(scale), to: scale, start: 0, duration: 0 };
    this.target = $state.raw(scale);
    this.curve = $state.raw(pure(scale));
  }

  /** Moves the axis to `scale`, from wherever it is now */
  to(scale: Scale) {
    if (scale === this.#move.to) return;
    const now = performance.now();
    this.#move = moveTo(this.#move, scale, now, motionSlow());
    this.target = scale;
    cancelAnimationFrame(this.#frame);
    this.#tick(now);
  }

  /** The axis at `now`, a frame's time */
  at(now: number): Curve {
    return curveAt(this.#move, now);
  }

  /** Whether the axis is still moving at `now` */
  moving(now: number) {
    return now < this.#move.start + this.#move.duration;
  }

  #tick = (now: number) => {
    this.curve = this.at(now);
    this.#frame = this.moving(now) ? requestAnimationFrame(this.#tick) : 0;
  };

  /** Stops moving, for a view that is going away */
  stop() {
    cancelAnimationFrame(this.#frame);
  }
}
