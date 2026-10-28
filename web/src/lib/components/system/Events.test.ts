import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { active, press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Events from "./Events.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const flush = async () => {
  for (let i = 0; i < 3; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};
const event = (id: string, minute: number) => ({
  id,
  timestamp: `2026-10-26T10:${String(minute).padStart(2, "0")}:00Z`,
  type: "Noop",
  data: {}
});

describe("the event log", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("adds a raised event above the ones shown, and moves, selects and opens by key", async () => {
    let log = [event("b", 2), event("a", 1)];
    const net = fakeNetwork(() => log);
    live.start();
    component = mount(Events, {
      target: document.body,
      props: { page: 1, perPage: 25, types: [] }
    });
    await flush();
    const first = document.querySelector('[data-line="b"]');
    log = [event("c", 3), ...log];
    net.frame("events.raised", event("c", 3) as never);
    await flush();
    expect(
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))
    ).toEqual(["c", "b", "a"]);
    expect(document.querySelector('[data-line="b"]')).toBe(first);

    const grid = document.querySelector("[role=grid]")!;
    press(grid, "ArrowDown");
    press(grid, "ArrowDown");
    expect(active(grid)).toBe("b");
    press(grid, " ");
    expect(ticked()).toEqual(["b"]);
    press(grid, "Enter");
    await flush();
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain("b at");
  });

  it("selects a row clicked on its payload, and opens it from the payload's control", async () => {
    fakeNetwork(() => [{ ...event("a", 1), data: { path: "Music" } }]);
    component = mount(Events, {
      target: document.body,
      props: { page: 1, perPage: 25, types: [] }
    });
    await flush();
    const payload = [...document.querySelectorAll('[data-line="a"] span')].find(
      (s) => s.textContent === '{"path":"Music"}'
    )!;
    payload.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await flush();
    expect(ticked()).toEqual(["a"]);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    document
      .querySelector<HTMLButtonElement>('[data-line="a"] button[aria-label="Show full payload"]')!
      .click();
    await flush();
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain("a at");
  });
});
