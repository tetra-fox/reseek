import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { browser } from "#lib/browse/browse.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { sharedFile } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { connected } from "#lib/testing/live.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { box } from "#lib/testing/state.svelte.js";

import BrowsePage from "./BrowsePage.svelte";

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
    session.principal = null;
    app.state = null;
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("updates the same line as frames arrive, then shows the tree", async () => {
    let answer!: (listing: Schemas["BrowseResponse"]) => void;
    const listing = new Promise<Schemas["BrowseResponse"]>((resolve) => (answer = resolve));
    const net = connected((url) =>
      url.pathname.endsWith("/users/alice/browse") ? listing : undefined
    );
    component = mount(BrowsePage, {
      target: document.body,
      props: { user: "alice", path: null }
    });
    await flushFaked();
    const line = () => document.querySelector("[role=status] p")!;
    expect(line().textContent).toContain("Loading shares... 0%");
    const before = line();

    net.frame("users.browse_progress", progress(40));
    await flushFaked();
    expect(line()).toBe(before);
    expect(line().textContent).toContain("Loading shares... 40%");
    // another user's browse doesn't move this one
    net.frame("users.browse_progress", { ...progress(90), username: "bob" });
    await flushFaked();
    expect(line().textContent).toContain("Loading shares... 40%");

    answer({
      directories: [
        { name: "Music", fileCount: 0, files: [] },
        { name: "Music\\Album", fileCount: 0, files: [] }
      ],
      directoryCount: 2,
      lockedDirectories: [],
      lockedDirectoryCount: 0
    });
    await flushFaked();
    // the first folder is open, a hint that folders open
    const rows = [...document.querySelectorAll("[role=treegrid] [role=rowgroup] [data-line]")];
    expect(rows.map((r) => r.getAttribute("data-line"))).toEqual([
      "folder:Music",
      "folder:Music\\Album"
    ]);
  });

  it("ticks a folder over the files under it, not those of a locked folder", async () => {
    fakeNetwork((url) =>
      url.pathname.endsWith("/users/alice/browse")
        ? {
            directories: [
              { name: "Music", fileCount: 1, files: [sharedFile("1.flac")] },
              { name: "Music\\CD2", fileCount: 1, files: [sharedFile("2.flac")] },
              { name: "Video", fileCount: 1, files: [sharedFile("x.mkv")] }
            ],
            directoryCount: 3,
            lockedDirectories: [
              { name: "Music\\Locked", fileCount: 1, files: [sharedFile("3.flac")] }
            ],
            lockedDirectoryCount: 1
          }
        : undefined
    );
    session.principal = { name: "me", role: "Administrator" } as never;
    app.state = { server: { isConnected: true }, user: { username: "me" } } as never;
    component = mount(BrowsePage, {
      target: document.body,
      props: { user: "alice", path: null }
    });
    await flushFaked();
    await flushFaked();
    const tick = (name: string) =>
      document.querySelector<HTMLElement>(
        `[role=checkbox][aria-label="Select every file in ${name}"]`
      )!;
    tick("Music").click();
    flushSync();
    expect([...browser.selected].toSorted()).toEqual(["Music\\1.flac", "Music\\CD2\\2.flac"]);
    tick("Video").click();
    flushSync();
    expect(document.querySelector("[aria-label='Selected files']")!.textContent).toContain(
      "3 files"
    );
  });

  it("leaves the next user's tree where it is when the one before answers late", async () => {
    let answerAlice!: (listing: Schemas["BrowseResponse"]) => void;
    const folders = (...names: string[]): Schemas["BrowseResponse"] => ({
      directories: names.map((name) => ({ name, fileCount: 0, files: [] })),
      directoryCount: names.length,
      lockedDirectories: [],
      lockedDirectoryCount: 0
    });
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/users/alice/browse")) {
        return new Promise((resolve) => (answerAlice = resolve));
      }
      if (url.pathname.endsWith("/users/bob/browse")) {
        return folders("Music", "Music\\Album", "Other", "Other\\Deep");
      }
      return undefined;
    });
    const address = box<{ user: string; path: string | null }>({
      user: "alice",
      path: "Other\\Deep"
    });
    component = mount(BrowsePage, {
      target: document.body,
      props: {
        get user() {
          return address.value.user;
        },
        get path() {
          return address.value.path;
        }
      }
    });
    await flushFaked();
    address.value = { user: "bob", path: null };
    await flushFaked();
    await flushFaked();
    const lines = () =>
      [...document.querySelectorAll("[role=treegrid] [role=rowgroup] [data-line]")].map((r) =>
        r.getAttribute("data-line")
      );
    expect(lines()).toEqual(["folder:Music", "folder:Music\\Album", "folder:Other"]);

    answerAlice(folders("Other", "Other\\Deep"));
    await flushFaked();
    await flushFaked();
    expect(browser.username).toBe("bob");
    expect(lines()).toEqual(["folder:Music", "folder:Music\\Album", "folder:Other"]);
  });
});
