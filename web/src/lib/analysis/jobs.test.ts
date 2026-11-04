import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

import type { Job } from "./analysis";
import { jobs } from "./jobs.svelte";

vi.mock("#lib/request.js", () => ({ call: vi.fn() }));

const reads = vi.mocked(call);

/** The queue each read answers with */
let queue: Job[] = [];

const job = (id: string): Job => ({
  id,
  path: `/d/${id}.flac`,
  root: "downloads",
  relativePath: `${id}.flac`,
  transferId: null,
  state: "queued",
  queuedAt: "2026-11-04T12:00:00Z",
  startedAt: null,
  progress: null
});

/** The listeners the queue registered on the stream, by frame type */
const listeners = new Map<string, (data: unknown) => void>();

describe("the analysis queue", () => {
  let release: () => void;

  beforeEach(() => {
    vi.useFakeTimers();
    queue = [];
    reads.mockReset();
    reads.mockImplementation(async () => ({ ok: true, data: { jobs: queue, backlog: 0 } }));
    listeners.clear();
    vi.spyOn(live, "on").mockImplementation((type, handler) => {
      listeners.set(type, handler as (data: unknown) => void);
      return () => listeners.delete(type);
    });
    live.status = "connected";
    release = jobs.retain();
    flushSync();
  });

  afterEach(() => {
    release();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("is read once while idle, then on the stream's polling while it is down", async () => {
    await vi.advanceTimersByTimeAsync(0);
    expect(reads).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(reads).toHaveBeenCalledTimes(1);

    live.status = "polling";
    flushSync();
    await vi.advanceTimersByTimeAsync(0);
    expect(reads).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(live.pollMs!);
    expect(reads).toHaveBeenCalledTimes(3);
  });

  it("stops polling once the stream is back", async () => {
    live.status = "polling";
    flushSync();
    await vi.advanceTimersByTimeAsync(0);
    const polled = reads.mock.calls.length;
    live.status = "connected";
    flushSync();
    // the read already due runs once more and sets no timer after it
    await vi.advanceTimersByTimeAsync(5_000);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(reads).toHaveBeenCalledTimes(polled + 1);
  });

  it("shows a job queued while idle at once and reads the queue once for a batch", async () => {
    await vi.advanceTimersByTimeAsync(0);
    expect(reads).toHaveBeenCalledTimes(1);
    const queued = listeners.get("files.analysis_queued")!;
    for (const id of ["a", "b", "c"]) queued(job(id));
    expect(jobs.list.map((j) => j.id)).toEqual(["a", "b", "c"]);
    expect(reads).toHaveBeenCalledTimes(1);
    // the three ran by the time the queue is read
    await vi.advanceTimersByTimeAsync(1_000);
    expect(reads).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(reads).toHaveBeenCalledTimes(2);
  });
});
