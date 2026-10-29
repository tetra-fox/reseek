import { flushSync } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import { setMedia } from "#lib/testing/media.js";

import { pure } from "./frequency";
import { curveAt, type Move, moveTo, ScaleMotion } from "./scalemotion.svelte";

const REDUCE = "prefers-reduced-motion: reduce";

describe("moving between scales", () => {
  afterEach(() => {
    setMedia(REDUCE, false);
    flushSync();
  });

  const still: Move = { from: pure("linear"), to: "linear", start: 0, duration: 0 };

  it("eases from one scale to the next and lands on it", () => {
    const move = moveTo(still, "log", 1000, 250);
    expect(curveAt(move, 1000)).toEqual(pure("linear"));
    // half way through the time is half way, as the easing is even about the middle
    expect(curveAt(move, 1125)).toEqual({ linear: 0.5, log: 0.5, mel: 0 });
    // eased, a tenth of the time is less than a tenth of the way
    expect(curveAt(move, 1025).log).toBeLessThan(0.1);
    expect(curveAt(move, 1250)).toEqual(pure("log"));
    expect(curveAt(move, 9000)).toEqual(pure("log"));
  });

  it("starts a change mid-move from where the axis is", () => {
    const first = moveTo(still, "log", 0, 250);
    const there = curveAt(first, 100);
    const second = moveTo(first, "mel", 100, 250);
    expect(curveAt(second, 100)).toEqual(there);
    // a little later the axis has moved on from there, not jumped
    const next = curveAt(second, 110);
    for (const scale of ["linear", "log", "mel"] as const) {
      expect(Math.abs(next[scale] - there[scale])).toBeLessThan(0.05);
    }
    expect(curveAt(second, 350)).toEqual(pure("mel"));
  });

  it("switches at once when reduced motion is asked for", () => {
    document.documentElement.style.setProperty("--motion-slow", "250ms");
    const moving = new ScaleMotion("linear");
    moving.to("mel");
    // the first frame is still the old scale
    expect(moving.curve).toEqual(pure("linear"));
    moving.stop();

    setMedia(REDUCE, true);
    flushSync();
    const motion = new ScaleMotion("linear");
    motion.to("mel");
    expect(motion.curve).toEqual(pure("mel"));
    motion.stop();
  });
});
