import { describe, expect, it } from "vitest";

import { column } from "./columns.svelte";

const LIMITS = { min: 15, width: 18, max: 28 };
const REM = 16;

describe("column", () => {
  it("shows its default width until one is dragged to", () => {
    expect(column(LIMITS, null, Infinity, REM)).toEqual({
      min: 240,
      max: 448,
      initial: 288,
      width: 288
    });
  });

  it("keeps a dragged width inside its limits", () => {
    expect(column(LIMITS, 300, Infinity, REM).width).toBe(300);
    expect(column(LIMITS, 100, Infinity, REM).width).toBe(240);
    expect(column(LIMITS, 900, Infinity, REM).width).toBe(448);
  });

  it("gives way to the room left, never under its least", () => {
    const narrow = column(LIMITS, 400, 260, REM);
    expect(narrow.width).toBe(260);
    expect(narrow.max).toBe(260);
    expect(narrow.initial).toBe(260);
    expect(column(LIMITS, 400, 100, REM)).toMatchObject({ width: 240, max: 240, initial: 240 });
  });

  it("follows the browser's text size", () => {
    expect(column(LIMITS, null, Infinity, 20)).toEqual({
      min: 300,
      max: 560,
      initial: 360,
      width: 360
    });
  });
});
