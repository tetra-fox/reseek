import { flushSync } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { setMedia } from "#lib/testing/media.js";

import { crossFade, motionFast } from "./motion";

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
    setMedia(REDUCE, true);
    flushSync();
    expect(motionFast()).toBe(0);
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
});
