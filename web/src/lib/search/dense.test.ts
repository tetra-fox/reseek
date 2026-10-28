import { describe, expect, it } from "vitest";

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
