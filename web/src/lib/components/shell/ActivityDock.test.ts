import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { activity, type ActivityEntry } from "#lib/activity/activity.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import ActivityDock from "./ActivityDock.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const entry = (id: number, username: string, kind: ActivityEntry["kind"]): ActivityEntry => ({
  id,
  at: `2026-10-27T18:00:${String(id).padStart(2, "0")}Z`,
  username,
  kind
});

describe("the activity dock", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    activity.entries.clear();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("puts new entries on top and grows one in place, keeping a selection of text", async () => {
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/activity")
        ? [
            entry(1, "alice", { type: "browse" }),
            entry(2, "bob", { type: "queued", files: 1, filename: "Music\\a.flac" })
          ]
        : undefined
    );
    live.start();
    component = mount(ActivityDock, { target: document.body });
    await flush();
    const rows = () => [...document.querySelectorAll("[data-line]")];
    expect(rows().map((r) => r.getAttribute("data-line"))).toEqual(["2", "1"]);
    const [bob, alice] = rows();
    const selection = document.getSelection()!;
    // a selection in a row that doesn't change
    selection.selectAllChildren(alice);
    const selected = selection.toString();

    // a new entry, and bob's grown by another file
    net.frame("activity.entry", entry(3, "carol", { type: "search", query: "boc", results: 4 }));
    net.frame(
      "activity.entry",
      entry(2, "bob", { type: "queued", files: 2, filename: "Music\\b.flac" })
    );
    await flush();
    expect(rows().map((r) => r.getAttribute("data-line"))).toEqual(["3", "2", "1"]);
    expect(rows()[1]).toBe(bob);
    expect(rows()[2]).toBe(alice);
    expect(bob.textContent).toContain("2 files");
    expect(document.getSelection()!.toString()).toBe(selected);
  });

  it("hides the kinds turned off, an upload's end with its start", async () => {
    fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/activity")
        ? [
            entry(1, "alice", { type: "browse" }),
            entry(2, "bob", {
              type: "uploadEnded",
              filename: "a.flac",
              state: "Completed, Succeeded",
              bytes: 9
            })
          ]
        : undefined
    );
    live.start();
    component = mount(ActivityDock, { target: document.body });
    await flush();
    [...document.querySelectorAll("button")]
      .find((b) => b.textContent?.trim() === "Uploads")!
      .click();
    flushSync();
    expect(
      [...document.querySelectorAll("[data-line]")].map((r) => r.getAttribute("data-line"))
    ).toEqual(["1"]);
    [...document.querySelectorAll("button")]
      .find((b) => b.textContent?.trim() === "Uploads")!
      .click();
  });
});
