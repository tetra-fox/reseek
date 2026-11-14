import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { interaction } from "#lib/lists/interaction.svelte.js";
import { userSummary } from "#lib/testing/fixtures.js";
import { CLOSE_DELAY_MS, OPEN_DELAY_MS } from "#lib/users/hover.svelte.js";

import UserCard from "./UserCard.svelte";
import UserEntry from "./UserEntry.svelte";
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

describe("a name's look", () => {
  const name = " dj shadow ";
  afterEach(() => (document.body.innerHTML = ""));

  /** A box of its own under the body */
  const place = () => document.body.appendChild(document.createElement("div"));

  /** The element a name's text is drawn in, under `root` */
  const drawn = (root: ParentNode) =>
    [...root.querySelectorAll<HTMLElement>("[dir=auto]")].find((e) =>
      e.textContent.includes("dj shadow")
    );

  it("is the same in a card, a menu and a picker's entry as anywhere else", async () => {
    const [plainBox, cardBox, entryBox, linkBox] = [place(), place(), place(), place()];
    const mounted = [
      mount(Username, { target: plainBox, props: { name, plain: true } }),
      mount(UserCard, { target: cardBox, props: { name, user: undefined, onblock: () => {} } }),
      mount(UserEntry, { target: entryBox, props: { user: { name }, text: "dj" } }),
      mount(Username, { target: linkBox, props: { name } })
    ];
    flushSync();
    linkBox
      .querySelector("a")!
      .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    await vi.waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull());

    const look = drawn(plainBox)!;
    // the spaces at its ends show as dots
    expect(look.textContent).toBe("·dj shadow·");
    for (const shown of [cardBox, document.querySelector('[role="menu"]')!, entryBox]) {
      const text = drawn(shown);
      expect(text?.getAttribute("style")).toBe(look.getAttribute("style"));
      expect(text?.classList.contains("font-medium")).toBe(true);
      expect(text?.classList.contains("font-mono")).toBe(false);
    }
    expect(drawn(cardBox)!.textContent).toBe("·dj shadow·");
    // a picker's entry marks what was typed in the name
    expect(entryBox.querySelector("mark")?.textContent).toBe("dj");
    for (const component of mounted) unmount(component);
  });
});
