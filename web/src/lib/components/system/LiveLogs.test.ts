import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import LiveLogs from "./LiveLogs.svelte";

const flush = async () => {
  for (let i = 0; i < 3; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};
const record = (message: string, second: number) =>
  ({
    timestamp: `2026-10-26T10:00:${String(second).padStart(2, "0")}Z`,
    context: "reseek::shares",
    subContext: null,
    level: "Information",
    message
  }) as const;

describe("the live log", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const messages = () =>
    [...document.querySelectorAll("[data-line]")].map((l) =>
      l.textContent?.trim().split(/\s+/).pop()
    );

  it("puts new records on top without remaking the rows, and holds them while paused", async () => {
    let buffer = [record("one", 1), record("two", 2)];
    const net = fakeNetwork(() => buffer);
    live.start();
    component = mount(LiveLogs, { target: document.body });
    await flush();
    expect(messages()).toEqual(["two", "one"]);
    const two = document.querySelector("[data-line]");

    net.frame("logs.record", record("three", 3));
    await flush();
    expect(messages()).toEqual(["three", "two", "one"]);
    expect(document.querySelectorAll("[data-line]")[1]).toBe(two);

    // the buffer read again keeps the rows already shown
    buffer = [record("one", 1), record("two", 2), record("three", 3)];
    net.frame("logs.buffer", buffer as never);
    await flush();
    expect(document.querySelectorAll("[data-line]")[1]).toBe(two);

    [...document.querySelectorAll("button")].find((b) => b.textContent?.includes("Pause"))!.click();
    flushSync();
    net.frame("logs.record", record("four", 4));
    await flush();
    expect(messages()).toEqual(["three", "two", "one"]);
    const resume = [...document.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Resume")
    )!;
    expect(resume.textContent).toContain("1 new");
    resume.click();
    flushSync();
    expect(messages()).toEqual(["four", "three", "two", "one"]);
  });
});
