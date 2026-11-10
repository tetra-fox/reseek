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

  it("makes two groups asked for at once, the second from the options the first left", async () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    // a daemon that refuses a write made against options it has changed since
    let options = view();
    let changes = 0;
    const net = fakeNetwork((url, method, body) => {
      if (url.pathname.endsWith("/options/view")) return structuredClone(options);
      if (url.pathname.endsWith("/groups")) {
        const gone = (name: string) => ({ ...group(name), defined: false, members: 1 });
        return { groups: [group("buddies", true), group("jazz"), gone("ska"), gone("dub")] };
      }
      if (url.pathname.endsWith("/shares")) return {};
      if (method !== "PATCH") return undefined;
      const sent = body as { optionsRevision?: string; revision?: string; set: object };
      const read = url.pathname.endsWith("/file") ? sent.optionsRevision : sent.revision;
      if (read !== options.revision) {
        return {
          status: 409,
          body: { error: "The options changed since they were read", detail: options.revision }
        };
      }
      options = structuredClone(options);
      options.revision = `changed ${++changes}`;
      const groups = (options.values as { transfers: { groups: Record<string, unknown> } })
        .transfers.groups;
      groups.user_defined = (sent.set as Record<string, unknown>)["transfers.groups.user_defined"];
      return {
        revision: "file",
        optionsRevision: options.revision,
        applied: [],
        restartRequired: [],
        reconnectRequired: []
      };
    });
    const component = mount(GroupsPage, { target: document.body });
    await flush();
    await flush();
    const create = [...document.querySelectorAll<HTMLButtonElement>("button")].filter(
      (b) => b.textContent?.trim() === "Create group"
    );
    expect(create).toHaveLength(2);
    for (const b of create) b.click();
    await vi.waitFor(() =>
      expect(toasts.map((t) => t.text)).toEqual(
        expect.arrayContaining(["Group ska created", "Group dub created"])
      )
    );
    const made = (options.values as { transfers: { groups: { user_defined: object } } }).transfers
      .groups.user_defined;
    expect(Object.keys(made).toSorted()).toEqual(["dub", "jazz", "rock", "ska"]);
    expect(net.requests.filter((r) => r.method === "PATCH")).toHaveLength(2);
    unmount(component);
  });

  it("moves a group by its buttons, keeping focus on the button pressed", async () => {
    const { component } = await start();
    const order = () =>
      [...document.querySelectorAll("li[data-line]")].map((l) => l.getAttribute("data-line"));
    const press = async (label: string) => {
      const pressed = button(label);
      pressed.focus();
      pressed.click();
      await flush();
    };
    await press("Move jazz down");
    expect(order()).toEqual(["buddies", "rock", "jazz", "leechers"]);
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Move jazz down");
    await press("Move jazz down");
    expect(order()).toEqual(["buddies", "rock", "leechers", "jazz"]);
    // at the bottom its down is off, so focus goes to its up
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Move jazz up");
    unmount(component);
  });
});
