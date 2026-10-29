import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MessagePane, PAGE } from "#lib/chat/pane.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import MessageList from "./MessageList.svelte";

const flush = async () => {
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};

/** a conversation with alice of `total` messages, read a page at a time from the newest */
const history = (total: number) => {
  const pages: (number | null)[] = [];
  const net = fakeNetwork((url) => {
    if (!url.pathname.endsWith("/messages")) return undefined;
    const before = Number(url.searchParams.get("before") ?? total + 1);
    pages.push(url.searchParams.has("before") ? before : null);
    const first = Math.max(1, before - PAGE);
    return Array.from({ length: before - first }, (_, n) => {
      const localId = before - 1 - n;
      return {
        localId,
        id: localId,
        timestamp: new Date(Date.UTC(2026, 9, 27, 0, 0, localId)).toISOString(),
        username: "alice",
        direction: "In",
        message: `line ${localId}`,
        isAcknowledged: true,
        wasReplayed: false
      };
    });
  });
  return { net, pages };
};

describe("the message list", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const open = async () => {
    const pane = new MessagePane({ kind: "user", name: "alice" }, () => "me");
    await pane.load();
    component = mount(MessageList, {
      target: document.body,
      props: { pane, at: null, watch: [], onread: () => {} }
    });
    await flush();
    const box = document.querySelector<HTMLElement>('[role="log"]')!;
    // jsdom lays nothing out: the box is 300 px wide, its scrollbar past that
    Object.defineProperty(box, "clientWidth", { value: 300 });
    return { pane, box };
  };
  const pullToTop = async (box: HTMLElement) => {
    box.scrollTop = 0;
    box.dispatchEvent(new Event("scroll"));
    await flush();
  };

  it("reads page after page to the start while the scrollbar is held at the top", async () => {
    const { pages } = history(250);
    const { pane, box } = await open();
    // a press on the scrollbar, beside the content
    box.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, clientX: 305 }));
    await pullToTop(box);
    expect(pane.hasMore).toBe(false);
    expect(pages.filter((p) => p !== null)).toEqual([151, 51]);
  });
});
