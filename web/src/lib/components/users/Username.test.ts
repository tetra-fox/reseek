import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { interaction } from "#lib/interaction.svelte.js";

import { CLOSE_DELAY_MS, OPEN_DELAY_MS } from "./hover.svelte";
import Username from "./Username.svelte";

const summary = {
  username: "alice",
  known: true,
  isPrivileged: false,
  group: { name: "default", source: "default", priority: 1 },
  tags: [],
  muted: false,
  pinned: false,
  figures: {
    downloads: { succeeded: 1, failed: 0, active: 0, queued: 0 },
    uploads: { succeeded: 2, failed: 0, active: 0, queued: 0 },
    messages: { in: 0, out: 0, unread: 0 }
  },
  unreadMessages: 0,
  activeTransfers: 0
};

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

  it("is a link to the profile", () => {
    expect(link.getAttribute("href")).toBe("#/users/alice");
    expect(link.textContent).toBe("alice");
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
});
