import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { browser } from "./browse.svelte";

describe("the browse screen's state", () => {
  afterEach(() => {
    browser.clear();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("keeps a browse under way when the saved one is restored after it started", async () => {
    let answer!: (listing: Schemas["BrowseResponse"]) => void;
    const listing = new Promise<Schemas["BrowseResponse"]>((resolve) => (answer = resolve));
    fakeNetwork((url) => (url.pathname.endsWith("/users/alice/browse") ? listing : undefined));
    localStorage.setItem(
      "reseek-browse-state",
      JSON.stringify({ username: "bob", phase: "complete", separator: "\\" })
    );

    const browsing = browser.browse("alice");
    await browser.restore();
    expect(browser.username).toBe("alice");
    expect(browser.phase).toBe("pending");

    answer({
      directories: [{ name: "Music", fileCount: 0, files: [] }],
      directoryCount: 1,
      lockedDirectories: [],
      lockedDirectoryCount: 0
    });
    await browsing;
    expect(browser.phase).toBe("complete");
    expect(browser.tree.roots.map((r) => r.name)).toEqual(["Music"]);
  });
});
