import { afterEach, describe, expect, it, vi } from "vitest";

describe("a transfers view saved by an older build", () => {
  afterEach(() => localStorage.clear());

  // older builds saved every column's width, defaults included
  const OLD = {
    groupBy: "folder",
    density: "compact",
    columns: [
      { id: "file", width: 240, visible: true },
      { id: "select", width: 40, visible: true },
      { id: "speed", width: 130, visible: false },
      { id: "size", width: 84, visible: true }
    ],
    sort: { id: "speed", dir: "desc" },
    filters: { states: ["failed"], analysis: "fake" }
  };

  const load = async (saved: unknown) => {
    localStorage.setItem("transfers.view.download", JSON.stringify(saved));
    vi.resetModules();
    return (await import("./settings.svelte")).views.download;
  };

  it("keeps the order, shown columns and sort but no width", async () => {
    const view = await load(OLD);
    expect(view.groupBy).toBe("folder");
    expect(view.density).toBe("compact");
    expect(view.sort).toEqual({ id: "speed", dir: "desc" });
    expect(view.filters.analysis).toBe("fake");
    expect(view.columns.slice(0, 4)).toEqual([
      { id: "file", visible: true },
      { id: "select", visible: true },
      { id: "speed", visible: false },
      { id: "size", visible: true }
    ]);
    expect(view.columns.some((c) => "dragged" in c)).toBe(false);
  });

  it("drops an analysis filter it does not know", async () => {
    const view = await load({ filters: { analysis: "reading:consistent" } });
    expect(view.filters).toEqual({ states: [], analysis: null });
  });

  it("keeps a width the user dragged", async () => {
    const view = await load({ columns: [{ id: "file", visible: true, dragged: 420 }] });
    expect(view.columns.find((c) => c.id === "file")).toEqual({
      id: "file",
      visible: true,
      dragged: 420
    });
    expect(view.columns.find((c) => c.id === "size")).toEqual({ id: "size", visible: true });
  });
});
