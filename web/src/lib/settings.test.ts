import { afterEach, describe, expect, it, vi } from "vitest";

describe("the saved search options", () => {
  afterEach(() => localStorage.clear());

  const load = async (saved: unknown) => {
    localStorage.setItem("reseek.search.options", JSON.stringify(saved));
    vi.resetModules();
    return (await import("./settings.svelte")).searchOptions;
  };

  it("read the view once saved as cards or dense as the tree or the list layout", async () => {
    expect((await load({ view: "dense" })).layout).toBe("list");
    expect((await load({ view: "cards" })).layout).toBe("tree");
    expect((await load({ layout: "list", view: "cards" })).layout).toBe("list");
  });
});
