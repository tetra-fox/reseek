import { describe, expect, it } from "vitest";

import type { Layout, Report, Target } from "#lib/analysis/analysis.js";

import { Scope } from "./scope.svelte";

const target: Target = { kind: "file", root: "downloads", path: "a.flac" };

/** A scope over a file `duration` seconds long, whose moves land at once */
const scope = (duration: number) => {
  const layout = { durationSeconds: duration, tiles: 1 } as Layout;
  return new Scope(target, { windows: [] } as unknown as Report, layout);
};

describe("two linked scopes", () => {
  it("zoom and scroll together from either side", () => {
    const [a, b] = [scope(100), scope(100)];
    a.link(b);
    a.show({ start: 20, span: 10 });
    expect(b.view).toEqual({ start: 20, span: 10 });
    b.pan(5);
    expect(a.view).toEqual({ start: 25, span: 10 });
    b.zoom(2, 0, false);
    expect(a.view).toEqual({ start: 25, span: 5 });
  });

  it("start from what the first shows and stay inside a shorter file", () => {
    const [a, b] = [scope(100), scope(50)];
    a.show({ start: 80, span: 10 });
    a.link(b);
    expect(b.view).toEqual({ start: 40, span: 10 });
    expect(a.view).toEqual({ start: 80, span: 10 });
    b.show({ start: 0, span: 5 });
    expect(a.view).toEqual({ start: 0, span: 5 });
    a.show({ start: 0, span: 100 });
    expect(b.view).toEqual({ start: 0, span: 50 });
  });

  it("move apart once unlinked", () => {
    const [a, b] = [scope(100), scope(100)];
    a.link(b);
    a.show({ start: 10, span: 10 });
    b.unlink();
    a.show({ start: 50, span: 10 });
    expect(b.view).toEqual({ start: 10, span: 10 });
    b.show({ start: 70, span: 10 });
    expect(a.view).toEqual({ start: 50, span: 10 });
  });
});
