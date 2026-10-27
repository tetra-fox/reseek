import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeNetwork } from "#lib/testing/network.js";
import { toasts } from "#lib/toast.svelte.js";

import { searches } from "./searches.svelte";

describe("the searches store", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    searches.clear();
    toasts.splice(0);
  });

  it("says once per outage that a reload failed, and never for the first load", async () => {
    let up = false;
    fakeNetwork(() => (up ? [] : { status: 500, body: { error: "database is locked" } }));
    await searches.load();
    expect(toasts).toHaveLength(0);
    up = true;
    await searches.load();
    up = false;
    await searches.load();
    await searches.load();
    expect(toasts.map((t) => t.text)).toEqual(["database is locked"]);
  });
});
