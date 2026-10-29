import { afterEach, describe, expect, it, vi } from "vitest";

describe("the dense view's saved widths", () => {
  afterEach(() => localStorage.clear());

  const load = async (saved: unknown) => {
    localStorage.setItem("reseek.search.dense", JSON.stringify(saved));
    vi.resetModules();
    return import("./dense");
  };

  it("keeps a dragged width, but not one narrower than a drag can make", async () => {
    const { denseView } = await load({ dragged: { file: 400, size: 12, select: 90 } });
    expect(denseView.dragged).toEqual({ file: 400 });
  });
});
