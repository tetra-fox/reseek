import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { toasts } from "#lib/toast.svelte.js";
import { SUMMARY_TTL_MS, users } from "#lib/users.svelte.js";

import { type Arrival, notifier, type NotifyOptions, shouldNotify } from "./notify.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const options: NotifyOptions = {
  enabled: true,
  onlyWhenHidden: true,
  sound: false,
  privateMessages: true,
  mentions: true,
  roomMessages: false
};
const arrival = (fields: Partial<Arrival>): Arrival => ({
  kind: "private",
  level: "all",
  mention: false,
  muted: false,
  seen: false,
  ...fields
});

describe("which messages notify", () => {
  it("notifies private messages and room mentions by default, plain room messages not", () => {
    expect(shouldNotify(arrival({}), options)).toBe(true);
    expect(shouldNotify(arrival({ kind: "room", level: "mentions", mention: true }), options)).toBe(
      true
    );
    expect(shouldNotify(arrival({ kind: "room", level: "all" }), options)).toBe(false);
    expect(
      shouldNotify(arrival({ kind: "room", level: "all" }), { ...options, roomMessages: true })
    ).toBe(true);
    expect(
      shouldNotify(arrival({ kind: "room", level: "mentions" }), { ...options, roomMessages: true })
    ).toBe(false);
  });

  it("stays quiet for a muted user, a level of none, a pane in view, or options off", () => {
    expect(shouldNotify(arrival({ muted: true }), options)).toBe(false);
    expect(shouldNotify(arrival({ level: "none" }), options)).toBe(false);
    expect(shouldNotify(arrival({ seen: true }), options)).toBe(false);
    expect(shouldNotify(arrival({ seen: true }), { ...options, onlyWhenHidden: false })).toBe(true);
    expect(shouldNotify(arrival({}), { ...options, enabled: false })).toBe(false);
    expect(shouldNotify(arrival({}), { ...options, privateMessages: false })).toBe(false);
    expect(
      shouldNotify(arrival({ kind: "room", mention: true }), { ...options, mentions: false })
    ).toBe(false);
  });
});

describe("the new message toast", () => {
  afterEach(() => {
    app.options = null;
    toasts.splice(0);
    users.clear();
    vi.unstubAllGlobals();
  });

  const message = {
    localId: 1,
    id: 1,
    timestamp: "2026-10-26T10:00:00Z",
    username: "alice",
    direction: "In",
    message: "hi",
    isAcknowledged: false,
    wasReplayed: false
  } as Schemas["PrivateMessage"];
  const toasted = async (level: Schemas["NotifyLevel"], open: string | null = null) => {
    toasts.splice(0);
    await notifier.privateMessage("alice", message, level, open);
    return toasts.length;
  };
  /** the directory's summary of alice, muted or not */
  const directory = (muted: boolean) => {
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/users/alice/summary") ? { username: "alice", muted } : undefined
    );
    return () => net.requests.filter((r) => r.url.pathname.endsWith("/summary")).length;
  };

  it("follows the notification rules, with the tab's visibility set aside", async () => {
    directory(false);
    expect(await toasted("all")).toBe(1);
    expect(await toasted("all", "user:alice")).toBe(0);
    expect(await toasted("none")).toBe(0);
    app.options = { chat: { notifications: { enabled: false } } } as never;
    expect(await toasted("all")).toBe(0);
    app.options = { chat: { notifications: { private_messages: false } } } as never;
    expect(await toasted("all")).toBe(0);
  });

  it("reads the sender's summary first when none is held, and stays quiet for a muted one", async () => {
    const asked = directory(true);
    expect(users.get("alice")).toBeUndefined();
    expect(await toasted("all")).toBe(0);
    expect(asked()).toBe(1);
    // held now: not read again
    expect(await toasted("all")).toBe(0);
    expect(asked()).toBe(1);
  });

  it("reads a summary over a minute old again, so a mute made elsewhere holds", async () => {
    vi.useFakeTimers({ now: Date.parse("2026-10-26T10:00:00Z"), toFake: ["Date"] });
    let muted = false;
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/users/alice/summary") ? { username: "alice", muted } : undefined
    );
    expect(await toasted("all")).toBe(1);
    // muted from another browser, whose frame this tab doesn't follow
    muted = true;
    vi.setSystemTime(Date.now() + SUMMARY_TTL_MS + 1);
    expect(await toasted("all")).toBe(0);
    expect(net.requests.filter((r) => r.url.pathname.endsWith("/summary"))).toHaveLength(2);
    vi.useRealTimers();
  });
});
