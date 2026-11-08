import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { jobs } from "#lib/jobs/jobs.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import JobsPage from "./JobsPage.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const cleanup = {
  id: "00000000-0000-4000-8000-000000000001",
  kind: "maintenance",
  name: "Cleanup",
  starter: { kind: "schedule", name: "cleanup" },
  state: "ended",
  ending: "failed",
  error: "Events: the database is locked",
  summary: null,
  counts: { total: 9, succeeded: 8, failed: 1, canceled: 0, paused: 0 },
  progress: null,
  follows: null,
  pausable: false,
  createdAt: "2026-11-05T10:00:00Z",
  startedAt: "2026-11-05T10:00:00Z",
  endedAt: "2026-11-05T10:00:01Z"
};

describe("the Jobs page", () => {
  let page: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (page) unmount(page);
    page = null;
    jobs.rows.clear();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  it("opens a job a link names and brings it into view once the list has come", async () => {
    const scrolled = vi.fn();
    Element.prototype.scrollIntoView = scrolled;
    // the parts list draws the parts in view, so its box needs a height, which jsdom never lays out
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(24);
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/api/v0/jobs")) return { jobs: [cleanup] };
      if (url.pathname.endsWith("/api/v0/jobs/schedules")) return [];
      if (url.pathname.endsWith(`/api/v0/jobs/${cleanup.id}`))
        return { job: cleanup, parts: [{ name: "Events", state: "failed", error: "locked" }] };
      return undefined;
    });
    page = mount(JobsPage, { target: document.body, props: { shown: cleanup.id } });
    await flush();
    await flush();
    const toggle = document.querySelector(`#job-${cleanup.id} button[aria-expanded]`);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(scrolled).toHaveBeenCalledWith({ block: "center" });
    expect(document.body.textContent).toContain("locked");
  });
});
