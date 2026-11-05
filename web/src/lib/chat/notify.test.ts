import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { SUMMARY_TTL_MS, users } from "#lib/users/users.svelte.js";

import type { NotifyMode } from "./modes";
import {
  type Arrival,
  arrivalMode,
  notifier,
  type NotifyOptions,
  shouldNotify
} from "./notify.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const options: NotifyOptions = {
  onlyWhenHidden: true,
  privateMessages: "both",
  mentions: "both",
  roomMessages: "off",
  batchFinished: "both",
  uploadQueued: "off"
};
const arrival = (fields: Partial<Arrival>): Arrival => ({
  kind: "private",
  level: "all",
  mention: false,
  muted: false,
  ...fields
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
/** the directory's summary of alice, muted or not */
const directory = (muted: boolean) => {
  const net = fakeNetwork((url) =>
    url.pathname.endsWith("/users/alice/summary") ? { username: "alice", muted } : undefined
  );
  return () => net.requests.filter((r) => r.url.pathname.endsWith("/summary")).length;
};

describe("which messages notify", () => {
  it("takes the mode of the message's type, plain room messages only in an all-messages room", () => {
    expect(arrivalMode(arrival({}), options)).toBe("both");
    expect(arrivalMode(arrival({ kind: "room", level: "mentions", mention: true }), options)).toBe(
      "both"
    );
    expect(arrivalMode(arrival({ kind: "room", level: "all" }), options)).toBe("off");
    const rooms = { ...options, roomMessages: "sound" } as const;
    expect(arrivalMode(arrival({ kind: "room", level: "all" }), rooms)).toBe("sound");
    expect(arrivalMode(arrival({ kind: "room", level: "mentions" }), rooms)).toBe("off");
  });

  it("stays quiet for a muted user, a level of none, a type that is off or a pane in view", () => {
    expect(arrivalMode(arrival({ muted: true }), options)).toBe("off");
    expect(arrivalMode(arrival({ level: "none" }), options)).toBe("off");
    expect(
      arrivalMode(arrival({ kind: "room", mention: true }), { ...options, mentions: "off" })
    ).toBe("off");
    expect(shouldNotify("off", false, false)).toBe(false);
    expect(shouldNotify("sound", true, true)).toBe(false);
    expect(shouldNotify("sound", true, false)).toBe(true);
    expect(shouldNotify("desktop", false, true)).toBe(true);
  });
});

describe("what a mode delivers", () => {
  const played = vi.fn();
  const shown = vi.fn();
  const permission = notifier.permission;

  beforeEach(() => {
    played.mockClear();
    shown.mockClear();
    vi.stubGlobal(
      "Audio",
      class {
        play = async () => played();
      }
    );
    vi.stubGlobal(
      "Notification",
      class {
        constructor(title: string) {
          shown(title);
        }
      }
    );
  });
  afterEach(() => {
    app.options = null;
    notifier.permission = permission;
    toasts.splice(0);
    users.clear();
    vi.unstubAllGlobals();
  });

  const MATRIX: [NotifyMode, typeof permission, boolean, boolean][] = [
    ["off", "granted", false, false],
    ["sound", "granted", true, false],
    ["desktop", "granted", false, true],
    ["both", "granted", true, true],
    ["off", "default", false, false],
    ["sound", "default", true, false],
    ["desktop", "default", false, false],
    ["both", "default", true, false],
    ["desktop", "denied", false, false],
    ["both", "denied", true, false],
    ["desktop", "insecure", false, false],
    ["both", "insecure", true, false]
  ];

  it.each(MATRIX)(
    "%s with permission %s: sound %s, desktop %s",
    async (mode, allowed, sound, desktop) => {
      notifier.permission = allowed;
      app.options = {
        chat: { notifications: { only_when_hidden: false, private_messages: mode } }
      } as never;
      directory(false);
      const expectDelivered = () => {
        expect(played).toHaveBeenCalledTimes(sound ? 1 : 0);
        expect(shown).toHaveBeenCalledTimes(desktop ? 1 : 0);
        played.mockClear();
        shown.mockClear();
      };
      notifier.other(mode, "Download finished", "a.flac from alice", "download:1", "/downloads");
      expectDelivered();
      await notifier.privateMessage("alice", message, "all", null);
      expectDelivered();
    }
  );

  it("delivers nothing while the tab is visible when only hidden tabs are notified", () => {
    notifier.permission = "granted";
    const visible = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    notifier.other("both", "Download finished", "", "download:1", "/downloads");
    expect(played).not.toHaveBeenCalled();
    expect(shown).not.toHaveBeenCalled();
    visible.mockReturnValue("hidden");
    notifier.other("both", "Download finished", "", "download:1", "/downloads");
    expect(played).toHaveBeenCalledOnce();
    expect(shown).toHaveBeenCalledOnce();
    visible.mockRestore();
  });
});

describe("asking for permission", () => {
  const permission = notifier.permission;
  afterEach(() => {
    notifier.permission = permission;
    vi.unstubAllGlobals();
  });

  it("takes the answer of a browser that gives it only to a callback", async () => {
    vi.stubGlobal("Notification", {
      permission: "default",
      requestPermission: (answer?: (p: NotificationPermission) => void) => {
        answer?.("granted");
        return undefined;
      }
    });
    notifier.permission = "default";
    await notifier.ask();
    expect(notifier.permission).toBe("granted");
  });
});

describe("the new message toast", () => {
  // jsdom's media elements don't play, and private messages sound by default
  beforeEach(() => {
    vi.stubGlobal(
      "Audio",
      class {
        play = async () => {};
      }
    );
  });
  afterEach(() => {
    app.options = null;
    toasts.splice(0);
    users.clear();
    vi.unstubAllGlobals();
  });

  const toasted = async (level: Schemas["NotifyLevel"], open: string | null = null) => {
    toasts.splice(0);
    await notifier.privateMessage("alice", message, level, open);
    return toasts.length;
  };

  it("follows the notification rules, with the tab's visibility set aside", async () => {
    directory(false);
    expect(await toasted("all")).toBe(1);
    expect(await toasted("all", "user:alice")).toBe(0);
    expect(await toasted("none")).toBe(0);
    app.options = { chat: { notifications: { private_messages: "off" } } } as never;
    expect(await toasted("all")).toBe(0);
    app.options = { chat: { notifications: { private_messages: "sound" } } } as never;
    expect(await toasted("all")).toBe(1);
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
