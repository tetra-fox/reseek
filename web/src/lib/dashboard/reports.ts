import { api, type Schemas } from "#lib/api/client.js";
import { call } from "#lib/request.js";

export type Direction = "Upload" | "Download";
export type Summaries = Schemas["DirectionSummaries"]["Upload"];
export type Window = { start: string; end: string };

/** The ranges the dashboard offers: how many days back, and how many buckets the chart has */
export const RANGES = [
  { label: "24h", days: 1, buckets: 24 },
  { label: "7d", days: 7, buckets: 84 },
  { label: "30d", days: 30, buckets: 60 },
  { label: "90d", days: 90, buckets: 90 },
  { label: "180d", days: 180, buckets: 90 },
  { label: "1y", days: 365, buckets: 100 },
  { label: "All", days: null, buckets: 100 }
] as const;

export type RangeLabel = (typeof RANGES)[number]["label"];

/** The window a range covers, ending `now`; All starts at the epoch */
export const windowOf = (label: RangeLabel, now = Date.now()): Window & { buckets: number } => {
  const range = RANGES.find((r) => r.label === label) ?? RANGES[2];
  return {
    start: new Date(range.days === null ? 0 : now - range.days * 86_400_000).toISOString(),
    end: new Date(now).toISOString(),
    buckets: range.buckets
  };
};

/** How many rows the dashboard's tables ask for */
export const TOP = 10;

/** The queries of the dashboard's reports, each as the daemon takes it */
export const queries = {
  summary: (w: Window) => ({ start: w.start, end: w.end }),
  histogram: (w: Window, buckets: number) => ({ start: w.start, end: w.end, buckets }),
  leaderboard: (w: Window, direction: Direction, sortBy: Schemas["LeaderboardSort"] = "Count") => ({
    direction,
    start: w.start,
    end: w.end,
    sortBy,
    sortOrder: "desc" as const,
    limit: TOP,
    offset: 0
  }),
  directories: (w: Window) => ({ start: w.start, end: w.end, limit: TOP, offset: 0 }),
  exceptions: (w: Window, direction: Direction, exception?: string) => ({
    direction,
    start: w.start,
    end: w.end,
    exception,
    sortOrder: "desc" as const,
    limit: TOP,
    offset: 0
  }),
  pareto: (w: Window, direction: Direction) => ({
    direction,
    start: w.start,
    end: w.end,
    limit: TOP,
    offset: 0
  })
};

const base = "/api/v0/telemetry/reports/transfers";

/** The dashboard's report calls */
export const reports = {
  summary: (w: Window) =>
    call(() => api.GET(`${base}/summary`, { params: { query: queries.summary(w) } })),
  histogram: (w: Window, buckets: number) =>
    call(() => api.GET(`${base}/histogram`, { params: { query: queries.histogram(w, buckets) } })),
  leaderboard: (w: Window, direction: Direction, sortBy?: Schemas["LeaderboardSort"]) =>
    call(() =>
      api.GET(`${base}/leaderboard`, {
        params: { query: queries.leaderboard(w, direction, sortBy) }
      })
    ),
  directories: (w: Window) =>
    call(() => api.GET(`${base}/directories`, { params: { query: queries.directories(w) } })),
  exceptions: (w: Window, direction: Direction, exception?: string) =>
    call(() =>
      api.GET(`${base}/exceptions`, {
        params: { query: queries.exceptions(w, direction, exception) }
      })
    ),
  pareto: (w: Window, direction: Direction) =>
    call(() =>
      api.GET(`${base}/exceptions/pareto`, { params: { query: queries.pareto(w, direction) } })
    )
};

/** Every outcome but success counts as an error */
const FAILED = ["Cancelled", "TimedOut", "Errored", "Rejected", "Aborted"];

/** How many transfers a direction's summaries hold, whatever their outcome */
export const sumCounts = (d: Summaries | undefined) =>
  Object.values(d ?? {}).reduce((n, s) => n + s.count, 0);
/** How many bytes a direction's summaries hold, whatever their outcome */
export const sumBytes = (d: Summaries | undefined) =>
  Object.values(d ?? {}).reduce((n, s) => n + s.totalBytes, 0);
/** How many of a direction's transfers ended in anything but success */
export const errorCount = (d: Summaries | undefined) =>
  FAILED.reduce((n, outcome) => n + (d?.[outcome]?.count ?? 0), 0);

/** One bucket of the history chart */
export type Point = {
  timestamp: number;
  uploadBytes: number;
  downloadBytes: number;
  uploadCount: number;
  downloadCount: number;
  uploadErrors: number;
  downloadErrors: number;
  uploadErrorRate: number;
  downloadErrorRate: number;
  uploadSpeed: number;
  downloadSpeed: number;
  uploadWait: number;
  shareRatio: number;
};

/** The chart's points from the histogram, oldest first */
export const chartData = (histogram: Record<string, Schemas["DirectionSummaries"]>): Point[] =>
  Object.entries(histogram)
    .map(([at, bucket]): Point => {
      const u = bucket.Upload;
      const d = bucket.Download;
      const uploadCount = sumCounts(u);
      const downloadCount = sumCounts(d);
      const uploadErrors = errorCount(u);
      const downloadErrors = errorCount(d);
      const uploadBytes = sumBytes(u);
      const downloadBytes = sumBytes(d);
      return {
        timestamp: Date.parse(at),
        uploadBytes,
        downloadBytes,
        uploadCount,
        downloadCount,
        uploadErrors,
        downloadErrors,
        uploadErrorRate: uploadCount ? (uploadErrors / uploadCount) * 100 : 0,
        downloadErrorRate: downloadCount ? (downloadErrors / downloadCount) * 100 : 0,
        uploadSpeed: u.Succeeded?.averageSpeed ?? 0,
        downloadSpeed: d.Succeeded?.averageSpeed ?? 0,
        uploadWait: u.Succeeded?.averageWait ?? 0,
        shareRatio: downloadBytes ? uploadBytes / downloadBytes : 0
      };
    })
    .toSorted((a, b) => a.timestamp - b.timestamp);

/** The four tiles: bytes and files moved by succeeded transfers, the ratio, the peers */
export const tiles = (summary: Schemas["SummaryReport"]) => {
  const down = summary.Download.Succeeded;
  const up = summary.Upload.Succeeded;
  const downloaded = down?.totalBytes ?? 0;
  const uploaded = up?.totalBytes ?? 0;
  return {
    downloaded,
    downloadedFiles: down?.count ?? 0,
    uploaded,
    uploadedFiles: up?.count ?? 0,
    ratio: downloaded ? uploaded / downloaded : null,
    peers: summary.distinctUsers
  };
};

/** The tone of a share ratio: good above two thirds, fair above a third, poor below */
export const ratioTone = (ratio: number | null) =>
  ratio === null ? "muted" : ratio > 0.66 ? "success" : ratio >= 0.33 ? "warning" : "danger";

/** The rows of both directions as one list, each with its direction, the top `n` by `rank` */
export const merged = <T>(
  upload: T[],
  download: T[],
  rank: (a: T, b: T) => number,
  n = TOP
): (T & { direction: Direction })[] =>
  [
    ...upload.map((r) => ({ ...r, direction: "Upload" as const })),
    ...download.map((r) => ({ ...r, direction: "Download" as const }))
  ]
    .toSorted(rank)
    .slice(0, n);

/** The last two parts of a directory path, either separator */
export const shortDirectory = (path: string) =>
  path.split(/[\\/]/).filter(Boolean).slice(-2).join("/");

/** How the transfers of one direction ended in the range: done, failed, or cancelled here */
export type Outcome = { direction: string; succeeded: number; failed: number; cancelled: number };

/**
 * Each direction's finished transfers by how they ended, for the outcome bars. A timeout, an
 * error, a refusal and a size mismatch are all failures, as the transfers list's Errored filter
 * counts them
 */
export const outcomes = (summary: Schemas["SummaryReport"]): Outcome[] =>
  (
    [
      ["Downloads", summary.Download],
      ["Uploads", summary.Upload]
    ] as const
  ).map(([direction, d]) => ({
    direction,
    succeeded: d.Succeeded?.count ?? 0,
    failed: ["Errored", "TimedOut", "Rejected", "Aborted"].reduce(
      (n, s) => n + (d[s]?.count ?? 0),
      0
    ),
    cancelled: d.Cancelled?.count ?? 0
  }));
