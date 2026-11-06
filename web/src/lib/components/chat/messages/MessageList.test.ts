import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { layout } from "#lib/chat/messages/messages.js";
import { MessagePane, PAGE } from "#lib/chat/pane.svelte.js";
import { dismiss, toasts } from "#lib/feedback/toast.svelte.js";
import { interaction } from "#lib/lists/interaction.svelte.js";
import { ListWindow } from "#lib/lists/virtual.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { press } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import MessageList from "./MessageList.svelte";

/** alice's message `localId` as the daemon stores it */
const stored = (localId: number) => ({
  localId,
  id: localId,
  timestamp: new Date(Date.UTC(2026, 9, 27, 0, 0, localId)).toISOString(),
  username: "alice",
  direction: "In",
  message: `line ${localId}`,
  isAcknowledged: true,
  wasReplayed: false,
  autoReply: false
});

/** a conversation with alice of `total` messages, read a page at a time from the newest */
const history = (total: number) => {
  const pages: (number | null)[] = [];
  const net = fakeNetwork((url) => {
    if (!url.pathname.endsWith("/messages")) return undefined;
    const before = Number(url.searchParams.get("before") ?? total + 1);
    pages.push(url.searchParams.has("before") ? before : null);
    const first = Math.max(1, before - PAGE);
    return Array.from({ length: before - first }, (_, n) => stored(before - 1 - n));
  });
  return { net, pages };
};

describe("the message list", () => {
  /**
   * the lists mounted, each in a box of its own, so a test that runs past its time can't reach
   * the next one's
   */
  const mounted: ReturnType<typeof mount>[] = [];
  afterEach(() => {
    for (const component of mounted.splice(0)) unmount(component);
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    while (toasts.length) dismiss(toasts[0].id);
    document.body.innerHTML = "";
  });

  const open = async (at: number | null = null) => {
    const pane = new MessagePane({ kind: "user", name: "alice" }, () => "me", at);
    await pane.load();
    const target = document.body.appendChild(document.createElement("div"));
    mounted.push(mount(MessageList, { target, props: { pane, watch: [], onread: () => {} } }));
    await flush();
    const box = target.querySelector<HTMLElement>('[role="log"]')!;
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
    // jsdom measures every line as 0 high, which would draw them all
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(24);
    const { pane, box } = await open();
    // a press on the scrollbar, beside the content
    box.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, clientX: 305 }));
    await pullToTop(box);
    expect(pane.hasMore).toBe(false);
    expect(pages.filter((p) => p !== null)).toEqual([151, 51]);
  });

  it("opens where a message no longer stored was, and says so once", async () => {
    // messages 1 to 300 with 150 gone, read newest first before an id or oldest first after one
    fakeNetwork((url) => {
      if (!url.pathname.endsWith("/messages")) return undefined;
      const ids = Array.from({ length: 300 }, (_, n) => n + 1).filter((id) => id !== 150);
      const after = url.searchParams.get("after");
      const before = Number(url.searchParams.get("before") ?? 301);
      const page =
        after === null
          ? ids
              .filter((id) => id < before)
              .slice(-PAGE)
              .toReversed()
          : ids.filter((id) => id > Number(after)).slice(0, PAGE);
      return page.map(stored);
    });
    // jsdom scrolls nothing, so where the list goes is read from what it was asked
    const to = vi.spyOn(ListWindow.prototype, "scrollTo").mockImplementation(() => {});
    const toEnd = vi.spyOn(ListWindow.prototype, "scrollToEnd").mockImplementation(() => {});
    // jsdom measures every line as 0 high, which would draw them all
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(24);
    const { pane } = await open(150);
    // older messages are stored, so the list starts with the lines `layout` gives
    const join = layout(pane.ordered, pane.gaps, pane.unreadFrom).findIndex(
      (l) => l.key === "m:151"
    );
    expect(to.mock.calls).toEqual([[join, "center"]]);
    expect(toEnd).not.toHaveBeenCalled();
    expect(toasts.map((t) => t.text)).toEqual(["That message is no longer stored"]);
  });

  it("brings the message the keys are on into view before opening its menu", async () => {
    history(250);
    // jsdom measures every line as 0 high, which would draw them all
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(24);
    const { box } = await open();
    const to = vi.spyOn(ListWindow.prototype, "scrollTo").mockImplementation(() => {});
    box.focus();
    press(box, "ArrowUp");
    await flush();
    const active = document.activeElement as HTMLElement;
    expect(active.dataset.message).toBeDefined();
    const at = Number(active.parentElement!.dataset.index);
    to.mockClear();

    press(active, "Enter");
    await flush();
    expect(to.mock.calls).toEqual([[at]]);
    expect(document.querySelector('[role="menu"]')).not.toBeNull();
    press(document.querySelector('[role="menuitem"]')!, "Escape");
    await flush();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(interaction.frozen).toBe(false);
  });

  it("walks past the messages of ours that are not sent", async () => {
    history(3);
    const { pane, box } = await open();
    await pane.send("lost");
    pane.messages.upsert({ ...pane.ordered[0], localId: -2, text: "going", sending: "sending" });
    await flush();
    expect(pane.ordered.map((m) => m.sending ?? m.localId)).toEqual([1, 2, 3, "failed", "sending"]);
    box.focus();
    press(box, "ArrowUp");
    await flush();
    expect((document.activeElement as HTMLElement).dataset.message).toBe("3");
    press(document.activeElement!, "ArrowDown");
    await flush();
    expect((document.activeElement as HTMLElement).dataset.message).toBe("3");
  });

  it("gives focus back to the log when the view is cleared under the message the keys are on", async () => {
    history(3);
    const { pane, box } = await open();
    box.focus();
    press(box, "ArrowUp");
    await flush();
    expect((document.activeElement as HTMLElement).dataset.message).toBe("3");
    pane.clearView();
    await flush();
    expect(document.querySelector("[data-message]")).toBeNull();
    expect(document.activeElement).toBe(box);
  });
});
