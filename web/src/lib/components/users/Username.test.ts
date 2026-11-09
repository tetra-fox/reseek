import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { interaction } from "#lib/lists/interaction.svelte.js";
import { userSummary } from "#lib/testing/fixtures.js";
import { CLOSE_DELAY_MS, OPEN_DELAY_MS } from "#lib/users/hover.svelte.js";

import Username from "./Username.svelte";

const summary = userSummary("alice", {
  groups: [{ name: "friends", kind: "manual", defined: true, uploadSettings: true, color: "blue" }],
  uploads: { queue: "friends", slots: 2, speed: "default", speedLimit: null, limits: [] },
  description: "plays bass\nlikes dub",
  notes: "owes me a record",
  info: {
    fetchedAt: "2026-10-26T10:00:00Z",
    hasFreeUploadSlot: true,
    hasPicture: true,
    pictureUrl: "/api/v0/users/alice/picture?v=ab",
    queueLength: 0,
    stale: false,
    uploadSlots: 1
  },
  figures: {
    downloads: { succeeded: 1, failed: 0, active: 0, queued: 0 },
    uploads: { succeeded: 2, failed: 0, active: 0, queued: 0 },
    messages: { in: 0, out: 0, unread: 0 }
  }
});

const pointer = (type: string, pointerType: string) => {
  const event = new MouseEvent(type, {
    bubbles: type !== "pointerenter" && type !== "pointerleave"
  });
  Object.defineProperty(event, "pointerType", { value: pointerType });
  return event;
};

describe("a username", () => {
  let component: ReturnType<typeof mount> | null = null;
  let link: HTMLAnchorElement;
  const card = () => document.querySelector('[role="dialog"][aria-label="Profile of alice"]');

  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify(summary), { headers: { "Content-Type": "application/json" } })
      )
    );
    component = mount(Username, { target: document.body, props: { name: "alice" } });
    flushSync();
    link = document.querySelector("a")!;
  });
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("opens its card after the pointer rests on it, and closes it after it leaves", async () => {
    link.dispatchEvent(pointer("pointerenter", "mouse"));
    await vi.advanceTimersByTimeAsync(OPEN_DELAY_MS - 1);
    flushSync();
    expect(card()).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    flushSync();
    expect(card()).not.toBeNull();
    expect(interaction.frozen).toBe(true);
    expect(card()!.textContent).toContain("2 uploads");
    // a preview of the profile: the picture, the text's first lines, the note and the group
    expect(card()!.querySelector("img")?.getAttribute("src")).toMatch(
      /\/api\/v0\/users\/alice\/picture\?v=ab$/
    );
    expect(card()!.textContent).toContain("plays bass\nlikes dub");
    expect(card()!.textContent).toContain("owes me a record");
    // the face's initial has a background color of its own, so the dot is found in the badge
    const badge = [...card()!.querySelectorAll<HTMLElement>("span")].find(
      (s) => s.textContent.trim() === "friends"
    );
    expect(badge?.querySelector("span[aria-hidden]")).not.toBeNull();

    link.dispatchEvent(pointer("pointerleave", "mouse"));
    await vi.advanceTimersByTimeAsync(CLOSE_DELAY_MS);
    flushSync();
    expect(card()).toBeNull();
    expect(interaction.frozen).toBe(false);
  });

  it("doesn't open for a pointer that passes over, or on touch", async () => {
    link.dispatchEvent(pointer("pointerenter", "mouse"));
    await vi.advanceTimersByTimeAsync(OPEN_DELAY_MS / 2);
    link.dispatchEvent(pointer("pointerleave", "mouse"));
    await vi.advanceTimersByTimeAsync(OPEN_DELAY_MS);
    link.dispatchEvent(pointer("pointerenter", "touch"));
    await vi.advanceTimersByTimeAsync(OPEN_DELAY_MS * 2);
    flushSync();
    expect(card()).toBeNull();
  });

  it("opens from the keyboard only on Space or Alt+Down, never on focus alone", async () => {
    link.focus();
    await vi.advanceTimersByTimeAsync(OPEN_DELAY_MS * 2);
    flushSync();
    expect(card()).toBeNull();
    link.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    flushSync();
    await vi.advanceTimersByTimeAsync(0);
    expect(card()).not.toBeNull();
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true })
    );
    flushSync();
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(card()).toBeNull();
    expect(document.activeElement).toBe(link);

    link.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", altKey: true, bubbles: true })
    );
    flushSync();
    expect(card()).not.toBeNull();
  });

  it("closes the card the user scrolls away from, and not on a scroll inside it", async () => {
    link.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    flushSync();
    await vi.advanceTimersByTimeAsync(0);
    card()!.dispatchEvent(new WheelEvent("wheel", { bubbles: true }));
    flushSync();
    expect(card()).not.toBeNull();
    document.body.dispatchEvent(new Event("scroll"));
    flushSync();
    expect(card()).not.toBeNull();
    document.body.dispatchEvent(new WheelEvent("wheel", { bubbles: true }));
    flushSync();
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(card()).toBeNull();
  });
});
