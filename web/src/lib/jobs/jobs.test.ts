import { describe, expect, it } from "vitest";

import {
  everyText,
  failureUnseen,
  filtered,
  type Job,
  ordered,
  progressText,
  type Schedule,
  starterText,
  summaryText
} from "./jobs";

const job = (fields: Partial<Job> = {}): Job => ({
  id: "j",
  kind: "download",
  name: "Album from alice",
  starter: { kind: "you", name: null },
  state: "running",
  ending: null,
  error: null,
  summary: null,
  counts: { total: 0, succeeded: 0, failed: 0, canceled: 0, paused: 0 },
  progress: null,
  follows: null,
  pausable: true,
  createdAt: "2026-11-05T10:00:00Z",
  startedAt: "2026-11-05T10:00:00Z",
  endedAt: null,
  ...fields
});

const schedule = (fields: Partial<Schedule> = {}): Schedule => ({
  name: "cleanup",
  title: "Cleanup",
  description: "",
  every: "5m",
  option: "schedules.cleanup",
  note: null,
  runnable: true,
  running: false,
  ...fields
});

describe("the jobs list", () => {
  it("runs first, then waits oldest first, then ended newest first", () => {
    const list = [
      job({ id: "old", state: "ended", ending: "succeeded", endedAt: "2026-11-05T09:00:00Z" }),
      job({ id: "q2", state: "queued", createdAt: "2026-11-05T10:02:00Z" }),
      job({ id: "new", state: "ended", ending: "failed", endedAt: "2026-11-05T11:00:00Z" }),
      job({ id: "paused", state: "paused" }),
      job({ id: "q1", state: "queued", createdAt: "2026-11-05T10:01:00Z" })
    ];
    expect(ordered(list).map((j) => j.id)).toEqual(["paused", "q1", "q2", "new", "old"]);
  });

  it("filters by kind, state or ending, starter and name together", () => {
    const list = [
      job({ id: "a", kind: "analysis", name: "Analyze Album" }),
      job({
        id: "b",
        state: "ended",
        ending: "failed",
        starter: { kind: "schedule", name: "cleanup" }
      }),
      job({ id: "c", state: "ended", ending: "succeeded" })
    ];
    const keep = (f: Partial<Parameters<typeof filtered>[1]>) =>
      filtered(list, { kinds: [], states: [], starters: [], text: "", ...f }).map((j) => j.id);
    expect(keep({})).toEqual(["a", "b", "c"]);
    expect(keep({ kinds: ["analysis"] })).toEqual(["a"]);
    expect(keep({ states: ["failed", "running"] })).toEqual(["a", "b"]);
    expect(keep({ starters: ["schedule"] })).toEqual(["b"]);
    expect(keep({ text: "  album " })).toEqual(["a", "b", "c"]);
    expect(keep({ text: "analyze", states: ["succeeded"] })).toEqual([]);
  });

  it("says how far a job is and who started it", () => {
    const counts = { total: 12, succeeded: 6, failed: 1, canceled: 0, paused: 0 };
    expect(progressText(job({ counts }))).toBe("7 of 12 files, 1 failed");
    expect(progressText(job({ state: "ended", counts: { ...counts, canceled: 5 } }))).toBe(
      "12 files, 1 failed, 5 canceled"
    );
    expect(progressText(job({ kind: "maintenance", counts: { ...counts, total: 9 } }))).toBe(
      "7 of 9 steps, 1 failed"
    );
    expect(progressText(job())).toBe("");
    expect(starterText(job({ starter: { kind: "schedule", name: "cleanup" } }), [schedule()])).toBe(
      "Started by the cleanup schedule"
    );
    expect(starterText(job({ starter: { kind: "resumed", name: null } }))).toBe(
      "Started when reseek started"
    );
  });
});

describe("the bottom bar", () => {
  it("counts what runs and is paused, and marks a failure until the page is seen", () => {
    const summary = { running: 2, paused: 1, lastFailedAt: "2026-11-05T10:00:00Z" };
    expect(summaryText(summary)).toBe("2 jobs running, 1 paused");
    expect(summaryText({ running: 1, paused: 0, lastFailedAt: null })).toBe("1 job running");
    expect(summaryText({ running: 0, paused: 0, lastFailedAt: null })).toBeNull();
    expect(summaryText(summary, true)).toBe("2 running, 1 paused");
    // what was seen is the daemon's own time of the failure, never this browser's clock
    expect(failureUnseen(summary, null)).toBe(true);
    expect(failureUnseen(summary, "2026-11-05T09:00:00Z"), "an earlier failure").toBe(true);
    expect(failureUnseen(summary, "2026-11-05T10:00:00Z")).toBe(false);
    expect(failureUnseen({ ...summary, lastFailedAt: null }, null)).toBe(false);
  });
});

describe("the schedules card", () => {
  it("says how often in words, with the time of day", () => {
    expect(everyText(schedule({ every: "5m" }))).toBe("Every 5 minutes");
    expect(everyText(schedule({ every: "1d at 04:00" }))).toBe("Every day at 04:00");
    expect(everyText(schedule({ every: "7d at 04:00" }))).toBe("Every 7 days at 04:00");
    expect(everyText(schedule({ every: null }))).toBe("Off");
  });
});
