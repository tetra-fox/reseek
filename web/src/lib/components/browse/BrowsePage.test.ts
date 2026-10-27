import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { browser } from "#lib/browse/browse.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import BrowsePage from "./BrowsePage.svelte";

const flush = async () => {
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
};

const progress = (percentComplete: number): Schemas["BrowseProgress"] => ({
  username: "alice",
  percentComplete,
  bytesTransferred: percentComplete,
  bytesRemaining: 100 - percentComplete,
  size: 100
});

describe("browse progress", () => {
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    if (component) unmount(component);
    live.stop();
    browser.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("updates the same line as frames arrive, then shows the tree", async () => {
    let answer!: (listing: Schemas["BrowseResponse"]) => void;
    const listing = new Promise<Schemas["BrowseResponse"]>((resolve) => (answer = resolve));
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/users/alice/browse")) return listing;
      return undefined;
    });
    live.start();
    net.frame("hello", { principal: { name: "a", role: "ReadOnly" } } as Schemas["Hello"]);
    component = mount(BrowsePage, {
      target: document.body,
      props: { user: "alice", path: null, flat: false }
    });
    await flush();
    const line = () => document.querySelector("[role=status] p")!;
    expect(line().textContent).toContain("Downloaded 0% of Response");
    const before = line();

    net.frame("users.browse_progress", progress(40));
    await flush();
    expect(line()).toBe(before);
    expect(line().textContent).toContain("Downloaded 40% of Response");
    // another user's browse doesn't move this one
    net.frame("users.browse_progress", { ...progress(90), username: "bob" });
    await flush();
    expect(line().textContent).toContain("Downloaded 40% of Response");

    answer({
      directories: [
        { name: "Music", fileCount: 0, files: [] },
        { name: "Music\\Album", fileCount: 0, files: [] }
      ],
      directoryCount: 2,
      lockedDirectories: [],
      lockedDirectoryCount: 0
    });
    await flush();
    expect(document.querySelectorAll("[role=treeitem]")).toHaveLength(2);
  });
});
