import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

/** A `fetch` for the event stream whose answers the test writes, frame by frame */
const fakeServer = (status = 200) => {
  const opened: Opened[] = [];
  const fetcher = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
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
    return new Response(status === 200 ? body : null, { status });
  });
  return { opened, fetcher: fetcher as unknown as typeof fetch };
};

const frame = (id: number, type: string, data: unknown) =>
  `id: ${id}\nevent: ${type}\ndata: ${JSON.stringify(data)}\n\n`;
const hello = (id: number, resumed = false) =>
  frame(id, "hello", {
    serverTime: "2026-10-26T12:00:00Z",
    version: {},
    principal: { name: "admin", role: "Administrator" },
    topics: [],
    resumed,
    epoch: "first"
  });

/** Lets the stream readers take what was written */
const settle = () => vi.advanceTimersByTimeAsync(0);

describe("Live", () => {
  let unauthorized: ReturnType<typeof vi.fn<() => void>>;
  let notices: string[];
  const make = (fetcher: typeof fetch) => {
    unauthorized = vi.fn<() => void>();
    notices = [];
    return new Live({
      fetch: fetcher,
      url: (topics) => `/live?topics=${topics.join(",")}`,
      unauthorized,
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
      frame(10, "application.state", {}) + frame(10, "application.options", {})
    );
    server.opened[0].send(frame(11, "users.removed", { username: "a" }));
    server.opened[0].send(frame(12, "users.removed", { username: "b" }));
    await settle();
    expect(live.status).toBe("connected");
    expect(live.principal?.name).toBe("admin");
    expect(seen).toEqual(["state", "options", "removed a", "removed b"]);
    expect(live.lastId).toBe("12");
  });

  it("reopens a lost stream from the last frame, and grows the wait after three quick losses", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.retain("application");
    live.start();
    server.opened[0].send(hello(5) + frame(6, "users.removed", { username: "a" }));
    await settle();
    server.opened[0].end();
    await settle();
    expect(live.status).toBe("polling");
    expect(notices).toEqual(["Live connection to reseek lost, refreshing until it is back"]);
    await vi.advanceTimersByTimeAsync(999);
    expect(server.opened).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(server.opened).toHaveLength(2);
    expect(server.opened[1].headers["Last-Event-ID"]).toBe("6");

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
    server.opened[0].send(hello(1) + frame(2, "bye", { reason: "unauthorized" }));
    await settle();
    expect(unauthorized).toHaveBeenCalledOnce();
    expect(live.status).toBe("off");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(server.opened).toHaveLength(1);

    const refused = fakeServer(401);
    const other = make(refused.fetcher);
    other.start();
    await settle();
    expect(unauthorized).toHaveBeenCalledOnce();
    expect(other.status).toBe("off");
  });

  it("reconnects after a goodbye for any other reason", async () => {
    const server = fakeServer();
    const live = make(server.fetcher);
    live.start();
    server.opened[0].send(hello(1) + frame(1, "bye", { reason: "shutdown" }));
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
    server.opened[0].send(hello(1) + frame(2, "users.removed", { username: "old" }));
    await settle();
    // a stream that did not resume sends the snapshots of all its topics
    expect(added).toEqual([]);

    const release = live.retain("users");
    await vi.advanceTimersByTimeAsync(249);
    expect(server.opened).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(server.opened).toHaveLength(2);
    expect(server.opened[1].url).toBe("/live?topics=application,users");
    expect(server.opened[1].headers["Last-Event-ID"]).toBe("2");
    expect(server.opened[0].signal.aborted).toBe(false);

    // the old stream is silent from here; the new one replays from frame 2
    server.opened[0].send(frame(3, "users.removed", { username: "twice" }));
    server.opened[1].send(hello(2, true) + frame(3, "users.removed", { username: "twice" }));
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
    server.opened[1].send(frame(2, "users.removed", { username: "once" }));
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
});
