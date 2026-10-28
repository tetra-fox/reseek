import { afterEach, describe, expect, it, vi } from "vitest";

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
    const { denseView } = await load(OLD);
    expect(denseView.sort).toEqual({ id: "size", dir: "desc" });
    expect(denseView.dragged).toEqual({});
  });

  it("keeps a dragged width, but not one narrower than a drag can make", async () => {
    const { denseView } = await load({ dragged: { file: 400, size: 12, select: 90 } });
    expect(denseView.dragged).toEqual({ file: 400 });
  });
});
