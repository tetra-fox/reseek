import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { userSummary } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import BuddyList from "./BuddyList.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

describe("the buddy list", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("places each drawn row among all the buddies, the header first", async () => {
    const buddy = { buddy: { notifyPresence: false } };
    fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/users")
        ? { users: [userSummary("ann", buddy), userSummary("bob", buddy)], total: 2 }
        : undefined
    );
    component = mount(BuddyList, { target: document.body });
    await flush();
    const grid = document.querySelector('[role="grid"]')!;
    expect(grid.getAttribute("aria-rowcount")).toBe("3");
    const rows = [...grid.querySelectorAll('[role="row"]')];
    expect(rows.map((r) => r.getAttribute("aria-rowindex"))).toEqual(["1", "2", "3"]);
  });
});
