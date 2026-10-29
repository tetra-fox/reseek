import { afterEach, describe, expect, it, vi } from "vitest";

import { toasts } from "#lib/feedback/toast.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { searches } from "./searches.svelte";

describe("the searches store", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    searches.clear();
    toasts.splice(0);
  });

  it("leaves a failed first load to the page", async () => {
    fakeNetwork(() => ({ status: 500, body: { error: "database is locked" } }));
    await searches.load();
    expect(searches.error).toBe("database is locked");
    expect(toasts).toHaveLength(0);
  });
});
