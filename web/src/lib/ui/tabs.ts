/**
 * What the tabs of one browser tell each other: a tab that is there, one that asks the others to
 * answer, one that goes, each with when the tab was last focused, 0 for never or while its stream
 * is not connected, and whether its stream is
 */
type TabMessage = {
  kind: "beat" | "hello" | "bye";
  id: string;
  focusedAt: number;
  connected: boolean;
};

/** A tab as the election ranks it */
type Ranked = { id: string; focusedAt: number; connected: boolean };

/**
 * Whether `a` ranks above `b`: focused more recently; at the same focus, or neither ever, the one
 * whose stream is connected; then the one loaded earlier
 */
const outranks = (a: Ranked, b: Ranked) => {
  if (a.focusedAt !== b.focusedAt) return a.focusedAt > b.focusedAt;
  if (a.connected !== b.connected) return a.connected;
  return a.id < b.id;
};

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
 * one focused most recently, else, while none has been, one whose stream is connected, the one of
 * those loaded first. A tab joins as its shell starts following the stream and quits when it
 * stops, as at the login. While in, it says it is there every `BEAT_MS` and goes when it closes,
 * freezes or is not heard from for `QUIET_MS`. Its focus counts only while its stream is
 * connected.
 *
 * A tab that asks the others to answer, as it joins, comes back or wakes, leads nothing for
 * `SETTLE_MS`, the time they have to answer, and the others rank it nothing for `SETTLE_MS` from
 * hearing it ask, so the asker leads at the end of its settle and the one before stands down at
 * the end of its own count of that settle, one crossing later. A tab that becomes first by a
 * focus, or as its stream connects, says so at once unless it has been silent longer than the
 * others count it, when it asks instead as above. Said at once, it leads at once and the one
 * before stands down when that message reaches it. Either way both lead for the time a message
 * takes to cross the channel, and a hand-over leaves no time where neither does. Some times have
 * no leader all the same: a lone tab's settle as it asks again, every tab asking in the same
 * instant, and the `QUIET_MS` a leader that went without a word is still counted
 */
export class Tabs {
  /** sorts by when the page loaded, a random part keeping two loaded together apart */
  readonly id: string;
  /**
   * the other tabs, by id: when each was last heard from, when it last asked the others to answer
   * (it ranks nothing for `SETTLE_MS` after), and what it ranks by
   */
  readonly #heard = new Map<
    string,
    { at: number; asked: number; focusedAt: number; connected: boolean }
  >();
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
  /** this tab's stream is connected, so it can play and its focus counts */
  #connected = false;

  // `Date` is looked up at each call: `Date.now` itself would keep the clock in place when the
  // tab was made, not one a test fakes later
  constructor(channel: TabChannel, loadedAt: number, now: () => number = () => Date.now()) {
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

  /** This tab gains focus, which puts it first once its stream is connected */
  focus() {
    this.#focusedAt = this.#now();
    if (this.#in && this.#connected) this.#speak();
  }

  /** This tab's stream is connected, or no longer is; the others hear at once */
  connected(on: boolean) {
    if (on === this.#connected) return;
    this.#connected = on;
    if (this.#in) this.#speak();
  }

  /** This tab no longer takes part, as its shell stops following the stream */
  quit() {
    if (!this.#in) return;
    this.#in = false;
    this.#say("bye");
  }

  /** How long until this tab has listened long enough to lead, in ms, at most `SETTLE_MS` */
  settling(): number {
    // a clock set back since the tab asked would otherwise make it longer
    return Math.min(SETTLE_MS, Math.max(0, SETTLE_MS - (this.#now() - this.#since)));
  }

  /**
   * Says this tab is there; call every `BEAT_MS`. Silent longer than the others count a tab, as
   * after a suspension whether it was shown or hidden, it asks again instead
   */
  beat() {
    if (this.#in) this.#speak();
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
    const me = { id: this.id, focusedAt: this.#counted(), connected: this.#connected };
    for (const [id, other] of this.#heard) {
      if (now - other.at > QUIET_MS) this.#heard.delete(id);
      // a tab that asked a moment ago is settling and plays nothing yet
      else if (now - other.asked >= SETTLE_MS && outranks({ id, ...other }, me)) return false;
    }
    return true;
  }

  /** the focus this tab ranks by: none while its stream is not connected */
  #counted() {
    return this.#connected ? this.#focusedAt : 0;
  }

  /**
   * Says this tab is there, or, silent longer than the others count a tab, asks again: whatever
   * it says first after a suspension, a focus, its stream or its timer, the others may have
   * dropped it and it may have missed them
   */
  #speak() {
    if (this.#dropped()) this.#ask();
    else this.#say("beat");
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
    const now = this.#now();
    // a beat from a tab not asking keeps when it asked, or counts it at once
    const asked =
      message.kind === "hello" ? now : (this.#heard.get(message.id)?.asked ?? -Infinity);
    const { focusedAt, connected } = message;
    this.#heard.set(message.id, { at: now, asked, focusedAt, connected });
    // a tab that asks hears every other at once, rather than after a beat
    if (message.kind === "hello" && this.#in) this.#say("beat");
  }

  #say(kind: TabMessage["kind"]) {
    this.#spoke = this.#now();
    this.#channel.postMessage({
      kind,
      id: this.id,
      focusedAt: this.#counted(),
      connected: this.#connected
    });
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
