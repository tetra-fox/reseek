import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { toasts } from "#lib/toast.svelte.js";

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
  const toasted = (level: Schemas["NotifyLevel"], open: string | null = null) => {
    toasts.splice(0);
    notifier.privateMessage("alice", message, level, open);
    return toasts.length;
  };

  it("follows the notification rules, with the tab's visibility set aside", () => {
    expect(toasted("all")).toBe(1);
    expect(toasted("all", "user:alice")).toBe(0);
    expect(toasted("none")).toBe(0);
    app.options = { chat: { notifications: { enabled: false } } } as never;
    expect(toasted("all")).toBe(0);
    app.options = { chat: { notifications: { private_messages: false } } } as never;
    expect(toasted("all")).toBe(0);
  });
});
