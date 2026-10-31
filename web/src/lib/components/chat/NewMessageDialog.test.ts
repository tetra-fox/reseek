import { goto } from "$app/navigation";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { flushFaked } from "#lib/testing/flush.js";
import { press } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import NewMessageDialog from "./NewMessageDialog.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const summary = (username: string) => ({
  username,
  known: true,
  group: { name: "default", source: "builtin", priority: 1, color: null },
  tags: [],
  lastSeenAt: null,
  figures: {}
});

describe("the new message dialog", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.mocked(goto).mockClear();
    document.body.innerHTML = "";
  });

  /** The dialog with `text` typed in its name box, the directory answering with `known` */
  const typed = async (text: string, known: string[] = []) => {
    vi.useFakeTimers();
    const net = fakeNetwork((url, method) => {
      if (method === "POST") return {};
      if (url.pathname.endsWith("/api/v0/users")) {
        return { users: known.map((username) => summary(username)), total: known.length };
      }
      return undefined;
    });
    component = mount(NewMessageDialog, { target: document.body, props: { open: true } });
    flushSync();
    const box = document.querySelector<HTMLInputElement>("[role=combobox]")!;
    box.value = text;
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await flushFaked();
    await vi.advanceTimersByTimeAsync(250);
    await flushFaked();
    await tick();
    return { box, net };
  };
  const entries = () =>
    [...document.querySelectorAll<HTMLElement>("[role=option]")].map((e) => e.textContent.trim());
  const messageBox = () => document.querySelector<HTMLInputElement>("#new-message-text")!;
  const send = () => document.querySelector<HTMLButtonElement>("button[type=submit]")!;
  const writeMessage = () => {
    messageBox().value = "hello";
    messageBox().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  };

  it("offers the name as typed first, and Enter keeps it for the message box", async () => {
    const { box } = await typed("ali", ["alice"]);
    expect(entries()).toEqual(["Message ali", expect.stringContaining("alice")]);
    press(box, "Enter");
    await flushFaked();
    expect(box.value).toBe("ali");
    expect(document.activeElement).toBe(messageBox());
  });

  it("takes the name the keys moved to", async () => {
    const { box } = await typed("ali", ["alice"]);
    press(box, "ArrowDown");
    await flushFaked();
    press(box, "Enter");
    await flushFaked();
    expect(box.value).toBe("alice");
    expect(document.activeElement).toBe(messageBox());
  });

  it("lists a name the directory holds once, as itself", async () => {
    const { box } = await typed("alice", ["alice", "alice2"]);
    expect(entries()).toEqual([
      expect.stringContaining("alice"),
      expect.stringContaining("alice2")
    ]);
    press(box, "Enter");
    await flushFaked();
    expect(box.value).toBe("alice");
  });

  it("sends to the name as typed and opens the conversation", async () => {
    const { net } = await typed("ali");
    writeMessage();
    send().click();
    await flushFaked();
    const posted = net.requests.find((r) => r.method === "POST");
    expect(posted?.url.pathname).toMatch(/\/conversations\/ali$/);
    expect(posted?.body).toBe("hello");
    expect(goto).toHaveBeenCalledWith("#/chat/ali");
  });

  it("refuses server", async () => {
    const { box, net } = await typed("server", ["serverfan"]);
    expect(entries()).toEqual([]);
    expect(document.body.textContent).toContain("server is not a user");
    writeMessage();
    expect(send().disabled).toBe(true);
    press(box, "Enter");
    messageBox().form!.requestSubmit();
    await flushFaked();
    expect(box.value).toBe("server");
    expect(net.requests.filter((r) => r.method === "POST")).toEqual([]);
  });
});
