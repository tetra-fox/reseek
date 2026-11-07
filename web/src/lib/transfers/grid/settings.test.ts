import { afterEach, describe, expect, it, vi } from "vitest";

describe("a saved transfers view", () => {
  afterEach(() => localStorage.clear());

  const load = async (saved: unknown) => {
    localStorage.setItem("transfers.view.download", JSON.stringify(saved));
    vi.resetModules();
    return (await import("./settings.svelte")).views.download;
  };

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

  it("reads the grouping once saved as flat as the list layout", async () => {
    const view = await load({ groupBy: "flat" });
    expect([view.layout, view.groupBy]).toEqual(["list", "user"]);
    const kept = await load({ layout: "tree", groupBy: "folder" });
    expect([kept.layout, kept.groupBy]).toEqual(["tree", "folder"]);
  });
});
