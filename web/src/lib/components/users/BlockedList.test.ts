import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { session } from "#lib/session.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import BlockedList from "./BlockedList.svelte";

describe("importing blocks", () => {
  afterEach(() => {
    session.principal = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const button = (text: string) =>
    [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === text)!;

  it("sends once however often Import is pressed, and opens again without the last error", async () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    const net = fakeNetwork(async (url, method) => {
      if (method === "GET")
        return url.pathname.endsWith("/users/blocked") ? { users: [], rules: null } : undefined;
      await held;
      return { status: 400, body: { error: "not a block list" } };
    });
    const component = mount(BlockedList, { target: document.body });
    await flush();
    button("Import").click();
    flushSync();
    const box = document.querySelector<HTMLTextAreaElement>("[aria-label='Blocks to import']")!;
    box.value = "alice";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const send = () =>
      [...document.querySelectorAll<HTMLElement>("[role=dialog] button")].find(
        (b) => b.textContent?.trim() === "Import"
      )!;
    send().click();
    send().click();
    release();
    await flush();
    expect(net.requests.filter((r) => r.method === "POST")).toHaveLength(1);
    expect(document.body.textContent).toContain("not a block list");

    button("Cancel").click();
    await flush();
    button("Import").click();
    await flush();
    expect(document.body.textContent).not.toContain("not a block list");
    unmount(component);
  });
});
