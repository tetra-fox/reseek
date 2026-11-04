import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SETTLE_MS } from "#lib/inspector/inspector.svelte.js";
import { flushFaked } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { encodePath } from "#lib/transfers/content.js";

import type { Report, Target } from "./analysis";
import { FileReport } from "./report.svelte";

const file = (path: string): Target => ({ kind: "file", root: "temporary", path });
const reportOf = (path: string) =>
  ({ root: "temporary", relativePath: path, analyzedAt: "2026-11-01T00:00:00Z" }) as Report;

describe("a file's report", () => {
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("drops the answer for the file it showed before", async () => {
    let answerA!: () => void;
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/analysis/jobs")) return { jobs: [], backlog: 0 };
      if (url.pathname.includes(encodePath("a.flac"))) {
        return new Promise((resolve) => (answerA = () => resolve(reportOf("a.flac"))));
      }
      if (url.pathname.includes(encodePath("b.flac"))) return reportOf("b.flac");
    });
    const target = $state({ value: file("a.flac") });
    let report!: FileReport;
    const stop = $effect.root(() => void (report = new FileReport(() => target.value)));
    flushSync();
    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    await flushFaked();

    target.value = file("b.flac");
    flushSync();
    answerA();
    await flushFaked();
    expect(report.report).toBeUndefined();

    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    await flushFaked();
    expect(report.report?.relativePath).toBe("b.flac");
    stop();
  });
});
