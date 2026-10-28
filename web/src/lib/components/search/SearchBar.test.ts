import { goto } from "$app/navigation";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { searches } from "#lib/search/searches.svelte.js";
import { pressEnter } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import SearchBar from "./SearchBar.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const connected = (isConnected: boolean) => {
  app.state = { server: { isConnected } } as never;
};

describe("the search box", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    app.state = null;
    searches.clear();
    vi.mocked(goto).mockClear();
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

  it("says why it can't search while disconnected", () => {
    connected(false);
    component = mount(SearchBar, { target: document.body, props: { text: "aphex twin" } });
    flushSync();
    expect(document.querySelector<HTMLInputElement>('input[type="search"]')!.disabled).toBe(true);
    expect(document.body.textContent).toContain("Connect to the Soulseek server to search.");
  });
});
