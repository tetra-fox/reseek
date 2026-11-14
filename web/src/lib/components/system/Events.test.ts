import { mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { flushFaked } from "#lib/testing/flush.js";
import { press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { box } from "#lib/testing/state.svelte.js";

import Events, { RAISED_SETTLE_MS } from "./Events.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

/**
 * Lets the page take the events raised so far, which it reads once they settle. The wait is on
 * the test clock, from when the page heard the last of them
 */
const settled = async () => {
  await flushFaked();
  await vi.advanceTimersByTimeAsync(RAISED_SETTLE_MS);
  await flushFaked();
};

const event = (id: string, minute: number) => ({
  id,
  timestamp: `2026-10-26T10:${String(minute).padStart(2, "0")}:00Z`,
  type: "Noop",
  data: {}
});

describe("the event log", () => {
  let component: ReturnType<typeof mount> | null = null;
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
    vi.useRealTimers();
  });

  it("adds a raised event above the ones shown, and opens one with Enter", async () => {
    let log = [event("b", 2), event("a", 1)];
    const net = fakeNetwork(() => log);
    live.start();
    component = mount(Events, {
      target: document.body,
      props: { place: { page: 1, size: 25 }, types: [] }
    });
    await flushFaked();
    const first = document.querySelector('[data-line="b"]');
    log = [event("c", 3), ...log];
    net.frame("events.raised", event("c", 3) as never);
    await settled();
    expect(
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))
    ).toEqual(["c", "b", "a"]);
    expect(document.querySelector('[data-line="b"]')).toBe(first);

    const grid = document.querySelector("[role=grid]")!;
    press(grid, "ArrowDown");
    press(grid, "ArrowDown");
    press(grid, "Enter");
    await flushFaked();
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain("b at");
  });

  it("selects a row clicked on its summary, and opens it from the payload's control", async () => {
    fakeNetwork(() => [
      {
        ...event("a", 1),
        type: "RelayAgentConnected",
        data: { version: 0, agent: "nas", ipAddress: "10.0.0.2" }
      }
    ]);
    component = mount(Events, {
      target: document.body,
      props: { place: { page: 1, size: 25 }, types: [] }
    });
    await flushFaked();
    const summary = [...document.querySelectorAll('[data-line="a"] span')].find(
      (s) => s.textContent === "nas from 10.0.0.2"
    )!;
    summary.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await flushFaked();
    expect(ticked()).toEqual(["a"]);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    document
      .querySelector<HTMLButtonElement>('[data-line="a"] button[aria-label="Show full payload"]')!
      .click();
    await flushFaked();
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain("a at");
  });

  it("keeps the page moved to when a reload of the first page answers late", async () => {
    let hold: Promise<void> | null = null;
    const net = fakeNetwork(async (url) => {
      const offset = Number(url.searchParams.get("offset"));
      if (offset === 0 && hold) await hold;
      return offset === 0 ? [event("new", 3)] : [event("old", 1)];
    });
    live.start();
    const shown = box(1);
    component = mount(Events, {
      target: document.body,
      props: {
        get place() {
          return { page: shown.value, size: 1 };
        },
        types: []
      }
    });
    await flushFaked();
    let release!: () => void;
    hold = new Promise((done) => (release = done));
    net.frame("events.raised", event("new", 3) as never);
    await settled();
    shown.value = 2;
    await flushFaked();
    release();
    await flushFaked();
    expect(
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))
    ).toEqual(["old"]);
  });

  it("reads the first page once for a burst of raised events", async () => {
    const net = fakeNetwork(() => [event("a", 1)]);
    live.start();
    component = mount(Events, {
      target: document.body,
      props: { place: { page: 1, size: 25 }, types: [] }
    });
    await flushFaked();
    const reads = () => net.requests.filter((r) => r.url.pathname.endsWith("/events")).length;
    const before = reads();
    for (let n = 0; n < 20; n++) net.frame("events.raised", event(`e${n}`, 2) as never);
    await settled();
    expect(reads()).toBe(before + 1);
  });
});
