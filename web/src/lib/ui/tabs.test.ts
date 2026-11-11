import { describe, expect, it } from "vitest";

import { BEAT_MS, QUIET_MS, SETTLE_MS, type TabChannel, Tabs } from "./tabs";

/** A BroadcastChannel among `Tabs` in one test: a message reaches every other tab, at once */
const bus = () => {
  const ends: TabChannel[] = [];
  return () => {
    const end: TabChannel = {
      onmessage: null,
      postMessage: (data) => {
        for (const other of ends) {
          if (other !== end) other.onmessage?.({ data } as MessageEvent);
        }
      }
    };
    ends.push(end);
    return end;
  };
};

/**
 * A clock for tabs in one test: `pass` moves it on a beat at a time, each tab in `beating` saying
 * it is there at every beat, as their timers do
 */
const time = () => {
  // a real clock's time, where 0 would read as never focused
  let now = 1_000_000;
  return {
    now: () => now,
    pass(ms: number, beating: Tabs[] = []) {
      const end = now + ms;
      while (now + BEAT_MS <= end) {
        now += BEAT_MS;
        for (const tab of beating) tab.beat();
      }
      now = end;
    }
  };
};

describe("the tabs of one browser", () => {
  it("agree on the first loaded to lead while none was focused, and on the next when it goes", () => {
    const clock = time();
    const channel = bus();
    const first = new Tabs(channel(), 100, clock.now);
    const second = new Tabs(channel(), 200, clock.now);
    first.join();
    clock.pass(10);
    second.join();
    // neither leads while it settles
    expect([first.leads(), second.leads()]).toEqual([false, false]);
    clock.pass(SETTLE_MS);
    expect([first.leads(), second.leads()]).toEqual([true, false]);
    first.leave();
    expect(second.leads()).toBe(true);
    // a tab that closes without a word counts until it is quiet too long
    const third = new Tabs(channel(), 50, clock.now);
    third.join();
    clock.pass(SETTLE_MS);
    expect([second.leads(), third.leads()]).toEqual([false, true]);
    // its last beat, then nothing, while the second goes on
    third.beat();
    clock.pass(QUIET_MS - 1, [second]);
    expect(second.leads()).toBe(false);
    clock.pass(2, [second]);
    expect(second.leads()).toBe(true);
  });

  it("hears the tabs already in at once when it joins", () => {
    const clock = time();
    const channel = bus();
    const open = new Tabs(channel(), 100, clock.now);
    open.join();
    clock.pass(SETTLE_MS);
    expect(open.leads()).toBe(true);
    const late = new Tabs(channel(), 300, clock.now);
    late.join();
    clock.pass(SETTLE_MS);
    // the open tab answered the new one's hello, so it knows it is not alone
    expect([open.leads(), late.leads()]).toEqual([true, false]);
  });

  it("leaves the lead to a tab that follows the stream, from one at the login", () => {
    const clock = time();
    const channel = bus();
    // the older tab shows the login: it never joins, or quits on signing out
    const login = new Tabs(channel(), 100, clock.now);
    const signedIn = new Tabs(channel(), 200, clock.now);
    signedIn.join();
    clock.pass(SETTLE_MS, [login, signedIn]);
    expect([login.leads(), signedIn.leads()]).toEqual([false, true]);
    login.join();
    clock.pass(SETTLE_MS, [login, signedIn]);
    expect([login.leads(), signedIn.leads()]).toEqual([true, false]);
    login.quit();
    clock.pass(BEAT_MS, [login, signedIn]);
    expect([login.leads(), signedIn.leads()]).toEqual([false, true]);
  });

  it("asks again once it has been silent longer than the others count it", () => {
    const clock = time();
    const channel = bus();
    const woken = new Tabs(channel(), 100, clock.now);
    const standIn = new Tabs(channel(), 200, clock.now);
    woken.join();
    standIn.join();
    // a long hide in which it beats changes nothing
    clock.pass(2 * QUIET_MS, [woken, standIn]);
    expect([woken.leads(), standIn.leads()]).toEqual([true, false]);
    // suspended just under the time from its last beat, while the other went on
    clock.pass(QUIET_MS + 1, [standIn]);
    expect(standIn.leads()).toBe(true);
    // woken before its timer runs: it asks again at once, the other leading while it settles,
    // and leads once settled
    expect(woken.leads()).toBe(false);
    expect(standIn.leads()).toBe(true);
    clock.pass(SETTLE_MS);
    expect([woken.leads(), standIn.leads()]).toEqual([true, false]);
  });

  it("asks again from its timer after a suspension, however it showed meanwhile", () => {
    const clock = time();
    const channel = bus();
    // loaded first, and in the background, so it was never seen to hide
    const end = channel();
    const asleep = new Tabs(end, 100, clock.now);
    const other = new Tabs(channel(), 200, clock.now);
    for (const tab of [asleep, other]) {
      tab.join();
      tab.connected(true);
    }
    clock.pass(SETTLE_MS, [asleep, other]);
    // suspended: it hears nothing and says nothing, while the other is focused
    const hear = end.onmessage;
    end.onmessage = null;
    clock.pass(QUIET_MS + BEAT_MS, [other]);
    other.focus();
    end.onmessage = hear;
    // its timer runs first on waking: it asks again rather than lead on what it last heard
    asleep.beat();
    expect([asleep.leads(), other.leads()]).toEqual([false, true]);
    clock.pass(SETTLE_MS, [asleep, other]);
    expect([asleep.leads(), other.leads()]).toEqual([false, true]);
  });

  it("let a focused tab joining beside the leader take over only as its stream connects", () => {
    const clock = time();
    const channel = bus();
    const leader = new Tabs(channel(), 100, clock.now);
    leader.join();
    leader.connected(true);
    leader.focus();
    clock.pass(SETTLE_MS, [leader]);
    // a new tab in the foreground, focused from the start, signed in
    const joining = new Tabs(channel(), 200, clock.now);
    joining.focus();
    joining.join();
    const pair = () => [leader.leads(), joining.leads()];
    expect(pair()).toEqual([true, false]);
    clock.pass(SETTLE_MS, [leader, joining]);
    expect(pair()).toEqual([true, false]);
    // its stream connects: it leads, and the leader stands down as it hears
    joining.connected(true);
    expect(pair()).toEqual([false, true]);
  });

  it("leave the leader playing while a focused tab joins and lands on the login", () => {
    const clock = time();
    const channel = bus();
    const leader = new Tabs(channel(), 100, clock.now);
    leader.join();
    leader.connected(true);
    leader.focus();
    clock.pass(SETTLE_MS, [leader]);
    const login = new Tabs(channel(), 200, clock.now);
    login.focus();
    login.join();
    expect(leader.leads()).toBe(true);
    // the session check finds no one signed in
    clock.pass(SETTLE_MS, [leader, login]);
    login.quit();
    expect([leader.leads(), login.leads()]).toEqual([true, false]);
  });

  it("leave the sound to the tab focused last, as focus moves", () => {
    const clock = time();
    const channel = bus();
    const older = new Tabs(channel(), 100, clock.now);
    const newer = new Tabs(channel(), 200, clock.now);
    for (const tab of [older, newer]) {
      tab.join();
      tab.connected(true);
    }
    clock.pass(SETTLE_MS, [older, newer]);
    newer.focus();
    expect([older.leads(), newer.leads()]).toEqual([false, true]);
    clock.pass(5_000, [older, newer]);
    older.focus();
    expect([older.leads(), newer.leads()]).toEqual([true, false]);
  });

  it("keep the lead with the focused tab when one opens in the background", () => {
    const clock = time();
    const channel = bus();
    // loaded first, but never focused: opened in the background, or left there
    const background = new Tabs(channel(), 100, clock.now);
    const focused = new Tabs(channel(), 200, clock.now);
    focused.join();
    focused.connected(true);
    focused.focus();
    clock.pass(SETTLE_MS, [focused]);
    background.join();
    background.connected(true);
    clock.pass(SETTLE_MS, [background, focused]);
    expect([background.leads(), focused.leads()]).toEqual([false, true]);
  });

  it("hand the lead to the tab focused last of the rest when the leader goes", () => {
    const clock = time();
    const channel = bus();
    const [first, second, third] = [100, 200, 300].map((at) => new Tabs(channel(), at, clock.now));
    for (const tab of [first, second, third]) {
      tab.join();
      tab.connected(true);
    }
    clock.pass(SETTLE_MS, [first, second, third]);
    third.focus();
    clock.pass(BEAT_MS, [first, second, third]);
    second.focus();
    expect([first, second, third].map((t) => t.leads())).toEqual([false, true, false]);
    // the leader closes: the one focused before it takes over, not the first loaded
    second.leave();
    expect([first.leads(), third.leads()]).toEqual([false, true]);
  });

  describe("as a tab already following asks again", () => {
    /** Two tabs following the stream, `first` focused last and so leading */
    const pair = () => {
      const clock = time();
      const channel = bus();
      const ends = [channel(), channel()];
      const [first, second] = ends.map((end, i) => new Tabs(end, 100 * (i + 1), clock.now));
      for (const tab of [first, second]) {
        tab.join();
        tab.connected(true);
      }
      clock.pass(SETTLE_MS, [first, second]);
      second.focus();
      clock.pass(BEAT_MS, [first, second]);
      first.focus();
      return { clock, first, second, ends };
    };

    it("lets a tab back from a freeze take over after its settle, focused within it", () => {
      const { clock, first, second } = pair();
      expect([second.leads(), first.leads()]).toEqual([false, true]);
      second.leave();
      clock.pass(5_000, [first]);
      second.back();
      clock.pass(SETTLE_MS / 2, [first]);
      second.focus();
      expect([second.leads(), first.leads()]).toEqual([false, true]);
      clock.pass(SETTLE_MS / 2 - 1);
      expect([second.leads(), first.leads()]).toEqual([false, true]);
      clock.pass(1);
      expect([second.leads(), first.leads()]).toEqual([true, false]);
    });

    it("lets a tab woken from sleep with the newest focus take over after its settle", () => {
      const { clock, first, second, ends } = pair();
      // the leader sleeps, hearing and saying nothing, until the other counts it gone
      const hear = ends[0].onmessage;
      ends[0].onmessage = null;
      clock.pass(QUIET_MS + BEAT_MS, [second]);
      expect(second.leads()).toBe(true);
      ends[0].onmessage = hear;
      // its timer asks again; the newest focus is still its own
      first.beat();
      expect([first.leads(), second.leads()]).toEqual([false, true]);
      clock.pass(SETTLE_MS - 1, [second]);
      expect([first.leads(), second.leads()]).toEqual([false, true]);
      clock.pass(1);
      expect([first.leads(), second.leads()]).toEqual([true, false]);
    });

    it("lets a tab retried from the lost page take over after its settle", () => {
      const { clock, first, second } = pair();
      // lost: it quits, while its stream still counts as connected, and the retry joins again
      first.quit();
      expect(second.leads()).toBe(true);
      first.join();
      expect([first.leads(), second.leads()]).toEqual([false, true]);
      clock.pass(SETTLE_MS - 1, [first, second]);
      expect([first.leads(), second.leads()]).toEqual([false, true]);
      clock.pass(1);
      expect([first.leads(), second.leads()]).toEqual([true, false]);
    });
  });

  it("put a connected tab above one whose stream is down, neither focused", () => {
    const clock = time();
    const channel = bus();
    // loaded first, its stream down
    const down = new Tabs(channel(), 100, clock.now);
    const up = new Tabs(channel(), 200, clock.now);
    down.join();
    up.join();
    up.connected(true);
    clock.pass(SETTLE_MS, [down, up]);
    expect([down.leads(), up.leads()]).toEqual([false, true]);
  });

  describe("after a suspension or a freeze", () => {
    /** Tabs x and y following the stream, each at its own end of the channel */
    const two = () => {
      const clock = time();
      const channel = bus();
      const ends = [channel(), channel()];
      const [x, y] = ends.map((end, i) => new Tabs(end, 100 * (i + 1), clock.now));
      for (const tab of [x, y]) {
        tab.join();
        tab.connected(true);
      }
      clock.pass(SETTLE_MS, [x, y]);
      return { clock, x, y, ends };
    };

    it("asks again as its stream is found dead, after a suspension with no word", () => {
      const { clock, x, y, ends } = two();
      y.focus();
      expect([x.leads(), y.leads()]).toEqual([false, true]);
      // suspended without a freeze: no bye, and it hears nothing while x is focused
      const hear = ends[1].onmessage;
      ends[1].onmessage = null;
      x.focus();
      clock.pass(QUIET_MS + BEAT_MS, [x]);
      ends[1].onmessage = hear;
      // back on screen, its stream found dead and connected again before its timer runs
      y.connected(false);
      expect([x.leads(), y.leads()]).toEqual([true, false]);
      y.connected(true);
      expect([x.leads(), y.leads()]).toEqual([true, false]);
      clock.pass(SETTLE_MS, [x, y]);
      expect([x.leads(), y.leads()]).toEqual([true, false]);
      clock.pass(5 * BEAT_MS, [x, y]);
      expect([x.leads(), y.leads()]).toEqual([true, false]);
    });

    it("asks again as it gains focus, after a suspension with no word", () => {
      const { clock, x, y, ends } = two();
      y.focus();
      // suspended without a freeze: no bye, and it hears nothing while x is focused
      const hear = ends[1].onmessage;
      ends[1].onmessage = null;
      x.focus();
      clock.pass(QUIET_MS + BEAT_MS, [x]);
      ends[1].onmessage = hear;
      // back on screen and focused before its timer runs: it asks, x leading while it settles
      y.focus();
      expect([x.leads(), y.leads()]).toEqual([true, false]);
      const leaders = () => [x.leads(), y.leads()].filter(Boolean).length;
      for (let at = 0; at < SETTLE_MS; at += 50) {
        expect(leaders()).toBe(1);
        clock.pass(50);
      }
      expect([x.leads(), y.leads()]).toEqual([false, true]);
    });

    it("keeps a leader as the clock jumps past QUIET_MS before either timer runs", () => {
      const { clock, x, y } = two();
      clock.pass(QUIET_MS + 1);
      x.beat();
      y.beat();
      const leaders = () => [x.leads(), y.leads()].filter(Boolean).length;
      for (let at = 0; at < SETTLE_MS; at += 50) {
        expect(leaders()).toBe(1);
        clock.pass(50);
      }
      expect([x.leads(), y.leads()]).toEqual([true, false]);
    });

    it("counts at once a tab back from a freeze whose focus comes before it asks", () => {
      const { clock, x, y } = two();
      y.focus();
      clock.pass(BEAT_MS, [x, y]);
      x.focus();
      expect([x.leads(), y.leads()]).toEqual([true, false]);
      // y freezes, saying bye; the browser hands it focus before it hears it resumed
      y.leave();
      clock.pass(5 * BEAT_MS, [x]);
      y.focus();
      expect([x.leads(), y.leads()]).toEqual([false, true]);
    });

    it("forgets a tab that closed while it was frozen, rather than defer to it", () => {
      const { clock, x, y, ends } = two();
      x.focus();
      expect([x.leads(), y.leads()]).toEqual([true, false]);
      // y frozen, hearing nothing, while x, the only other tab, closes
      const hear = ends[1].onmessage;
      ends[1].onmessage = null;
      y.leave();
      x.leave();
      ends[0].onmessage = null;
      clock.pass(5 * BEAT_MS);
      ends[1].onmessage = hear;
      y.back();
      clock.pass(SETTLE_MS, [y]);
      expect(y.leads()).toBe(true);
    });
  });
});
