import { describe, expect, it } from "vitest";

import {
  acrossOf,
  follow,
  formatTime,
  panBy,
  timeAt,
  timeTicks,
  whole,
  windowAt,
  zoomAround
} from "./timeview";

describe("zoom and pan", () => {
  it("zooms around the point under the pointer and keeps its time there", () => {
    const view = zoomAround(whole(300), 4, 0.25, 300);
    expect(view).toEqual({ start: 56.25, span: 75 });
    expect(timeAt(view, 0.25)).toBe(75);
    // and out again to the whole file
    expect(zoomAround(view, 0.25, 0.25, 300)).toEqual({ start: 0, span: 300 });
  });

  it("stays inside the file and no shorter than two seconds", () => {
    expect(zoomAround(whole(300), 1000, 1, 300)).toEqual({ start: 298, span: 2 });
    expect(panBy({ start: 10, span: 20 }, -50, 300)).toEqual({ start: 0, span: 20 });
    expect(panBy({ start: 10, span: 20 }, 500, 300)).toEqual({ start: 280, span: 20 });
    // a file shorter than two seconds shows whole
    expect(zoomAround(whole(1.5), 8, 0.5, 1.5)).toEqual({ start: 0, span: 1.5 });
  });

  it("maps a click to its time at any zoom", () => {
    for (const [view, across, t] of [
      [whole(300), 0.5, 150],
      [{ start: 100, span: 50 }, 0.5, 125],
      [{ start: 100, span: 50 }, 0, 100],
      [{ start: 297, span: 3 }, 0.9, 299.7]
    ] as const) {
      expect(timeAt(view, across)).toBeCloseTo(t, 9);
      expect(acrossOf(view, t)).toBeCloseTo(across, 9);
    }
  });

  it("pages on to keep a playhead in sight", () => {
    const view = { start: 0, span: 20 };
    expect(follow(view, 17, 300)).toBe(view);
    expect(follow(view, 18.5, 300)).toEqual({ start: 16.5, span: 20 });
    expect(follow({ start: 100, span: 20 }, 40, 300)).toEqual({ start: 38, span: 20 });
    expect(follow({ start: 270, span: 20 }, 299, 300)).toEqual({ start: 280, span: 20 });
  });
});

describe("windowAt", () => {
  it("finds the second that holds a time", () => {
    const every = Array.from({ length: 40 }, (_, t) => t);
    expect(windowAt(every, 0)).toBe(0);
    expect(windowAt(every, 39.99)).toBe(39);
    expect(windowAt(every, 20.5)).toBe(20);
  });

  it("takes the nearest sample where a file was read in samples", () => {
    const samples = [0, 100, 200, 300];
    expect(windowAt(samples, 104)).toBe(1);
    expect(windowAt(samples, 160)).toBe(2);
    expect(windowAt(samples, 399)).toBe(3);
    expect(windowAt([], 3)).toBe(-1);
  });
});

describe("the time axis", () => {
  it("labels a step that leaves room between labels", () => {
    expect(timeTicks({ start: 0, span: 300 }, 0.15)).toEqual([0, 60, 120, 180, 240, 300]);
    expect(timeTicks({ start: 12.3, span: 3 }, 0.15)).toEqual([12.5, 13, 13.5, 14, 14.5, 15]);
  });

  it("writes times as minutes and seconds", () => {
    expect(formatTime(65)).toBe("1:05");
    expect(formatTime(65.36, true)).toBe("1:05.3");
    expect(formatTime(3725)).toBe("1:02:05");
  });
});
