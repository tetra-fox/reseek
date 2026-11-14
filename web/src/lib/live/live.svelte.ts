import { root, sessionGone } from "#lib/api/client.js";
import { toast } from "#lib/feedback/toast.svelte.js";

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

/**
 * Where the stream is. `polling` waits to open again after it was lost; `rejected` is not tried
 * again until the topics change, as the daemon refused the request itself. Pages poll in both
 */
export type LiveStatus = "off" | "connecting" | "connected" | "polling" | "rejected";

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
  url: (topics: Topic[], reopen: boolean) => string;
  /** the stream was refused 401 or said goodbye for that reason: the session is gone */
  unauthorized: () => void;
  /** a line the user should see about the connection */
  notice: (text: string, kind: "info" | "error") => void;
};

const defaults = (): Deps => ({
  fetch: (...args) => fetch(...args),
  url: (topics, reopen) =>
    `${root()}/api/v0/live?topics=${topics.map(encodeURIComponent).join(",")}${reopen ? "&reopen=true" : ""}`,
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
  /** when the next attempt to reopen a lost stream is made, while one is waiting */
  retryAt = $state<number | null>(null);
  /** why the daemon refused the stream, while it is `rejected` */
  rejection = $state<string | null>(null);

  #deps: Deps;
  #wanted = new Map<Topic, number>();
  #handlers = new Map<FrameType, Set<(data: unknown) => void>>();
  #added = new Set<(topics: Topic[]) => void>();
  #missed = new Set<() => void>();
  /** the topics of the stream that last greeted, whose frames are being handed over */
  #following: Topic[] = [];
  #stream: Stream | null = null;
  #replacing: Stream | null = null;
  #lastId: string | null = null;
  #epoch: string | null = null;
  /** a stream greeted since `start`, so a later one that does not resume has missed frames */
  #greeted = false;
  /** the stream whose greeting already said frames were missed, so its `lagged` says it again */
  #missedOn: Stream | null = null;
  /** the topics the daemon refused, while `rejected` */
  #rejectedFor: string | null = null;
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

  /** The run of the daemon the last stream greeted from, new each time it starts */
  get epoch() {
    return this.#epoch;
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
    this.#epoch = null;
    this.#greeted = false;
    this.#following = [];
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

  /**
   * Calls `handler` with the topics a resumed stream follows that the one before it did not:
   * nothing was sent for them before the resume point, so their stores read again
   */
  onAdded(handler: (topics: Topic[]) => void): () => void {
    this.#added.add(handler);
    return () => this.#added.delete(handler);
  }

  /**
   * Calls `handler` whenever frames may have been missed: a stream greeted without resuming
   * after another had greeted, or the daemon said `lagged`. A store a snapshot does not fill
   * reads again then
   */
  onMissed(handler: () => void): () => void {
    this.#missed.add(handler);
    return () => this.#missed.delete(handler);
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
      // other topics may be ones the daemon takes
      if (this.status === "rejected") {
        if (now !== this.#rejectedFor) this.#open();
        return;
      }
      const open = (this.#replacing ?? this.#stream)?.topics.join(",");
      if (now !== open && this.status === "connected") this.#replace();
    }, DEBOUNCE_MS);
  }

  /** Opens the stream; `reopen` says it follows the close of the stream before, whose place it may need */
  #open(reopen = false) {
    clearTimeout(this.#retry);
    this.retryAt = null;
    this.rejection = null;
    this.#rejectedFor = null;
    if (this.status !== "polling") this.status = "connecting";
    this.#stream = this.#connect(reopen);
  }

  /** A stream for the new topics, while the old one stays open but silent until the new one greets */
  #replace() {
    const old = this.#stream;
    if (old) old.dispatching = false;
    this.#replacing?.close();
    this.#replacing = this.#connect(false);
  }

  #connect(reopen: boolean): Stream {
    const topics = this.topics;
    const stream: Stream = new Stream(
      this.#deps.url(topics, reopen),
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
      const missed = !frame.data.resumed && this.#greeted;
      this.#greeted = true;
      this.#missedOn = missed ? stream : null;
      this.#epoch = frame.data.epoch;
      this.#failures = [];
      this.#streak = 0;
      if (this.#warned) this.#deps.notice("Live connection restored", "info");
      this.#warned = false;
      this.status = "connected";
      // the topics changed while this stream was opening
      if (stream.topics.join(",") !== this.topics.join(",")) this.#topicsChanged();
      const added = frame.data.resumed
        ? stream.topics.filter((topic) => !this.#following.includes(topic))
        : [];
      this.#following = stream.topics;
      if (added.length) for (const handler of this.#added) this.#call(handler, added);
      if (missed) for (const handler of this.#missed) this.#call(handler);
    }
    if (frame.type === "lagged" && stream !== this.#missedOn) {
      for (const handler of this.#missed) this.#call(handler);
    }
    if (id !== null) this.#lastId = id;
    const handlers = this.#handlers.get(frame.type);
    if (!handlers) return;
    for (const handler of handlers) this.#call(handler, frame.data);
  }

  /**
   * Runs one listener, so a listener that throws leaves the others to run and the stream to go
   * on. What it threw is a fault of the page: the console says what
   */
  #call<A extends unknown[]>(handler: (...args: A) => void, ...args: A) {
    try {
      handler(...args);
    } catch (error) {
      // oxlint-disable-next-line no-console
      console.error("a live update could not be taken", error);
    }
  }

  #ended(stream: Stream, end: End) {
    if (stream === this.#replacing) {
      this.#replacing = null;
      if (end.kind === "unauthorized") return this.#unauthorized();
      if (end.kind === "rejected") return this.#rejected(end);
      // the new topics failed to open. The old stream has dropped its frames since the change
      // began, so it is closed and a stream for the new topics opens from the last frame taken,
      // which brings them again. The old one was working: this is no lost connection, and a
      // stream that fails again from here is. At the stream cap the reopen needs the old one's
      // place, which the daemon frees once it sees the close, so it asks to wait for it
      this.#stream?.close();
      this.#stream = null;
      return this.#open(true);
    }
    if (stream !== this.#stream) return;
    this.#stream = null;
    if (end.kind === "unauthorized") return this.#unauthorized();
    if (end.kind === "rejected") return this.#rejected(end);
    // the stream for the new topics replays from the last frame too, so it is the one being
    // opened now, and a retry is only waited for while no stream is open or opening
    if (this.#replacing) {
      this.#stream = this.#replacing;
      this.#replacing = null;
      this.status = "connecting";
      return;
    }
    this.#lost(end);
  }

  #unauthorized() {
    this.stop();
    this.#deps.unauthorized();
  }

  /**
   * The daemon refused the request itself, which asking again for the same topics would not
   * change: no retry, and pages poll, until the topics change
   */
  #rejected(end: Extract<End, { kind: "rejected" }>) {
    this.#stream?.close();
    this.#replacing?.close();
    this.#stream = null;
    this.#replacing = null;
    this.#rejectedFor = this.topics.join(",");
    this.rejection = end.reason;
    this.status = "rejected";
    // oxlint-disable-next-line no-console
    console.error(`the event stream was refused (HTTP ${end.status}): ${end.reason}`);
  }

  #lost(end: End) {
    if (!this.#running) return;
    const now = Date.now();
    this.#failures = [...this.#failures.filter((t) => now - t < BURST_MS), now];
    // once the growing wait starts, every loss until the stream greets again doubles it, however
    // far apart the losses fall
    if (this.#streak > 0 || this.#failures.length >= 3) this.#streak += 1;
    let delay = this.#streak > 0 ? Math.min(RETRY_MS * 2 ** this.#streak, MAX_RETRY_MS) : RETRY_MS;
    if (end.kind === "refused" && end.retryAfterMs !== null)
      delay = Math.max(delay, end.retryAfterMs);
    this.status = "polling";
    if (!this.#warned) {
      this.#deps.notice("Live connection to reseek lost, refreshing until it is back", "error");
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
    this.rejection = null;
    this.#rejectedFor = null;
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
