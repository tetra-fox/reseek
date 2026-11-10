/**
 * What the tabs of one browser tell each other: a tab that is there, one new, one that goes, each
 * with when the tab was last focused, 0 for never
 */
type TabMessage = { kind: "beat" | "hello" | "bye"; id: string; focusedAt: number };

/** A tab as the election ranks it */
type Ranked = { id: string; focusedAt: number };

/**
 * Whether `a` ranks above `b`: focused more recently, or, focused at the same time or neither
 * ever, loaded earlier
 */
const outranks = (a: Ranked, b: Ranked) =>
  a.focusedAt !== b.focusedAt ? a.focusedAt > b.focusedAt : a.id < b.id;

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
 * The tabs of this browser that follow the live stream, which agree on one of them to lead: the
 * one focused most recently, else, while none has been, the one loaded first. A tab joins as its
 * shell starts following the stream and quits when it stops, as at the login. While in, it says
 * it is there every `BEAT_MS` and at once as it gains focus, and goes when it closes, freezes or
 * is not heard from for `QUIET_MS`. A tab that joins asks the others to answer at once, and leads
 * nothing for `SETTLE_MS`, the time they have to answer. As focus moves, the tab focused leads at
 * once and the one before stands down when that tab's beat reaches it, so both lead for the time
 * the beat takes to cross the channel
 */
export class Tabs {
  /** sorts by when the page loaded, a random part keeping two loaded together apart */
  readonly id: string;
  /** the other tabs, by id, with when each was last heard from and last focused */
  readonly #heard = new Map<string, { at: number; focusedAt: number }>();
  readonly #channel: TabChannel;
  readonly #now: () => number;
  /** this tab follows the stream and takes part */
  #in = false;
  /** when this tab last asked the others to answer; it leads nothing for `SETTLE_MS` after */
  #since = 0;
  /** when this tab last spoke, which is what the others count it by */
  #spoke = 0;
  /** when this tab last gained focus, 0 for never */
  #focusedAt = 0;

  constructor(channel: TabChannel, loadedAt: number, now: () => number = Date.now) {
    this.id = `${String(Math.round(loadedAt)).padStart(16, "0")}-${Math.random().toString(36).slice(2)}`;
    this.#channel = channel;
    this.#now = now;
    channel.onmessage = ({ data }) => this.#hear(data);
  }

  /** This tab takes part: it asks the others to answer, and leads nothing for `SETTLE_MS` */
  join() {
    this.#in = true;
    this.#ask();
  }

  /** This tab gains focus, which puts it first; the others hear of it at once */
  focus() {
    this.#focusedAt = this.#now();
    if (this.#in) this.#say("beat");
  }

  /** This tab no longer takes part, as its shell stops following the stream */
  quit() {
    if (!this.#in) return;
    this.#in = false;
    this.#say("bye");
  }

  /** How long until this tab has listened long enough to lead, in ms */
  settling(): number {
    return Math.max(0, SETTLE_MS - (this.#now() - this.#since));
  }

  /**
   * Says this tab is there; call every `BEAT_MS`. Silent longer than the others count it, as a
   * suspended tab is whatever it showed, it asks again instead
   */
  beat() {
    if (!this.#in) return;
    if (this.#dropped()) this.#ask();
    else this.#say("beat");
  }

  /** This tab goes for now, as its page is hidden for good or frozen */
  leave() {
    if (this.#in) this.#say("bye");
  }

  /** This tab is back, from the back-forward cache or a freeze: it asks again */
  back() {
    if (this.#in) this.#ask();
  }

  /**
   * Whether this tab leads: it takes part, has listened long enough and no tab there was focused
   * more recently
   */
  leads(): boolean {
    if (!this.#in) return false;
    // woken from a suspension before its timer runs: the others have dropped it
    if (this.#dropped()) this.#ask();
    const now = this.#now();
    if (now - this.#since < SETTLE_MS) return false;
    const me = { id: this.id, focusedAt: this.#focusedAt };
    for (const [id, { at, focusedAt }] of this.#heard) {
      if (now - at > QUIET_MS) this.#heard.delete(id);
      else if (outranks({ id, focusedAt }, me)) return false;
    }
    return true;
  }

  /** whether this tab has been silent longer than the others count it */
  #dropped() {
    return this.#now() - this.#spoke > QUIET_MS;
  }

  #ask() {
    this.#heard.clear();
    this.#since = this.#now();
    this.#say("hello");
  }

  #hear(message: TabMessage) {
    if (message.kind === "bye") return void this.#heard.delete(message.id);
    this.#heard.set(message.id, { at: this.#now(), focusedAt: message.focusedAt });
    // a tab that joins hears every other at once, rather than after a beat
    if (message.kind === "hello" && this.#in) this.#say("beat");
  }

  #say(kind: TabMessage["kind"]) {
    this.#spoke = this.#now();
    this.#channel.postMessage({ kind, id: this.id, focusedAt: this.#focusedAt });
  }
}

let tabs: Tabs | null | undefined;

/**
 * The tabs of this browser, made on the first call and the same after; null where the browser has
 * no BroadcastChannel, where this tab is taken to be alone
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
  // focus comes on a switch of tabs or windows; the page shown already focused hears no event
  addEventListener("focus", () => made.focus());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && document.hasFocus()) made.focus();
  });
  if (document.hasFocus()) made.focus();
  return (tabs = made);
};
