import { afterEach, describe, expect, it, vi } from "vitest";

describe("the results list's saved view", () => {
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

  it("hides only the columns that can be hidden", async () => {
    const { resultList } = await load("reseek.search.list", {
      hidden: ["folder", "size", "file", "nonsense", "queue"]
    });
    expect(resultList.hidden).toEqual(["queue", "folder"]);
  });
});
