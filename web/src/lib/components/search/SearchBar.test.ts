import { goto } from "$app/navigation";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { recent } from "#lib/search/recent.svelte.js";
import { searches } from "#lib/search/searches.svelte.js";
import { userSummary } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { press, pressEnter } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import SearchBar from "./SearchBar.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const connected = (isConnected: boolean) => {
  app.state = { server: { isConnected }, user: { username: "me" } } as never;
};

describe("the search box", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    app.state = null;
    searches.clear();
    recent.texts = [];
    recent.pinned = [];
    vi.mocked(goto).mockClear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("starts a search for the typed phrase on Enter and opens its page", async () => {
    const net = fakeNetwork((_url, method, body) => (method === "POST" ? body : undefined));
    connected(true);
    component = mount(SearchBar, { target: document.body });
    flushSync();
    const box = document.querySelector<HTMLInputElement>('input[type="search"]')!;
    box.value = "aphex twin";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    pressEnter(box);
    const [row] = searches.list.rows;
    expect(row?.searchText).toBe("aphex twin");
    expect(goto).toHaveBeenCalledWith(expect.stringContaining(`/searches/${row?.id}`));
    await vi.waitFor(() =>
      expect(net.requests).toContainEqual(
        expect.objectContaining({
          method: "POST",
          body: expect.objectContaining({ searchText: "aphex twin" })
        })
      )
    );
  });

  /** The box holding `text`, the directory's answer listed under it */
  const typed = async (text: string, listed: string[] = []) => {
    vi.useFakeTimers();
    const net = fakeNetwork((url, method, body) => {
      if (method === "POST") return body;
      if (url.pathname.endsWith("/api/v0/users")) {
        return { users: listed.map((username) => userSummary(username)), total: listed.length };
      }
      return url.pathname.endsWith("/AFX/profile") ? userSummary("AFX") : undefined;
    });
    connected(true);
    component = mount(SearchBar, { target: document.body });
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
  const nothingOpened = () =>
    expect(vi.mocked(goto).mock.calls.filter(([to]) => String(to).includes("/users/"))).toEqual([]);

  it("offers the search first, then recent searches, users and the look-up", async () => {
    recent.texts = ["aphex twin", "boards of canada", "aphex"];
    await typed("aphex", ["aphex_fan"]);
    expect(entries()).toEqual([
      "Search for aphex",
      "aphex twin",
      expect.stringContaining("aphex_fan"),
      "Look up aphex"
    ]);
  });

  it("runs the file search on Enter with users listed, whatever the pointer is on", async () => {
    const { box } = await typed("aphex", ["aphex"]);
    const user = document.querySelectorAll<HTMLElement>("[role=option]")[1];
    user.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, cancelable: true, pointerType: "mouse" })
    );
    await tick();
    press(box, "Enter");
    expect(searches.list.rows[0]?.searchText).toBe("aphex");
    nothingOpened();
  });

  it("opens a known user's shares when the keys move to them", async () => {
    const { box, net } = await typed("aphex", ["aphex"]);
    press(box, "ArrowDown");
    await tick();
    press(box, "Enter");
    await flushFaked();
    expect(goto).toHaveBeenCalledExactlyOnceWith("#/users/aphex/shares");
    expect(net.requests.filter((r) => r.method === "POST")).toEqual([]);
  });

  it("looks up a name as typed and opens their shares once the network knows them", async () => {
    const { box, net } = await typed("AFX");
    press(box, "ArrowDown");
    await tick();
    press(box, "Enter");
    await flushFaked();
    const profile = net.requests.find((r) => r.url.pathname.endsWith("/AFX/profile"));
    expect(profile?.url.searchParams.get("refresh")).toBe("true");
    expect(goto).toHaveBeenCalledExactlyOnceWith("#/users/AFX/shares");
  });

  it("offers no user for a search of one user's shares", async () => {
    vi.useFakeTimers();
    fakeNetwork(() => undefined);
    connected(true);
    component = mount(SearchBar, {
      target: document.body,
      props: { scope: { type: "user", subjects: ["bob"] } }
    });
    flushSync();
    const box = document.querySelector<HTMLInputElement>("[role=combobox]")!;
    box.value = "aphex";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await flushFaked();
    await tick();
    expect(entries()).toEqual(["Search for aphex"]);
  });
});
