import { describe, expect, it } from "vitest";

import { QUIET_MS, SETTLE_MS, type TabChannel, Tabs } from "./tabs";

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

describe("the tabs of one browser", () => {
  it("agree on the oldest to lead, and on the next when it goes", () => {
    let now = 0;
    const clock = () => now;
    const channel = bus();
    const first = new Tabs(channel(), 100, clock);
    now = 10;
    const second = new Tabs(channel(), 200, clock);
    // neither leads while it settles
    expect([first.leads(), second.leads()]).toEqual([false, false]);
    now += SETTLE_MS;
    expect([first.leads(), second.leads()]).toEqual([true, false]);
    first.leave();
    expect(second.leads()).toBe(true);
    // a tab that closes without a word counts until it is quiet too long
    const third = new Tabs(channel(), 50, clock);
    now += SETTLE_MS;
    expect([second.leads(), third.leads()]).toEqual([false, true]);
    // its last beat, then nothing
    third.beat();
    now += QUIET_MS - 1;
    expect(second.leads()).toBe(false);
    now += 2;
    expect(second.leads()).toBe(true);
  });

  it("hears the tabs already open at once when it opens", () => {
    let now = 0;
    const clock = () => now;
    const channel = bus();
    const open = new Tabs(channel(), 100, clock);
    now += SETTLE_MS;
    expect(open.leads()).toBe(true);
    const late = new Tabs(channel(), 300, clock);
    now += SETTLE_MS;
    // the open tab answered the new one's hello, so it knows it is not alone
    expect([open.leads(), late.leads()]).toEqual([true, false]);
  });
});
