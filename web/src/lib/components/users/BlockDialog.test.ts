import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { blocking } from "#lib/users/block.svelte.js";

import BlockDialog from "./BlockDialog.svelte";

describe("the block dialog", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    blocking.open = false;
    app.options = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const open = async (add?: Schemas["BlockScope"][]) => {
    const net = fakeNetwork((url, method) =>
      method === "PUT" && url.pathname.endsWith("/block") ? { status: 200, body: {} } : undefined
    );
    component = mount(BlockDialog, { target: document.body });
    flushSync();
    blocking.ask("alice", add);
    await flush();
    return net;
  };
  const box = (label: string) =>
    [...document.querySelectorAll("label")]
      .find((l) => l.textContent?.trim() === label)!
      .querySelector<HTMLElement>("[role=checkbox]")!;
  const checked = () =>
    [...document.querySelectorAll("label")]
      .filter((l) => l.querySelector('[role=checkbox][aria-checked="true"]'))
      .map((l) => l.textContent?.trim());
  const blockButton = () =>
    [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Block alice")!;

  it("starts from the default scopes with what the caller adds, and sends what is chosen", async () => {
    app.options = { blocking: { default_scopes: ["searches", "browse"] } } as never;
    const net = await open(["uploads"]);
    expect(checked()).toEqual([
      "Refuse uploads to this user",
      "Withhold search results from this user",
      "Refuse share browsing"
    ]);
    box("Cancel downloads from this user").click();
    box("Withhold search results from this user").click();
    flushSync();
    const reason = document.querySelector<HTMLInputElement>("#block-reason")!;
    reason.value = "  spam ";
    reason.dispatchEvent(new Event("input", { bubbles: true }));
    blockButton().click();
    await flush();
    expect(net.requests.map((r) => [r.method, r.url.pathname, r.body])).toContainEqual([
      "PUT",
      "/api/v0/users/alice/block",
      {
        reason: "spam",
        scopes: ["browse", "uploads", "downloads"],
        addresses: [],
        expiresIn: null,
        cancelTransfers: true
      }
    ]);
    expect(blocking.open).toBe(false);
  });

  it("needs one scope at least", async () => {
    const net = await open();
    for (const label of checked()) box(label!).click();
    flushSync();
    expect(blockButton().disabled).toBe(true);
    expect(document.body.textContent).toContain("Select at least one");

    expect(net.requests.filter((r) => r.method === "PUT")).toEqual([]);
  });
});
