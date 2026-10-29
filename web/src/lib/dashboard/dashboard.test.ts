import { describe, expect, it } from "vitest";

import { axisPadding } from "./chart";
import {
  chartData,
  errorCount,
  merged,
  outcomes,
  queries,
  shortDirectory,
  tiles,
  windowOf
} from "./reports";

const summary = (count: number, totalBytes: number) => ({
  count,
  totalBytes,
  distinctUsers: 1,
  averageSpeed: 100,
  averageWait: 2,
  averageDuration: 3
});

describe("the dashboard's reports", () => {
  it("asks for the top ten, newest or most first, over the chosen window", () => {
    const w = windowOf("7d", Date.parse("2026-10-27T00:00:00Z"));
    expect(w).toEqual({
      start: "2026-10-20T00:00:00.000Z",
      end: "2026-10-27T00:00:00.000Z",
      buckets: 84
    });
    expect(windowOf("All", 0).start).toBe("1970-01-01T00:00:00.000Z");
    expect(queries.leaderboard(w, "Upload")).toEqual({
      direction: "Upload",
      start: w.start,
      end: w.end,
      sortBy: "Count",
      sortOrder: "desc",
      limit: 10,
      offset: 0
    });
    expect(queries.exceptions(w, "Download")).toMatchObject({
      sortOrder: "desc",
      limit: 10,
      offset: 0
    });
    expect(queries.pareto(w, "Download")).toEqual({
      direction: "Download",
      start: w.start,
      end: w.end,
      limit: 10,
      offset: 0
    });
    expect(queries.summary(w)).toEqual({ start: w.start, end: w.end });
  });

  it("counts every outcome but success as an error, Rejected and Aborted too", () => {
    expect(
      errorCount({
        Succeeded: summary(5, 0),
        Rejected: summary(1, 0),
        Aborted: summary(2, 0),
        Errored: summary(3, 0)
      })
    ).toBe(6);
  });

  it("makes the chart's points in time order with rates and the ratio", () => {
    const points = chartData({
      "2026-10-26T12:00:00Z": { Upload: {}, Download: {} },
      "2026-10-26T11:00:00Z": {
        Upload: { Succeeded: summary(3, 300), Errored: summary(1, 50) },
        Download: { Succeeded: summary(2, 150) }
      }
    });
    expect(points.map((p) => p.timestamp)).toEqual([
      Date.parse("2026-10-26T11:00:00Z"),
      Date.parse("2026-10-26T12:00:00Z")
    ]);
    expect(points[0]).toMatchObject({ uploadBytes: 350, uploadErrors: 1, uploadErrorRate: 25 });
    expect(points[0].shareRatio).toBeCloseTo(350 / 150);
    expect(points[1]).toMatchObject({ uploadCount: 0, uploadErrorRate: 0, shareRatio: 0 });
  });

  it("tiles count succeeded transfers only and have no ratio without downloads", () => {
    const t = tiles({
      Upload: { Succeeded: summary(2, 500), Errored: summary(9, 9000) },
      Download: {},
      distinctUsers: 4
    });
    expect(t).toEqual({
      downloaded: 0,
      downloadedFiles: 0,
      uploaded: 500,
      uploadedFiles: 2,
      ratio: null,
      peers: 4
    });
  });

  it("merges both directions by rank and shortens directories", () => {
    const rows = merged(
      [{ count: 1 }, { count: 9 }],
      [{ count: 5 }],
      (a, b) => b.count - a.count,
      2
    );
    expect(rows).toEqual([
      { count: 9, direction: "Upload" },
      { count: 5, direction: "Download" }
    ]);
    expect(shortDirectory("@@a\\Music\\Artist\\Album")).toBe("Artist/Album");
    expect(shortDirectory("Album")).toBe("Album");
  });
});

describe("the outcome bars", () => {
  it("count each direction's transfers as done, failed or canceled", () => {
    const n = (count: number) => ({ count }) as never;
    expect(
      outcomes({
        Download: { Succeeded: n(10), Errored: n(2), TimedOut: n(1), Cancelled: n(3) },
        Upload: { Rejected: n(4), Aborted: n(1) },
        distinctUsers: 0
      } as never)
    ).toEqual([
      { direction: "Downloads", succeeded: 10, failed: 3, canceled: 3 },
      { direction: "Uploads", succeeded: 0, failed: 5, canceled: 0 }
    ]);
  });
});

describe("chart axes", () => {
  it("leave the value labels room that grows with the longest and with the text size", () => {
    const at16 = axisPadding(["0 B", "953.7 MB"], 16);
    expect(at16.left).toBeGreaterThan(axisPadding(["0 B", "9 MB"], 16).left);
    expect(axisPadding(["0 B", "953.7 MB"], 24).left).toBeGreaterThan(at16.left);
  });
});
