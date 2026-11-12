import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import LiveLogs from "./LiveLogs.svelte";

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

  it("keeps the lines older than the daemon's buffer through a reload, and leaves cleared ones out", async () => {
    // the daemon keeps the newest two records
    let buffer = [record("one", 1), record("two", 2)];
    const net = fakeNetwork(() => buffer);
    live.start();
    component = mount(LiveLogs, { target: document.body });
    await flush();
    net.frame("logs.record", record("three", 3));
    net.frame("logs.record", record("four", 4));
    await flush();
    const rows = [...document.querySelectorAll("[data-line]")];

    // "five" was missed while the stream was away
    buffer = [record("four", 4), record("five", 5)];
    document.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(messages()).toEqual(["five", "four", "three", "two", "one"]);
    expect([...document.querySelectorAll("[data-line]")].slice(1)).toEqual(rows);

    [...document.querySelectorAll("button")].find((b) => b.textContent?.includes("Clear"))!.click();
    flushSync();
    document.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(messages()).toEqual([]);

    // a reopened stream's snapshot is the list
    net.frame("logs.buffer", buffer as never);
    await flush();
    expect(messages()).toEqual(["five", "four"]);
  });

  it("shows once a record that a read brought and a resumed stream replays", async () => {
    const net = fakeNetwork(() => [record("one", 1), record("two", 2)]);
    live.start();
    component = mount(LiveLogs, { target: document.body });
    await flush();
    expect(messages()).toEqual(["two", "one"]);
    net.frame("logs.record", record("two", 2));
    net.frame("logs.record", record("three", 3));
    await flush();
    expect(messages()).toEqual(["three", "two", "one"]);
  });
});
