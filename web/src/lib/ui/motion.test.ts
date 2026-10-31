import { flushSync } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { setMedia } from "#lib/testing/media.js";

import { chartMotion, crossFade, easeOutSoft, motionFast } from "./motion";

const REDUCE = "prefers-reduced-motion: reduce";

describe("motion", () => {
  afterEach(() => {
    setMedia(REDUCE, false);
    flushSync();
    Reflect.deleteProperty(document, "startViewTransition");
  });

  it("takes no time when reduced motion is asked for", () => {
    document.documentElement.style.setProperty("--motion-fast", "150ms");
    expect(motionFast()).toBe(150);
    // as the built stylesheet writes it
    document.documentElement.style.setProperty("--motion-fast", ".15s");
    expect(motionFast()).toBe(150);
    setMedia(REDUCE, true);
    flushSync();
    expect(motionFast()).toBe(0);
  });

  it("still charts when reduced motion is asked for", () => {
    expect(chartMotion()).toMatchObject({ type: "tween" });
    setMedia(REDUCE, true);
    flushSync();
    expect(chartMotion()).toBe("none");
  });

  it("cross-fades a change only where motion is welcome", async () => {
    const start = vi.fn((update: () => Promise<void>) => ({ updateCallbackDone: update() }));
    Object.assign(document, { startViewTransition: start });
    const update = vi.fn();
    await crossFade(update);
    expect(start).toHaveBeenCalledOnce();
    expect(update).toHaveBeenCalledOnce();

    setMedia(REDUCE, true);
    flushSync();
    await crossFade(update);
    expect(start).toHaveBeenCalledOnce();
    expect(update).toHaveBeenCalledTimes(2);
  });

  it("eases out along the soft curve, from rest to rest", () => {
    expect([easeOutSoft(0), easeOutSoft(1)]).toEqual([0, 1]);
    // cubic-bezier(0.2, 0, 0, 1) at a quarter, half and three quarters of the time
    expect(easeOutSoft(0.25)).toBeCloseTo(0.6072, 3);
    expect(easeOutSoft(0.5)).toBeCloseTo(0.8778, 3);
    expect(easeOutSoft(0.75)).toBeCloseTo(0.9755, 3);
  });
});
