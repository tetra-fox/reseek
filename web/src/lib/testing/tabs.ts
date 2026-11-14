import { BEAT_MS, type TabChannel, type Tabs } from "#lib/ui/tabs.js";

/** A BroadcastChannel among `Tabs` in one test: a message reaches every other tab, at once */
export const bus = () => {
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
export const time = () => {
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
