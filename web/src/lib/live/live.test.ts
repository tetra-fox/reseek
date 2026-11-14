import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { EPOCH, frameText } from "#lib/testing/frames.js";

import { Live, type Topic } from "./live.svelte";
import { SILENCE_MS } from "./stream";

/** One request the fake server got, with the body it answers through */
type Opened = {
  url: string;
  headers: Record<string, string>;
  signal: AbortSignal;
  send: (text: string) => void;
  end: () => void;
};

/**
 * A `fetch` for the event stream whose answers the test writes, frame by frame. `refuse` may
 * answer the request with that number, counted from 0, with something else than a stream
 */
const fakeServer = (refuse: (n: number) => Response | undefined = () => undefined) => {
  const opened: Opened[] = [];
  let asked = 0;
  const fetcher = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    const refused = refuse(asked++);
    if (refused) return refused;
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    const body = new ReadableStream<Uint8Array>({ start: (c) => void (controller = c) });
    const encoder = new TextEncoder();
    const signal = init!.signal!;
    signal.addEventListener("abort", () => controller.error(new DOMException("aborted")));
    opened.push({
      url: String(url),
      headers: init!.headers as Record<string, string>,
      signal,
      send: (text) => controller.enqueue(encoder.encode(text)),
      end: () => controller.close()
    });
    return new Response(body);
  });
  return { opened, fetcher: fetcher as unknown as typeof fetch };
};

const hello = (seq: number, resumed = false) =>
  frameText(seq, "hello", {
    serverTime: "2026-10-26T12:00:00Z",
    version: {},
    principal: { name: "admin", role: "Administrator" },
    topics: [],
    resumed,
    epoch: EPOCH
  });

/** Lets the stream readers take what was written */
const settle = () => vi.advanceTimersByTimeAsync(0);

describe("Live", () => {
  let unauthorized: ReturnType<typeof vi.fn<() => void>>;
  let withheld: ReturnType<typeof vi.fn<() => void>>;
  let notices: string[];
  const make = (fetcher: typeof fetch) => {
    unauthorized = vi.fn<() => void>();
    withheld = vi.fn<() => void>();
    notices = [];
    return new Live({
      fetch: fetcher,
      url: (topics, reopen) => `/live?topics=${topics.join(",")}${reopen ? "&reopen=true" : ""}`,
      unauthorized,
      withheld,
      notice: (text) => notices.push(text)
    });
  };

  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => void vi.useRealTimers());

  it("hands over the snapshots, then the frames, in the order they came", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.retain("users");
    live.retain("application");
    const seen: string[] = [];
    live.on("application.state", () => seen.push("state"));
    live.on("application.options", () => seen.push("options"));
    live.on("users.removed", (data) => seen.push(`removed ${data.username}`));
    live.start();
    expect(live.status).toBe("connecting");
    expect(server.opened[0].url).toBe("/live?topics=application,users");
    expect(server.opened[0].headers["Last-Event-ID"]).toBeUndefined();
    server.opened[0].send(hello(10));
    server.opened[0].send(
      frameText(10, "application.state", {}) + frameText(10, "application.options", {})
    );
    server.opened[0].send(frameText(11, "users.removed", { username: "a" }));
    server.opened[0].send(frameText(12, "users.removed", { username: "b" }));
    await settle();
    expect(live.status).toBe("connected");
    expect(seen).toEqual(["state", "options", "removed a", "removed b"]);
  });

  it("reopens a lost stream from the last frame, and grows the wait after three quick losses", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.retain("application");
    live.start();
    server.opened[0].send(hello(5) + frameText(6, "users.removed", { username: "a" }));
    await settle();
    server.opened[0].end();
    await settle();
    expect(live.status).toBe("polling");
    expect(notices).toEqual(["Live connection to reseek lost, refreshing until it is back"]);
    await vi.advanceTimersByTimeAsync(999);
    expect(server.opened).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(server.opened).toHaveLength(2);
    expect(server.opened[1].headers["Last-Event-ID"]).toBe(`${EPOCH}-6`);

    // the second loss waits 1 s again, the third starts the growing wait, which doubles with
    // every loss after it, however long each attempt took, up to 30 s
    const waits: number[] = [];
    for (let attempt = 1; attempt <= 7; attempt++) {
      server.opened.at(-1)!.end();
      await settle();
      const before = server.opened.length;
      let waited = 0;
      while (server.opened.length === before) {
        await vi.advanceTimersByTimeAsync(100);
        waited += 100;
      }
      waits.push(waited);
    }
    expect(waits).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000]);
    expect(notices).toHaveLength(1);

    server.opened.at(-1)!.send(hello(6, true));
    await settle();
    expect(live.status).toBe("connected");
    expect(notices.at(-1)).toBe("Live connection restored");
  });

  it("stops for good when the session is gone", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.start();
    server.opened[0].send(hello(1) + frameText(2, "bye", { reason: "unauthorized" }));
    await settle();
    expect(unauthorized).toHaveBeenCalledOnce();
    expect(live.status).toBe("off");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(server.opened).toHaveLength(1);

    const refused = fakeServer(() => new Response(null, { status: 401 }));
    const other = make(refused.fetcher);
    other.start();
    await settle();
    expect(unauthorized).toHaveBeenCalledOnce();
    expect(other.status).toBe("off");
  });

  it("stops, keeping the session, while its user has no role", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.start();
    server.opened[0].send(hello(1) + frameText(2, "bye", { reason: "withheld" }));
    await settle();
    expect(withheld).toHaveBeenCalledOnce();
    expect(unauthorized).not.toHaveBeenCalled();
    expect(live.status).toBe("off");

    const refused = fakeServer(() => Response.json({ error: "access_withheld" }, { status: 403 }));
    const other = make(refused.fetcher);
    other.start();
    await settle();
    expect(withheld).toHaveBeenCalledOnce();
    expect(unauthorized).not.toHaveBeenCalled();
    expect(other.status).toBe("off");
  });

  it("reconnects after a goodbye for any other reason", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.start();
    server.opened[0].send(hello(1) + frameText(1, "bye", { reason: "shutdown" }));
    await settle();
    expect(live.status).toBe("polling");
    await vi.advanceTimersByTimeAsync(1000);
    expect(server.opened).toHaveLength(2);
  });

  it("opens a stream for new topics before closing the old one, and says which it added", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.retain("application");
    const removed: string[] = [];
    live.on("users.removed", (data) => removed.push(data.username));
    const added: Topic[][] = [];
    live.onAdded((topics) => added.push(topics));
    live.start();
    server.opened[0].send(hello(1) + frameText(2, "users.removed", { username: "old" }));
    await settle();
    // a stream that did not resume sends the snapshots of all its topics
    expect(added).toEqual([]);

    const release = live.retain("users");
    await vi.advanceTimersByTimeAsync(249);
    expect(server.opened).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(server.opened).toHaveLength(2);
    expect(server.opened[1].url).toBe("/live?topics=application,users");
    expect(server.opened[1].headers["Last-Event-ID"]).toBe(`${EPOCH}-2`);
    expect(server.opened[0].signal.aborted).toBe(false);

    // the old stream is silent from here; the new one replays from frame 2
    server.opened[0].send(frameText(3, "users.removed", { username: "twice" }));
    server.opened[1].send(hello(2, true) + frameText(3, "users.removed", { username: "twice" }));
    await settle();
    expect(server.opened[0].signal.aborted).toBe(true);
    expect(removed).toEqual(["old", "twice"]);
    expect(live.status).toBe("connected");
    expect(added).toEqual([["users"]]);

    // a release and a retain within the debounce open nothing
    release();
    live.retain("users");
    await vi.advanceTimersByTimeAsync(500);
    expect(server.opened).toHaveLength(2);
  });

  it("lets a new-topics stream take over when the old one drops before it greets", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.retain("application");
    const removed: string[] = [];
    live.on("users.removed", (data) => removed.push(data.username));
    live.start();
    server.opened[0].send(hello(1));
    await settle();
    live.retain("users");
    await vi.advanceTimersByTimeAsync(250);
    expect(server.opened).toHaveLength(2);

    server.opened[0].end();
    await settle();
    server.opened[1].send(hello(1, true));
    await settle();
    expect(live.status).toBe("connected");

    // nothing else is opened, and every frame is handed over once
    await vi.advanceTimersByTimeAsync(5_000);
    expect(server.opened).toHaveLength(2);
    server.opened[1].send(frameText(2, "users.removed", { username: "once" }));
    await settle();
    expect(removed).toEqual(["once"]);
  });

  it("takes a silent stream for dead", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.start();
    server.opened[0].send(hello(1));
    await settle();
    await vi.advanceTimersByTimeAsync(SILENCE_MS - 1000);
    server.opened[0].send(": ping\n\n");
    await vi.advanceTimersByTimeAsync(SILENCE_MS - 1000);
    expect(server.opened[0].signal.aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1000);
    expect(server.opened[0].signal.aborted).toBe(true);
    expect(live.status).toBe("polling");
    await vi.advanceTimersByTimeAsync(1000);
    expect(server.opened).toHaveLength(2);
  });

  it("hands a frame to every listener though one of them throws, and reads on", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const server = fakeServer();
    const live = make(server.fetcher);
    const seen: string[] = [];
    live.on("users.removed", () => {
      throw new TypeError("a state this page does not know");
    });
    live.on("users.removed", (data) => seen.push(data.username));
    live.start();
    server.opened[0].send(hello(1) + frameText(2, "users.removed", { username: "a" }));
    server.opened[0].send(frameText(3, "users.removed", { username: "b" }));
    await settle();
    expect(seen).toEqual(["a", "b"]);
    expect(live.status).toBe("connected");
    expect(notices).toEqual([]);
    expect(server.opened).toHaveLength(1);
    expect(error).toHaveBeenCalledTimes(2);
    error.mockRestore();
  });

  it("reopens from the last frame when new topics fail to open, which is no lost connection", async () => {
    const server = fakeServer((n) => (n === 1 ? new Response(null, { status: 500 }) : undefined));
    const live = make(server.fetcher);
    live.retain("application");
    const removed: string[] = [];
    live.on("users.removed", (data) => removed.push(data.username));
    live.start();
    server.opened[0].send(hello(1) + frameText(2, "users.removed", { username: "a" }));
    await settle();
    live.retain("users");
    await vi.advanceTimersByTimeAsync(250);
    await settle();
    // the old stream dropped its frames while the new topics opened, and is closed when they
    // fail; the stream that takes over brings those frames again
    expect(server.opened[0].signal.aborted).toBe(true);
    expect(live.status).toBe("connecting");
    expect(live.retryAt).toBeNull();
    expect(server.opened).toHaveLength(2);
    expect(server.opened[1].url).toBe("/live?topics=application,users&reopen=true");
    expect(server.opened[1].headers["Last-Event-ID"]).toBe(`${EPOCH}-2`);
    server.opened[1].send(hello(2, true) + frameText(3, "users.removed", { username: "again" }));
    await settle();
    expect(removed).toEqual(["a", "again"]);
    expect(live.status).toBe("connected");
    expect(notices).toEqual([]);
  });

  it("reopens at once at the stream cap, asking the daemon to wait for the old stream's place", async () => {
    const full = () => Response.json({ error: "Too many event streams are open" }, { status: 429 });
    const server = fakeServer((n) => (n === 1 ? full() : undefined));
    const live = make(server.fetcher);
    live.retain("application");
    live.start();
    server.opened[0].send(hello(1));
    await settle();
    expect(server.opened[0].url).toBe("/live?topics=application");
    live.retain("users");
    await vi.advanceTimersByTimeAsync(250);
    await settle();
    // the refused replacement is followed by the reopen with no wait between
    expect(server.fetcher).toHaveBeenCalledTimes(3);
    expect(server.opened[0].signal.aborted).toBe(true);
    expect(server.opened[1].url).toBe("/live?topics=application,users&reopen=true");
    expect(server.opened[1].headers["Last-Event-ID"]).toBe(`${EPOCH}-1`);
    expect(live.status).toBe("connecting");
    server.opened[1].send(hello(1, true));
    await settle();
    expect(live.status).toBe("connected");
    expect(notices).toEqual([]);
  });

  it("stops for a request it refuses as wrong, says why, and tries again for other topics", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const server = fakeServer((n) =>
      n === 0 ? Response.json({ error: "Unknown topic bogus" }, { status: 400 }) : undefined
    );
    const live = make(server.fetcher);
    const release = live.retain("bogus" as Topic);
    live.start();
    await settle();
    expect(live.status).toBe("rejected");
    expect(live.rejection).toBe("Unknown topic bogus");
    expect(live.pollMs).not.toBeNull();
    expect(notices).toEqual([]);
    expect(error).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(server.fetcher).toHaveBeenCalledOnce();

    release();
    live.retain("users");
    await vi.advanceTimersByTimeAsync(250);
    expect(server.opened).toHaveLength(1);
    server.opened[0].send(hello(1));
    await settle();
    expect(live.status).toBe("connected");
    expect(live.rejection).toBeNull();
    error.mockRestore();
  });

  it("says frames were missed once for a stream that greets without resuming", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    let missed = 0;
    live.onMissed(() => (missed += 1));
    live.start();
    server.opened[0].send(hello(1));
    await settle();
    expect(missed).toBe(0);
    server.opened[0].send(frameText(2, "lagged", {}));
    await settle();
    expect(missed).toBe(1);

    server.opened[0].end();
    await vi.advanceTimersByTimeAsync(1000);
    // the daemon could not resume: its greeting says so, and the lagged after it again
    server.opened[1].send(hello(3) + frameText(3, "lagged", {}));
    await settle();
    expect(missed).toBe(2);
    server.opened[1].end();
    await vi.advanceTimersByTimeAsync(1000);
    server.opened[2].send(hello(3, true));
    await settle();
    expect(missed).toBe(2);
  });
});
