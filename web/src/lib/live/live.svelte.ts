import { root, sessionGone } from "#lib/api/client.js";
import { toast } from "#lib/toast.svelte.js";

import {
  type End,
  type FrameData,
  type FrameType,
  type LiveFrame,
  ordered,
  Stream,
  type Topic
} from "./stream";

export type { FrameData, FrameType, LiveFrame, Topic };

export type LiveStatus = "off" | "connecting" | "connected" | "polling";

/** How often the shell and screens poll while the stream is down */
export const FALLBACK_MS = 5_000;
/** A topic change waits this long, so a navigation that drops one topic and adds another reopens once */
export const DEBOUNCE_MS = 250;
const RETRY_MS = 1_000;
const MAX_RETRY_MS = 30_000;
/** Three failures within this window switch from the plain retry to the growing one */
const BURST_MS = 10_000;

type Deps = {
  fetch: typeof fetch;
  /** the URL of the stream for `topics` */
  url: (topics: Topic[]) => string;
  /** the stream was refused 401 or said goodbye for that reason: the session is gone */
  unauthorized: () => void;
  /** a line the user should see about the connection */
  notice: (text: string, kind: "info" | "error") => void;
};

const defaults = (): Deps => ({
  fetch: (...args) => fetch(...args),
  url: (topics) => `${root()}/api/v0/live?topics=${topics.map(encodeURIComponent).join(",")}`,
  unauthorized: sessionGone,
  notice: (text, kind) => toast(text, kind)
});

type Handler<T extends FrameType> = (data: FrameData<T>) => void;

/**
 * The one event stream of the tab. Screens `retain` the topics they need while mounted and
 * listen with `on`; the stream asks for the union of what is retained, and when that changes
 * it opens a stream with the new topics, from the last frame seen, before closing the old one.
 * Snapshot frames arrive first and replace a store's contents; later frames patch it
 */
export class Live {
  status = $state<LiveStatus>("off");
  /** who the stream is for, from its first frame */
  principal = $state.raw<FrameData<"hello">["principal"] | null>(null);
  /** when the next attempt to reopen a lost stream is made, while one is waiting */
  retryAt = $state<number | null>(null);

  #deps: Deps;
  #wanted = new Map<Topic, number>();
  #handlers = new Map<FrameType, Set<(data: unknown) => void>>();
  #stream: Stream | null = null;
  #replacing: Stream | null = null;
  #lastId: string | null = null;
  #running = false;
  /** when the recent losses happened, to tell a burst from an occasional drop */
  #failures: number[] = [];
  /** losses since the last greeting once a burst started the growing wait, else 0 */
  #streak = 0;
  #retry: ReturnType<typeof setTimeout> | undefined;
  #debounce: ReturnType<typeof setTimeout> | undefined;
  #warned = false;

  constructor(deps: Partial<Deps> = {}) {
    this.#deps = { ...defaults(), ...deps };
  }

  /** The sequence number of the last frame handed to a listener */
  get lastId() {
    return this.#lastId;
  }

  /** How often pages should poll right now, `null` while frames keep them current */
  get pollMs(): number | null {
    return this.status === "connected" ? null : FALLBACK_MS;
  }

  /** Opens the stream for what is retained, if it isn't open */
  start() {
    if (this.#running) return;
    this.#running = true;
    this.#open();
  }

  /** Closes everything and forgets the last frame, as at logout */
  stop() {
    this.#shut();
    this.#lastId = null;
    this.principal = null;
  }

  /** Asks for `topic` until the returned function is called; counted, so two screens can share one */
  retain(topic: Topic): () => void {
    this.#wanted.set(topic, (this.#wanted.get(topic) ?? 0) + 1);
    this.#topicsChanged();
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const count = (this.#wanted.get(topic) ?? 1) - 1;
      if (count > 0) this.#wanted.set(topic, count);
      else this.#wanted.delete(topic);
      this.#topicsChanged();
    };
  }

  /** Calls `handler` with every frame of `type` until the returned function is called */
  on<T extends FrameType>(type: T, handler: Handler<T>): () => void {
    const set = this.#handlers.get(type) ?? new Set();
    this.#handlers.set(type, set);
    const wrapped = handler as (data: unknown) => void;
    set.add(wrapped);
    return () => set.delete(wrapped);
  }

  /** The topics asked for right now, in the order the stream asks for them */
  get topics(): Topic[] {
    return ordered(this.#wanted.keys());
  }

  #topicsChanged() {
    if (!this.#running) return;
    clearTimeout(this.#debounce);
    this.#debounce = setTimeout(() => {
      const now = this.topics.join(",");
      const open = (this.#replacing ?? this.#stream)?.topics.join(",");
      if (now !== open && this.status === "connected") this.#replace();
    }, DEBOUNCE_MS);
  }

  #open() {
    clearTimeout(this.#retry);
    this.retryAt = null;
    if (this.status !== "polling") this.status = "connecting";
    this.#stream = this.#connect();
  }

  /** A stream for the new topics, while the old one stays open but silent until the new one greets */
  #replace() {
    const old = this.#stream;
    if (old) old.dispatching = false;
    this.#replacing?.close();
    this.#replacing = this.#connect();
  }

  #connect(): Stream {
    const topics = this.topics;
    const stream: Stream = new Stream(
      this.#deps.url(topics),
      topics,
      this.#lastId,
      this.#deps.fetch,
      {
        frame: (frame, id) => this.#frame(stream, frame, id),
        ended: (end) => this.#ended(stream, end)
      }
    );
    return stream;
  }

  #frame(stream: Stream, frame: LiveFrame, id: string | null) {
    if (frame.type === "hello") {
      if (stream === this.#replacing) {
        this.#stream?.close();
        this.#stream = stream;
        this.#replacing = null;
      }
      this.principal = frame.data.principal;
      this.#failures = [];
      this.#streak = 0;
      if (this.#warned) this.#deps.notice("Live connection back", "info");
      this.#warned = false;
      this.status = "connected";
      // the topics changed while this stream was opening
      if (stream.topics.join(",") !== this.topics.join(",")) this.#topicsChanged();
    }
    if (id !== null) this.#lastId = id;
    const handlers = this.#handlers.get(frame.type);
    if (!handlers) return;
    for (const handler of handlers) handler(frame.data);
  }

  #ended(stream: Stream, end: End) {
    if (stream === this.#replacing) {
      // the new topics failed to open: the old stream takes over again and the next attempt
      // goes through the usual retry
      this.#replacing = null;
      if (this.#stream) this.#stream.dispatching = true;
      if (end.kind === "unauthorized") return this.#unauthorized();
      this.#stream?.close();
      this.#stream = null;
      return this.#lost(end);
    }
    if (stream !== this.#stream) return;
    this.#stream = null;
    if (end.kind === "unauthorized") return this.#unauthorized();
    this.#lost(end);
  }

  #unauthorized() {
    this.stop();
    this.#deps.unauthorized();
  }

  #lost(end: End) {
    if (!this.#running) return;
    const now = Date.now();
    this.#failures = [...this.#failures.filter((t) => now - t < BURST_MS), now];
    // three losses within the window start the growing wait; from then on every loss until the
    // stream greets again doubles it, however far apart the losses fall
    if (this.#streak > 0 || this.#failures.length >= 3) this.#streak += 1;
    let delay = this.#streak > 0 ? Math.min(RETRY_MS * 2 ** this.#streak, MAX_RETRY_MS) : RETRY_MS;
    if (end.kind === "refused" && end.retryAfterMs !== null)
      delay = Math.max(delay, end.retryAfterMs);
    this.status = "polling";
    if (!this.#warned) {
      this.#deps.notice(
        "Couldn't keep a live connection to reseek, polling until it's back",
        "error"
      );
    }
    this.#warned = true;
    this.retryAt = now + delay;
    this.#retry = setTimeout(() => this.#open(), delay);
  }

  #shut() {
    this.#running = false;
    clearTimeout(this.#retry);
    clearTimeout(this.#debounce);
    this.retryAt = null;
    this.#stream?.close();
    this.#replacing?.close();
    this.#stream = null;
    this.#replacing = null;
    this.#failures = [];
    this.#streak = 0;
    this.#warned = false;
    this.status = "off";
  }
}

export const live = new Live();
