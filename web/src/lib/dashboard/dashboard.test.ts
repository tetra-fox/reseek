import { describe, expect, it } from "vitest";

import { axisPadding, axisTicks, valueLabel, type Unit } from "./chart";
import { readPrefs } from "./prefs.svelte";
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
import { ERRORS, VIEWS } from "./series";

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

  it("counts failures as errors, as the outcome bars do, and not cancels", () => {
    const d = {
      Succeeded: summary(5, 0),
      Rejected: summary(1, 0),
      Aborted: summary(2, 0),
      Errored: summary(3, 0),
      TimedOut: summary(4, 0),
      Cancelled: summary(7, 0),
      None: summary(8, 0)
    };
    expect(errorCount(d)).toBe(10);
    const [bars] = outcomes({ Download: d, Upload: {}, distinctUsers: 0 });
    expect(bars.failed).toBe(10);
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

describe("the history chart's views", () => {
  it("draw every field of a point on one axis, in exactly one place", () => {
    const keys = [...VIEWS, ERRORS].flatMap((v) => v.series.map((s) => s.key));
    const point = chartData({ "2026-10-26T10:00:00Z": { Upload: {}, Download: {} } })[0];
    const { timestamp: _, ...fields } = point;
    expect(keys.toSorted()).toEqual(Object.keys(fields).toSorted());
  });

  it("have the toggles of both directions or none", () => {
    for (const view of [...VIEWS, ERRORS]) {
      const directions = view.series.map((s) => s.direction).filter(Boolean);
      expect(directions.length ? directions : ["Upload", "Download"]).toEqual([
        "Upload",
        "Download"
      ]);
    }
  });

  const UNITS: Unit[] = ["bytes", "count", "rate", "ratio", "seconds", "speed"];
  const TOPS = [
    0.004,
    0.3,
    0.6,
    1,
    2,
    3,
    7,
    10,
    45,
    59,
    61,
    100,
    999,
    1000,
    1024,
    1500,
    7200,
    900_000,
    1024 ** 2,
    1.1 * 1024 ** 3,
    3 * 1024 ** 3,
    1023.9 * 1024 ** 4
  ];

  it.each(UNITS)("end a %s axis on a round step past the top, each label its own", (unit) => {
    const label = valueLabel(unit);
    for (const top of TOPS) {
      const ticks = axisTicks(unit, top);
      expect(ticks[0]).toBe(0);
      expect(ticks.at(-1)).toBeGreaterThanOrEqual(top);
      expect(ticks.length).toBeLessThanOrEqual(7);
      expect(new Set(ticks.map(label)).size, `${unit} up to ${top}`).toBe(ticks.length);
    }
  });

  it("has one tick for a chart of zeroes", () => {
    expect(axisTicks("bytes", 0)).toEqual([0]);
  });

  it("counts in whole files", () => {
    expect(axisTicks("count", 2)).toEqual([0, 1, 2]);
    expect(axisTicks("count", 3).every(Number.isInteger)).toBe(true);
  });

  it("steps bytes and speeds in labels that read whole", () => {
    const labels = (unit: Unit, top: number) => axisTicks(unit, top).map(valueLabel(unit));
    expect(labels("bytes", 3 * 1024 ** 3)).toEqual(["0 B", "1 GB", "2 GB", "3 GB"]);
    expect(labels("bytes", 2 * 1024 ** 3)).toEqual(["0 B", "512 MB", "1 GB", "1.5 GB", "2 GB"]);
    expect(labels("speed", 900 * 1024)).toEqual([
      "0 B/s",
      "200 KB/s",
      "400 KB/s",
      "600 KB/s",
      "800 KB/s",
      "1000 KB/s"
    ]);
  });

  it("steps a queue wait as a clock does", () => {
    expect(axisTicks("seconds", 45)).toEqual([0, 10, 20, 30, 40, 50]);
    expect(axisTicks("seconds", 200)).toEqual([0, 60, 120, 180, 240]);
  });
});

describe("the dashboard's saved choices", () => {
  it("keep what is valid", () => {
    expect(readPrefs({ range: "7d", view: "speed", hidden: ["Upload"] })).toEqual({
      range: "7d",
      view: "speed",
      hidden: ["Upload"]
    });
  });

  it.each([
    null,
    "x",
    3,
    [],
    { view: "bytes", hidden: "Upload" },
    { view: 1, hidden: [1, null] },
    { hidden: ["Upload", "Download"] }
  ])("fall back from %j", (saved) => {
    expect(readPrefs(saved)).toEqual({ range: "30d", view: "size", hidden: [] });
  });
});
