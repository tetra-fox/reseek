import type { Schemas } from "#lib/api/client.js";

import { SseParser } from "./sse";

export type LiveFrame = Schemas["LiveFrame"];
export type FrameType = LiveFrame["type"];
/** The data a frame of `type` carries */
export type FrameData<T extends FrameType> = Extract<LiveFrame, { type: T }>["data"];

/** A topic of the event stream, as `GET /live?topics=` names them */
export type Topic =
  | "application"
  | "logs"
  | "metrics"
  | "search"
  | `search:${string}`
  | "transfers"
  | "jobs"
  | "conversations"
  | "rooms"
  | "public_rooms"
  | "users"
  | "events"
  | "activity";

/** The order topics are asked for in, so one set of topics is always the same request */
const ORDER = [
  "application",
  "metrics",
  "logs",
  "search",
  "transfers",
  "jobs",
  "conversations",
  "rooms",
  "public_rooms",
  "users",
  "events",
  "activity"
];
const rank = (topic: string) => {
  const i = ORDER.indexOf(topic.startsWith("search:") ? "search" : topic);
  return i === -1 ? ORDER.length : i;
};
export const ordered = (topics: Iterable<Topic>): Topic[] =>
  [...topics].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));

/** How one stream ended, which decides what the store does next */
export type End =
  | { kind: "unauthorized" }
  /** the server refused the stream for this principal or set of topics; polling takes over */
  | { kind: "refused"; status: number; retryAfterMs: number | null }
  /** the connection failed or ended, or the server said goodbye for a reason other than auth */
  | { kind: "lost" };

type Callbacks = {
  /** a frame arrived; `id` is its sequence number */
  frame: (frame: LiveFrame, id: string | null) => void;
  ended: (end: End) => void;
};

/** How long a stream may stay silent before it is taken for dead; the server pings every 15 s */
export const SILENCE_MS = 45_000;

/**
 * One `GET /live` request read as an event stream. It is read with `fetch` rather than an
 * `EventSource`, which can't send `Last-Event-ID` on a stream it opens itself and doesn't say
 * what status a refused stream was answered with
 */
export class Stream {
  readonly topics: Topic[];
  /** while false the stream is read but its frames are dropped, for a stream being replaced */
  dispatching = true;
  #abort = new AbortController();
  #silence: ReturnType<typeof setTimeout> | undefined;
  #ended = false;
  #callbacks: Callbacks;

  constructor(
    url: string,
    topics: Topic[],
    lastId: string | null,
    fetcher: typeof fetch,
    callbacks: Callbacks
  ) {
    this.topics = topics;
    this.#callbacks = callbacks;
    const headers: Record<string, string> = { Accept: "text/event-stream" };
    if (lastId !== null) headers["Last-Event-ID"] = lastId;
    this.#hearing();
    void this.#read(
      fetcher(url, { headers, credentials: "same-origin", signal: this.#abort.signal })
    );
  }

  /** Stops reading without reporting an end */
  close() {
    this.#ended = true;
    clearTimeout(this.#silence);
    this.#abort.abort();
  }

  #hearing() {
    clearTimeout(this.#silence);
    this.#silence = setTimeout(() => this.#end({ kind: "lost" }), SILENCE_MS);
  }

  #end(end: End) {
    if (this.#ended) return;
    this.close();
    this.#callbacks.ended(end);
  }

  async #read(request: Promise<Response>) {
    let response: Response;
    try {
      response = await request;
    } catch {
      this.#end({ kind: "lost" });
      return;
    }
    if (response.status === 401) return this.#end({ kind: "unauthorized" });
    if (!response.ok || !response.body) {
      const after = Number(response.headers.get("Retry-After"));
      const retryAfterMs = Number.isFinite(after) && after > 0 ? after * 1000 : null;
      if (response.status === 403 || response.status === 429) {
        return this.#end({ kind: "refused", status: response.status, retryAfterMs });
      }
      return this.#end({ kind: "lost" });
    }
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    const parser = new SseParser();
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done || this.#ended) break;
        this.#hearing();
        for (const event of parser.push(value)) {
          if (this.#ended) break;
          this.#event(event.event, event.data, event.id);
        }
      }
    } catch {
      // the connection broke, or close() aborted it
    }
    this.#end({ kind: "lost" });
  }

  #event(type: string, data: string, id: string | null) {
    let frame: LiveFrame;
    try {
      frame = { type, data: JSON.parse(data) } as LiveFrame;
    } catch {
      // a frame that isn't JSON can't be one of ours; the rest of the stream is still good
      return;
    }
    if (frame.type === "bye") {
      return this.#end(
        frame.data.reason === "unauthorized" ? { kind: "unauthorized" } : { kind: "lost" }
      );
    }
    if (this.dispatching) this.#callbacks.frame(frame, id);
  }
}
