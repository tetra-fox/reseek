import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
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
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("shows the totals, the history, the leaderboard and the errors of fixture reports", async () => {
    app.state = {
      server: { isConnected: true, listen: { port: 2234, reachable: "unknown" } },
      user: { username: "me" },
      shares: { files: 0, directories: 0, scanning: false, scanPending: false }
    } as never;
    fakeNetwork((url) => {
      const path = url.pathname.replace("/api/v0/telemetry/reports/transfers/", "");
      if (path === "summary") {
        return {
          Download: { Succeeded: summary(3, 3 * 1024 ** 3), Errored: summary(1, 0) },
          Upload: { Succeeded: summary(6, 1.5 * 1024 ** 3) },
          distinctUsers: 5
        };
      }
      if (path === "histogram") {
        return {
          "2026-10-26T10:00:00Z": { Download: { Succeeded: summary(1, 100) }, Upload: {} },
          "2026-10-26T11:00:00Z": { Download: {}, Upload: { Succeeded: summary(2, 400) } }
        };
      }
      if (path === "leaderboard") return [{ username: "alice", ...summary(7, 7000) }];
      if (path === "directories")
        return [{ directory: "@@a\\Music\\Album", count: 4, distinctUsers: 2 }];
      if (path === "exceptions/pareto")
        return [{ exception: "Remote file not found", count: 2, distinctUsers: 1 }];
      if (path === "exceptions") return [];
      return undefined;
    });
    component = mount(Dashboard, { target: document.body });
    await flush();
    const text = () => document.body.textContent?.replace(/\s+/g, " ") ?? "";
    expect(text()).toContain("3GB Downloaded · 3 files");
    expect(text()).toContain("1.5GB Uploaded · 6 files");
    expect(text()).toContain("0.50 Share ratio");
    expect(text()).toContain("5 Users transferred with");
    // two series drawn by default, which the chart names with their peaks
    const history = document.querySelector('[role=img][aria-label^="Transfers over time"]')!;
    expect(history.getAttribute("aria-label")).toMatch(
      /Upload size peaks at .*; Download size peaks/
    );
    expect(text()).toContain("alice");

    [...document.querySelectorAll("button")].find((b) => b.textContent === "Folders")!.click();
    flushSync();
    expect(text()).toContain("Music/Album");
    [...document.querySelectorAll("button")].find((b) => b.textContent === "Errors")!.click();
    await flush();
    expect(text()).toContain("Remote file not found");
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
      [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === label)!;
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
    button("7d").click();
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
