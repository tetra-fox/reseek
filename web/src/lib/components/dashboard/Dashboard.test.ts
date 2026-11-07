import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { dashboardPrefs } from "#lib/dashboard/prefs.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Dashboard from "./Dashboard.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const summary = (count: number, totalBytes: number) => ({
  count,
  totalBytes,
  distinctUsers: 2,
  averageSpeed: 2048,
  averageWait: 1,
  averageDuration: 4
});

describe("the dashboard", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    app.state = null;
    app.metrics = null;
    // the chart's choices are kept in this browser, past the test that made them
    Object.assign(dashboardPrefs, { range: "30d", view: "size", hidden: [] });
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("keeps the last direction on in the chart, and the choice", async () => {
    app.state = {
      server: { isConnected: true, listen: { port: 2234, reachable: "unknown" } },
      user: { username: "me" },
      shares: { files: 0, directories: 0, scanning: false, scanPending: false }
    } as never;
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/summary")) return { Download: {}, Upload: {}, distinctUsers: 0 };
      if (url.pathname.endsWith("/histogram")) {
        return {
          "2026-10-26T10:00:00Z": {
            Download: { Succeeded: summary(1, 100) },
            Upload: { Succeeded: summary(2, 400) }
          }
        };
      }
      return [];
    });
    component = mount(Dashboard, { target: document.body });
    await flush();
    const chart = () =>
      document.querySelector('[role=img][aria-label^="Transfers over time"]')!.ariaLabel;
    const press = (label: string) => {
      [...document.querySelectorAll<HTMLElement>("button")]
        .find((b) => b.textContent?.trim() === label)!
        .click();
      flushSync();
    };
    press("Upload");
    expect(chart()).toMatch(/: Download size [^;]*$/);
    press("Download");
    expect(chart()).toMatch(/: Download size [^;]*$/);
    expect(dashboardPrefs.hidden).toEqual(["Upload"]);
  });

  it("keeps the leaderboard's column through a reload and starts a new range at Count", async () => {
    app.state = {
      server: { isConnected: true, listen: { port: 2234, reachable: "unknown" } },
      user: { username: "me" },
      shares: { files: 0, directories: 0, scanning: false, scanPending: false }
    } as never;
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/summary")) return { Download: {}, Upload: {}, distinctUsers: 0 };
      return url.pathname.endsWith("/histogram") ? {} : [];
    });
    component = mount(Dashboard, { target: document.body });
    await flush();
    const sorts = () =>
      net.requests
        .filter((r) => r.url.pathname.endsWith("/leaderboard"))
        .map((r) => r.url.searchParams.get("sortBy"));
    const button = (label: string) =>
      [...document.querySelectorAll<HTMLElement>("button:not([role=radio])")].find(
        (b) => b.textContent?.trim() === label
      )!;
    button("Size").click();
    await flush();
    expect(sorts().slice(-2)).toEqual(["TotalBytes", "TotalBytes"]);
    // a background reload a moment later reads the same range again
    const asked = sorts().length;
    await new Promise((resolve) => setTimeout(resolve, 5));
    document.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(sorts().length).toBe(asked + 2);
    expect(sorts().slice(-2)).toEqual(["TotalBytes", "TotalBytes"]);
    document
      .querySelector<HTMLElement>('[aria-label="Time range"] [role=radio]:nth-child(2)')!
      .click();
    await flush();
    expect(sorts().slice(-2)).toEqual(["Count", "Count"]);
  });

  it("counts what moves now from the live figures and reloads the reports as transfers finish", async () => {
    app.state = {
      server: { isConnected: true, listen: { port: 2234, reachable: "yes" } },
      user: { username: "me" },
      shares: { files: 0, directories: 0, scanning: false, scanPending: false }
    } as never;
    const direction = (active: number, queued: number, finished: number) => ({
      inProgress: { files: active, users: 1, averageSpeed: 1024, totalSpeed: 2048 * active },
      queued: { files: queued, users: 1, bytes: 0 },
      completed: { succeeded: finished, failed: 0, bytes: 0 }
    });
    app.metrics = { downloads: direction(2, 5, 1), uploads: direction(1, 0, 0) };
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/summary")) return { Download: {}, Upload: {}, distinctUsers: 0 };
      return url.pathname.endsWith("/histogram") ? {} : [];
    });
    component = mount(Dashboard, { target: document.body });
    await flush();
    const now = document
      .querySelector('[aria-label="Current activity"]')
      ?.textContent?.replace(/\s+/g, " ");
    expect(now).toContain("2 active5 queued 4 KB/s");
    expect(now).toContain("1 active0 queued 2 KB/s");
    const summaries = () => net.requests.filter((r) => r.url.pathname.endsWith("/summary")).length;
    const loaded = summaries();
    app.metrics = { downloads: direction(1, 5, 1), uploads: direction(1, 0, 0) };
    await flush();
    expect(summaries()).toBe(loaded);
    app.metrics = { downloads: direction(1, 5, 2), uploads: direction(1, 0, 0) };
    await flush();
    expect(summaries()).toBe(loaded + 1);
    app.metrics = { downloads: direction(1, 5, 3), uploads: direction(1, 0, 0) };
    await flush();
    expect(summaries()).toBe(loaded + 1);
  });
});
