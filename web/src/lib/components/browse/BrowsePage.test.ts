import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { browser } from "#lib/browse/browse.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { flushFaked } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import BrowsePage from "./BrowsePage.svelte";

// clearing a browse opened from a link takes the user parameter out of the URL; there is no
// router in these tests
vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const file = (filename: string): Schemas["File"] => ({
  filename,
  size: 10,
  code: 1,
  extension: "",
  isLocked: false,
  attributeCount: 0,
  attributes: []
});

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
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/users/alice/browse")) return listing;
      return undefined;
    });
    live.start();
    net.frame("hello", { principal: { name: "a", role: "ReadOnly" } } as Schemas["Hello"]);
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

  it("leaves Escape to an open card, the filter field and a selection", async () => {
    fakeNetwork((url) =>
      url.pathname.endsWith("/users/alice/browse")
        ? {
            directories: [{ name: "Music", fileCount: 1, files: [file("1.flac")] }],
            directoryCount: 1,
            lockedDirectories: [],
            lockedDirectoryCount: 0
          }
        : undefined
    );
    component = mount(BrowsePage, {
      target: document.body,
      props: { user: "alice", path: null }
    });
    await flushFaked();
    await flushFaked();

    const region = document.querySelector("[role=region][aria-label=Browse]")!;
    const card = document.createElement("div");
    card.setAttribute("role", "dialog");
    document.body.append(card);
    region.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(browser.phase).toBe("complete");
    card.remove();
    // Escape in the filter field is the field's
    document
      .querySelector("input[aria-label=Filter]")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(browser.phase).toBe("complete");
    // a selection is cleared first
    browser.tick(["Music\\1.flac"], true);
    region.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(browser.phase).toBe("complete");
    browser.selected.clear();
    region.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(browser.phase).toBe("idle");
  });

  it("ticks a folder over the files under it and downloads one batch per top folder", async () => {
    const batches: unknown[] = [];
    fakeNetwork((url, _method, body) => {
      if (url.pathname.endsWith("/users/alice/browse")) {
        return {
          directories: [
            { name: "Music", fileCount: 1, files: [file("1.flac")] },
            { name: "Music\\CD2", fileCount: 1, files: [file("2.flac")] },
            { name: "Video", fileCount: 1, files: [file("x.mkv")] }
          ],
          directoryCount: 3,
          lockedDirectories: [{ name: "Music\\Locked", fileCount: 1, files: [file("3.flac")] }],
          lockedDirectoryCount: 1
        };
      }
      if (url.pathname.endsWith("/downloads/batches")) {
        batches.push(body);
        return { batchId: "b", failures: [] };
      }
      return undefined;
    });
    session.principal = { name: "me", role: "Administrator" } as never;
    app.state = { server: { isConnected: true } } as never;
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
    // Music's tick takes its own file and its subfolder's, not the locked folder's
    tick("Music").click();
    flushSync();
    expect([...browser.selected].toSorted()).toEqual(["Music\\1.flac", "Music\\CD2\\2.flac"]);
    tick("Video").click();
    flushSync();
    expect(document.querySelector("[aria-label='Selected files']")!.textContent).toContain(
      "3 files"
    );
    document.querySelector<HTMLElement>("[aria-label='Selected files'] button")!.click();
    await flushFaked();
    await flushFaked();
    expect(
      batches.map((b) => {
        const batch = b as { files: { filename: string }[]; options?: { destination: string } };
        return [batch.options?.destination ?? null, batch.files.map((f) => f.filename)];
      })
    ).toEqual([
      [null, ["Music\\1.flac"]],
      ["Music/CD2", ["Music\\CD2\\2.flac"]],
      [null, ["Video\\x.mkv"]]
    ]);
  });
});
