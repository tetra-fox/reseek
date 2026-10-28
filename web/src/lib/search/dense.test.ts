import { afterEach, describe, expect, it, vi } from "vitest";

import { columnWidth, WIDEST } from "#lib/format.js";

import { type DenseRow, nextSort, sortDense } from "./dense";

const row = (key: string, speed: number, size: number): DenseRow => ({
  key,
  username: key,
  country: null,
  speed,
  queue: 0,
  free: true,
  file: {
    filename: `${key}\\a.flac`,
    size,
    code: 1,
    extension: "flac",
    isLocked: false,
    locked: false,
    attributeCount: 0,
    attributes: []
  },
  folder: key
});

describe("the dense results' sort", () => {
  it("orders by a column, ties as they came, and keeps the order with none", () => {
    const rows = [row("a", 5, 3), row("b", 9, 1), row("c", 5, 2)];
    expect(sortDense(rows, { id: "speed", dir: "desc" }).map((r) => r.key)).toEqual([
      "b",
      "a",
      "c"
    ]);
    expect(sortDense(rows, { id: "size", dir: "asc" }).map((r) => r.key)).toEqual(["b", "c", "a"]);
    expect(sortDense(rows, null)).toBe(rows);
  });

  it("goes first way, the other way, then off on a header's clicks", () => {
    expect(nextSort(null, "size")).toEqual({ id: "size", dir: "asc" });
    expect(nextSort({ id: "size", dir: "asc" }, "size")).toEqual({ id: "size", dir: "desc" });
    expect(nextSort({ id: "size", dir: "desc" }, "size")).toBeNull();
    expect(nextSort({ id: "size", dir: "asc" }, "speed")).toEqual({ id: "speed", dir: "desc" });
  });
});

describe("the dense view's saved widths", () => {
  afterEach(() => localStorage.clear());

  // older builds saved every column's width, defaults included, under `widths`
  const OLD = {
    sort: { id: "size", dir: "desc" },
    widths: { user: 140, speed: 90, queue: 61, free: 72, file: 280, size: 84, length: 90 }
  };

  const load = async (saved: unknown) => {
    localStorage.setItem("reseek.search.dense", JSON.stringify(saved));
    vi.resetModules();
    return import("./dense");
  };

  it("drops widths an older build saved, so each column takes today's default", async () => {
    const { DENSE_COLUMNS, denseView, denseWidth } = await load(OLD);
    expect(denseView.sort).toEqual({ id: "size", dir: "desc" });
    expect(denseView.dragged).toEqual({});
    for (const c of DENSE_COLUMNS) expect(denseWidth(c)).toBe(c.width);
    expect(denseWidth(DENSE_COLUMNS.find((c) => c.id === "size")!)).toBe(columnWidth(WIDEST.size));
  });

  it("keeps a dragged width and leaves the rest to their defaults", async () => {
    const { DENSE_COLUMNS, denseWidth } = await load({ dragged: { file: 400, size: 12 } });
    const width = (id: string) => denseWidth(DENSE_COLUMNS.find((c) => c.id === id)!);
    expect(width("file")).toBe(400);
    // narrower than any column can be dragged, so not a drag
    expect(width("size")).toBe(columnWidth(WIDEST.size));
    expect(width("user")).toBe(DENSE_COLUMNS[0].width);
  });
});
