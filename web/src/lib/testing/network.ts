import { vi } from "vitest";

import type { FrameData, FrameType } from "#lib/live/live.svelte.js";

import { EPOCH, frameText } from "./frames";

/** What a fake API answers: a JSON body, or a status with an optional body */
export type Answer = unknown | { status: number; body?: unknown };

/**
 * Stands in for the daemon in a test: the event stream is held open and fed by `frame`, and
 * every other request is answered by `answer`, 404 when it gives undefined. A JSON body is
 * handed over parsed, any other as its text. Undone by `vi.unstubAllGlobals()`
 */
export const fakeNetwork = (
  answer: (url: URL, method: string, body: unknown) => Answer | Promise<Answer>
) => {
  let send: (text: string) => void = () => {};
  let end: () => void = () => {};
  let seq = 0;
  // the run of the daemon the frames' ids name, the one the last hello came from
  let epoch = EPOCH;
  const requests: { method: string; url: URL; body: unknown; keepalive: boolean }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init);
      const url = new URL(request.url);
      if (url.pathname.endsWith("/api/v0/live")) {
        let controller!: ReadableStreamDefaultController<Uint8Array>;
        const body = new ReadableStream<Uint8Array>({ start: (c) => void (controller = c) });
        request.signal.addEventListener("abort", () =>
          controller.error(new DOMException("aborted"))
        );
        send = (text) => controller.enqueue(new TextEncoder().encode(text));
        end = () => controller.close();
        return new Response(body);
      }
      const text = await request.text();
      const json = request.headers.get("content-type")?.includes("json") ?? false;
      const body = !text ? undefined : json ? JSON.parse(text) : text;
      requests.push({ method: request.method, url, body, keepalive: request.keepalive });
      const reply = await answer(url, request.method, body);
      if (reply === undefined) return new Response(null, { status: 404 });
      const shaped =
        reply && typeof reply === "object" && "status" in reply
          ? (reply as { status: number; body?: unknown })
          : { status: 200, body: reply };
      return new Response(shaped.body === undefined ? null : JSON.stringify(shaped.body), {
        status: shaped.status,
        headers: { "content-type": "application/json" }
      });
    })
  );
  return {
    requests,
    /** ends the open stream, as a lost connection does */
    drop() {
      end();
    },
    /** sends one frame on the open stream */
    frame<T extends FrameType>(type: T, data: FrameData<T>) {
      // a test's hello may leave the run out, which keeps the one before
      if (type === "hello") epoch = (data as FrameData<"hello">).epoch ?? epoch;
      seq += 1;
      send(frameText(seq, type, data, epoch));
    }
  };
};
