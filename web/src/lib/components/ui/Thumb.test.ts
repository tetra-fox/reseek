import { describe, expect, it } from "vitest";

import { slides } from "./Thumb.svelte";

const at = (x: number, y: number) => ({ x, y, width: 40, height: 20 });

describe("slides", () => {
  it("slides along a line, and is put in place as it first shows, on another line or on a resize", () => {
    expect(slides(at(0, 0), at(48, 0), true)).toBe(true);
    expect(slides(undefined, at(48, 0), true)).toBe(false);
    expect(slides(at(96, 0), at(0, 24), true)).toBe(false);
    expect(slides(at(0, 0), at(52, 0), false)).toBe(false);
  });
});
