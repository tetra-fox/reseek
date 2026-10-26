import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Keep from "#lib/testing/Keep.svelte";

import { FALLBACK_MS, live } from "./live.svelte";

describe("keepCurrent", () => {
  let send: (text: string) => void;
  let end: () => void;

  beforeEach(() => {
    vi.useFakeTimers();
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
    live.stop();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("follows its topic, reloads on lagged, and polls while the stream is down", async () => {
    const reload = vi.fn();
    const component = mount(Keep, { target: document.body, props: { reload, topic: "transfers" } });
    flushSync();
    expect(live.topics).toEqual(["transfers"]);
    live.start();
    send('id: 1\nevent: hello\ndata: {"principal":{"name":"a","role":"ReadOnly"}}\n\n');
    await vi.advanceTimersByTimeAsync(0);
    expect(live.status).toBe("connected");
    await vi.advanceTimersByTimeAsync(FALLBACK_MS * 2);
    expect(reload).not.toHaveBeenCalled();

    send("id: 1\nevent: lagged\ndata: {}\n\n");
    await vi.advanceTimersByTimeAsync(0);
    expect(reload).toHaveBeenCalledOnce();

    end();
    await vi.advanceTimersByTimeAsync(0);
    expect(live.status).toBe("polling");
    flushSync();
    await vi.advanceTimersByTimeAsync(FALLBACK_MS);
    expect(reload).toHaveBeenCalledTimes(2);

    unmount(component);
    expect(live.topics).toEqual([]);
  });
});
