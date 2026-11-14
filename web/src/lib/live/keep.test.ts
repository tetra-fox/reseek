import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Freeze, interaction } from "#lib/lists/interaction.svelte.js";
import { frameText } from "#lib/testing/frames.js";
import Keep from "#lib/testing/Keep.svelte";

import { RECONCILE_MS } from "./keep.svelte";
import { FALLBACK_MS, live } from "./live.svelte";

const principal = { name: "a", role: "ReadOnly" };
/** a stream's greeting after the `seq`th frame of the run */
const hello = (seq: number) => frameText(seq, "hello", { principal });
const resumed = (seq: number) => frameText(seq, "hello", { principal, resumed: true });
const LAGGED = frameText(2, "lagged", {});

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

  /** mounts a screen on topic `transfers`, with its list's `freeze`, and lets the stream greet */
  const connected = async (freeze?: Freeze) => {
    component = mount(Keep, {
      target: document.body,
      props: { reload, topic: "transfers", freeze }
    });
    flushSync();
    live.start();
    send(hello(1));
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
    send(LAGGED);
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
    send(hello(2));
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(live.status).toBe("connected");
    expect(reload).toHaveBeenCalledTimes(2);
  });

  it("reloads when a stream that resumed starts following its topic", async () => {
    const release = live.retain("application");
    try {
      live.start();
      send(hello(1));
      await vi.advanceTimersByTimeAsync(0);
      component = mount(Keep, { target: document.body, props: { reload, topic: "transfers" } });
      flushSync();
      await vi.advanceTimersByTimeAsync(300);
      expect(reload).not.toHaveBeenCalled();
      send(resumed(1));
      await vi.advanceTimersByTimeAsync(0);
      expect(reload).toHaveBeenCalledOnce();
    } finally {
      release();
    }
  });

  it("does not reload on the greeting of a stream reopened for other topics", async () => {
    await connected();
    const release = live.retain("search");
    await vi.advanceTimersByTimeAsync(300);
    send(resumed(1));
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(live.topics).toEqual(["search", "transfers"]);
    expect(reload).not.toHaveBeenCalled();
    release();
  });

  it("waits for its own list only, while another list is held", async () => {
    const own = new Freeze();
    const other = new Freeze();
    await connected(own);
    const elsewhere = other.hold();
    send(LAGGED);
    await pass(0);
    expect(reload).toHaveBeenCalledOnce();
    elsewhere();

    const held = own.hold();
    send(frameText(3, "lagged", {}));
    await pass(RECONCILE_MS);
    expect(reload).toHaveBeenCalledOnce();
    held();
    expect(reload).toHaveBeenCalledTimes(2);
  });

  it("waits while a list is held and reloads once when it is let go", async () => {
    await connected();
    const release = interaction.hold();
    send(LAGGED);
    await pass(RECONCILE_MS);
    setHidden(true);
    setHidden(false);
    expect(reload).not.toHaveBeenCalled();
    release();
    expect(reload).toHaveBeenCalledOnce();
  });
});
