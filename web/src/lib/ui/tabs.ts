/** What the tabs of one browser tell each other: a tab that is there, one new, one that goes */
type TabMessage = { kind: "beat" | "hello" | "bye"; id: string };

/** The part of a BroadcastChannel the tabs talk over */
export type TabChannel = {
  postMessage: (message: TabMessage) => void;
  onmessage: ((event: MessageEvent<TabMessage>) => void) | null;
};

/** How often a tab says it is there */
export const BEAT_MS = 1_000;
/**
 * How long a tab not heard from still counts. Past a minute, as a browser runs the timers of a tab
 * hidden for a while once a minute; a tab that closes or freezes says so at once
 */
export const QUIET_MS = 70_000;
/** How long a new tab listens for the others before it may lead */
export const SETTLE_MS = 250;

/**
 * The tabs of this browser open on reseek, which agree on one of them to lead: the oldest that is
 * there, by when its page loaded. Each says it is there every `BEAT_MS` and goes when it closes,
 * freezes or is not heard from for `QUIET_MS`. A new tab asks the others to answer at once and
 * leads nothing until they have had `SETTLE_MS` to
 */
export class Tabs {
  /** sorts by when the page loaded, a random part keeping two loaded together apart */
  readonly id: string;
  /** the other tabs, by id, with when each was last heard from */
  readonly #heard = new Map<string, number>();
  readonly #channel: TabChannel;
  readonly #now: () => number;
  readonly #since: number;

  constructor(channel: TabChannel, loadedAt: number, now: () => number = Date.now) {
    this.id = `${String(Math.round(loadedAt)).padStart(16, "0")}-${Math.random().toString(36).slice(2)}`;
    this.#channel = channel;
    this.#now = now;
    this.#since = now();
    channel.onmessage = ({ data }) => this.#hear(data);
    this.#say("hello");
  }

  /** Says this tab is there; call every `BEAT_MS` */
  beat() {
    this.#say("beat");
  }

  /** This tab goes, as its page is hidden for good or frozen */
  leave() {
    this.#say("bye");
  }

  /** This tab is back, from the back-forward cache or a freeze */
  back() {
    this.#heard.clear();
    this.#say("hello");
  }

  /** Whether this tab leads: it has listened long enough and no older tab is there */
  leads(): boolean {
    const now = this.#now();
    if (now - this.#since < SETTLE_MS) return false;
    for (const [id, at] of this.#heard) {
      if (now - at > QUIET_MS) this.#heard.delete(id);
      else if (id < this.id) return false;
    }
    return true;
  }

  #hear(message: TabMessage) {
    if (message.kind === "bye") return void this.#heard.delete(message.id);
    this.#heard.set(message.id, this.#now());
    // a new tab hears every other at once, rather than after a beat
    if (message.kind === "hello") this.#say("beat");
  }

  #say(kind: TabMessage["kind"]) {
    this.#channel.postMessage({ kind, id: this.id });
  }
}

let tabs: Tabs | null | undefined;

/**
 * The tabs of this browser, made on first use; null where the browser has no BroadcastChannel,
 * where this tab is taken to be alone
 */
export const browserTabs = (): Tabs | null => {
  if (tabs !== undefined) return tabs;
  if (typeof BroadcastChannel === "undefined") return (tabs = null);
  const made = new Tabs(new BroadcastChannel("reseek-tabs"), performance.timeOrigin);
  setInterval(() => made.beat(), BEAT_MS);
  // a page kept in the back-forward cache, or frozen, comes back through pageshow or resume
  addEventListener("pagehide", () => made.leave());
  addEventListener("freeze", () => made.leave());
  addEventListener("pageshow", (e) => {
    if (e.persisted) made.back();
  });
  addEventListener("resume", () => made.back());
  return (tabs = made);
};
