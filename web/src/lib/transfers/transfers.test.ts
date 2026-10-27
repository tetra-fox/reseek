import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeNetwork } from "#lib/testing/network.js";
import { toasts } from "#lib/toast.svelte.js";

import { Transfers } from "./transfers.svelte";

describe("the transfers store", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    toasts.splice(0);
  });

  it("says once per outage that a reload failed", async () => {
    let up = true;
    fakeNetwork(() => (up ? [] : { status: 500, body: { error: "database is locked" } }));
    const downloads = new Transfers("download");
    await downloads.load();
    up = false;
    await downloads.load();
    await downloads.load();
    expect(toasts.map((t) => t.text)).toEqual(["database is locked"]);
    up = true;
    await downloads.load();
    expect(downloads.error).toBe("");
    up = false;
    await downloads.load();
    expect(toasts).toHaveLength(2);
  });

  it("leaves a failed first load to the page", async () => {
    fakeNetwork(() => ({ status: 500, body: { error: "database is locked" } }));
    const uploads = new Transfers("upload");
    await uploads.load();
    expect(uploads.error).toBe("database is locked");
    expect(toasts).toHaveLength(0);
  });
});
