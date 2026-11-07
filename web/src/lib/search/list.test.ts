import { afterEach, describe, expect, it, vi } from "vitest";

describe("the results list's saved widths", () => {
  afterEach(() => localStorage.clear());

  const load = async (key: string, saved: unknown) => {
    localStorage.setItem(key, JSON.stringify(saved));
    vi.resetModules();
    return import("./list");
  };

  it("keeps a dragged width, but not one narrower than a drag can make", async () => {
    const { resultList } = await load("reseek.search.list", {
      dragged: { file: 400, size: 12, select: 90 }
    });
    expect(resultList.dragged).toEqual({ file: 400 });
  });

  it("takes what was kept under the dense view's key, once", async () => {
    const { resultList } = await load("reseek.search.dense", { dragged: { file: 400 } });
    expect(resultList.dragged).toEqual({ file: 400 });
    expect(localStorage.getItem("reseek.search.dense")).toBeNull();
  });
});
