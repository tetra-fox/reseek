import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { interaction } from "#lib/interaction.svelte.js";
import Keep from "#lib/testing/Keep.svelte";

import { RECONCILE_MS } from "./keep.svelte";
import { FALLBACK_MS, live } from "./live.svelte";

const HELLO = 'event: hello\ndata: {"principal":{"name":"a","role":"ReadOnly"}}\n\n';

describe("keepCurrent", () => {
  let send: (text: string) => void;
  let end: () => void;
  let hidden = false;
  let component: ReturnType<typeof mount> | null = null;
  const reload = vi.fn();

  const setHidden = (value: boolean) => {
    hidden = value;
    document.dispatchEvent(new Event("visibilitychange"));
    flushSync();
  };

  /** lets `ms` pass with the server's pings, so the stream isn't taken for dead */
  const pass = async (ms: number) => {
    for (let left = ms; left > 0; left -= 15_000) {
      await vi.advanceTimersByTimeAsync(Math.min(left, 15_000));
      send(": ping\n\n");
    }
    await vi.advanceTimersByTimeAsync(0);
  };

  /** mounts a screen on topic `transfers` and lets the stream greet */
  const connected = async () => {
    component = mount(Keep, { target: document.body, props: { reload, topic: "transfers" } });
    flushSync();
    live.start();
    send(`id: 1\n${HELLO}`);
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(live.status).toBe("connected");
  };

  beforeEach(() => {
    vi.useFakeTimers();
    reload.mockClear();
    hidden = false;
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init: RequestInit) => {
        let controller!: ReadableStreamDefaultController<Uint8Array>;
        const body = new ReadableStream<Uint8Array>({ start: (c) => void (controller = c) });
        init.signal!.addEventListener("abort", () => controller.error(new DOMException("aborted")));
        send = (text) => controller.enqueue(new TextEncoder().encode(text));
        end = () => controller.close();
        return new Response(body);
      })
    );
  });
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("follows its topic while mounted", async () => {
    await connected();
    expect(live.topics).toEqual(["transfers"]);
    unmount(component!);
    component = null;
    expect(live.topics).toEqual([]);
  });

  it("reconciles every minute while the tab is visible, and not while hidden", async () => {
    await connected();
    await pass(RECONCILE_MS - 1);
    expect(reload).not.toHaveBeenCalled();
    await pass(1);
    expect(reload).toHaveBeenCalledOnce();

    setHidden(true);
    await pass(RECONCILE_MS * 3);
    expect(reload).toHaveBeenCalledOnce();
  });

  it("reloads when the tab becomes visible again", async () => {
    await connected();
    setHidden(true);
    expect(reload).not.toHaveBeenCalled();
    setHidden(false);
    expect(reload).toHaveBeenCalledOnce();
  });

  it("reloads on lagged", async () => {
    await connected();
    send("id: 2\nevent: lagged\ndata: {}\n\n");
    await vi.advanceTimersByTimeAsync(0);
    expect(reload).toHaveBeenCalledOnce();
  });

  it("polls while the stream is down and reloads when a new stream greets", async () => {
    await connected();
    end();
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(live.status).toBe("polling");
    // the retry after a second opens a new stream, which has not greeted yet
    await vi.advanceTimersByTimeAsync(FALLBACK_MS);
    expect(reload).toHaveBeenCalledOnce();
    send(HELLO);
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(live.status).toBe("connected");
    expect(reload).toHaveBeenCalledTimes(2);
  });

  it("does not reload on the greeting of a stream reopened for other topics", async () => {
    await connected();
    const release = live.retain("search");
    await vi.advanceTimersByTimeAsync(300);
    send(HELLO);
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(live.topics).toEqual(["search", "transfers"]);
    expect(reload).not.toHaveBeenCalled();
    release();
  });

  it("waits while a list is held and reloads once when it is let go", async () => {
    await connected();
    const release = interaction.hold();
    send("id: 2\nevent: lagged\ndata: {}\n\n");
    await pass(RECONCILE_MS);
    setHidden(true);
    setHidden(false);
    expect(reload).not.toHaveBeenCalled();
    release();
    expect(reload).toHaveBeenCalledOnce();
  });
});
