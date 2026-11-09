import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { SETTLE_MS, Tabs } from "#lib/ui/tabs.js";
import { SUMMARY_TTL_MS, users } from "#lib/users/users.svelte.js";

import type { NotifyMode } from "./modes";
import {
  type Arrival,
  arrivalMode,
  CHIME_GAP_MS,
  notifier,
  type NotifyOptions
} from "./notify.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const options: NotifyOptions = {
  onlyWhenHidden: true,
  privateMessages: "both",
  mentions: "both",
  roomMessages: "off",
  jobFinished: "both",
  uploadQueued: "off",
  buddyPresence: "both",
  scansFinished: "off",
  analysisFinished: "off",
  maintenanceFinished: "off",
  wishlistFound: "both"
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
  wasReplayed: false,
  autoReply: false
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

  it("stays quiet for a muted user, a level of none or a type that is off", () => {
    expect(arrivalMode(arrival({ muted: true }), options)).toBe("off");
    expect(arrivalMode(arrival({ level: "none" }), options)).toBe("off");
    expect(
      arrivalMode(arrival({ kind: "room", mention: true }), { ...options, mentions: "off" })
    ).toBe("off");
  });
});

describe("what a mode delivers", () => {
  const played = vi.fn();
  const shown = vi.fn();
  const permission = notifier.permission;
  /** the clock the sound is spaced by, moved past the gap before each delivery */
  let now = 0;
  const later = () => (now += CHIME_GAP_MS);

  // the app starts the stream, which has the tabs agree on the one that plays; they are
  // settled before its frames come
  beforeAll(async () => {
    fakeNetwork(() => undefined);
    live.start();
    live.stop();
    vi.unstubAllGlobals();
    await new Promise((done) => setTimeout(done, SETTLE_MS));
  });
  beforeEach(() => {
    played.mockClear();
    shown.mockClear();
    later();
    vi.spyOn(performance, "now").mockImplementation(() => now);
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
    vi.restoreAllMocks();
  });

  const MATRIX: [NotifyMode, typeof permission, boolean, boolean][] = [
    ["off", "granted", false, false],
    ["sound", "granted", true, false],
    ["desktop", "granted", false, true],
    ["both", "granted", true, true],
    ["desktop", "default", false, false],
    ["both", "denied", true, false]
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
        later();
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

  it("plays the first sound in a tab alone", () => {
    app.options = { chat: { notifications: { only_when_hidden: false } } } as never;
    notifier.other("sound", "Download finished", "", "download:1", "/downloads");
    expect(played).toHaveBeenCalledOnce();
  });

  it("plays the sound only in the oldest tab of the browser", async () => {
    app.options = { chat: { notifications: { only_when_hidden: false } } } as never;
    const pause = () => new Promise((done) => setTimeout(done, 20));
    // a tab opened before this one, on the channel the tabs share
    const channel = new BroadcastChannel("reseek-tabs");
    const older = new Tabs(channel, 0);
    await pause();
    notifier.other("sound", "Download finished", "", "download:1", "/downloads");
    expect(played).not.toHaveBeenCalled();
    older.leave();
    channel.close();
    await pause();
    later();
    notifier.other("sound", "Download finished", "", "download:2", "/downloads");
    expect(played).toHaveBeenCalledOnce();
  });

  it("plays one sound for a burst of messages", () => {
    app.options = { chat: { notifications: { only_when_hidden: false } } } as never;
    notifier.other("sound", "Download finished", "", "download:1", "/downloads");
    notifier.other("sound", "Download finished", "", "download:2", "/downloads");
    expect(played).toHaveBeenCalledOnce();
    later();
    notifier.other("sound", "Download finished", "", "download:3", "/downloads");
    expect(played).toHaveBeenCalledTimes(2);
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
    vi.useRealTimers();
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
  });
});
