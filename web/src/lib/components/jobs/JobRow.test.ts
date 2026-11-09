import { mount, unmount } from "svelte";
import { expect, it, vi } from "vitest";

import type { Job } from "#lib/jobs/jobs.js";
import { flush } from "#lib/testing/flush.js";

import JobRow from "./JobRow.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

it("says when an ended job ended and how long it took, a space apart", async () => {
  const job = {
    id: "00000000-0000-4000-8000-000000000001",
    kind: "maintenance",
    name: "Cleanup",
    starter: { kind: "schedule", name: "cleanup" },
    state: "ended",
    ending: "succeeded",
    error: null,
    summary: null,
    counts: { total: 1, succeeded: 1, failed: 0, canceled: 0, paused: 0 },
    progress: null,
    follows: null,
    pausable: false,
    createdAt: "2026-11-05T10:00:00Z",
    startedAt: "2026-11-05T10:00:00Z",
    endedAt: "2026-11-05T10:00:01Z"
  } as unknown as Job;
  const row = mount(JobRow, {
    target: document.body,
    props: { job, open: false, ontoggle: () => {} }
  });
  await flush();
  const ended = [...document.querySelectorAll("span")].find((s) =>
    s.textContent?.startsWith("Ended")
  )!;
  expect(ended.textContent).toMatch(/^Ended .+ \(took 1s\)$/);
  unmount(row);
  document.body.innerHTML = "";
});
