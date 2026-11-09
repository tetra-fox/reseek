import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ConfigView } from "#lib/config/fields.js";
import fixture from "#lib/config/fixtures/view.json";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { session } from "#lib/session.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import GroupsPage from "./GroupsPage.svelte";

/** The options with two groups of our own, jazz above rock */
const view = (): ConfigView => {
  const v = structuredClone(fixture) as unknown as ConfigView;
  const groups = (v.values as { transfers: { groups: Record<string, unknown> } }).transfers.groups;
  groups.order = ["buddies", "jazz", "rock", "leechers"];
  groups.user_defined = { jazz: { color: "blue" }, rock: { color: "red" } };
  return v;
};

const group = (name: string, builtin = false) => ({
  name,
  builtin,
  kind: name === "leechers" ? "smart" : "manual",
  defined: true,
  members: 0,
  shares: [],
  color: null,
  uploadSettings: false
});

describe("the groups page", () => {
  afterEach(() => {
    session.principal = null;
    toasts.splice(0);
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const start = async () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/options/view")) return view();
      if (url.pathname.endsWith("/groups")) {
        return {
          groups: ["buddies", "jazz", "rock", "leechers"].map((n) =>
            group(n, !["jazz", "rock"].includes(n))
          )
        };
      }
      if (url.pathname.endsWith("/shares")) return {};
      if (method === "PATCH")
        return { applied: [], restartRequired: [], reconnectRequired: [], revision: "x" };
      return undefined;
    });
    const component = mount(GroupsPage, { target: document.body });
    await flush();
    await flush();
    return { net, component };
  };
  const button = (label: string) =>
    [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.getAttribute("aria-label") === label || b.textContent?.trim() === label
    )!;

  it("writes a move still waiting when the page is left", async () => {
    const { net, component } = await start();
    button("Move rock up").click();
    flushSync();
    // left at once, inside the pause before the order is written
    unmount(component);
    await flush();
    const patch = net.requests.find((r) => r.method === "PATCH");
    expect(patch?.body).toMatchObject({
      set: { "transfers.groups.order": ["buddies", "rock", "jazz", "leechers"] }
    });
  });

  it("keeps a deleted group's name taken while its Undo waits", async () => {
    const { net, component } = await start();
    button("Delete group jazz").click();
    flushSync();
    expect(document.querySelector('[data-line="jazz"]')).toBeNull();
    button("New group").click();
    await flush();
    const name = document.querySelector<HTMLInputElement>("#group-name")!;
    name.value = "jazz";
    name.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(name.getAttribute("aria-invalid")).toBe("true");
    expect(button("Create").disabled).toBe(true);
    expect(net.requests.filter((r) => r.method === "PATCH")).toEqual([]);
    unmount(component);
  });
});
