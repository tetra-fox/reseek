import type { Unit } from "./chart";
import type { Point } from "./reports";

/** A line of the history chart: what it plots, its name, unit and colour token */
export type Series = {
  key: Exclude<keyof Point, "timestamp">;
  name: string;
  unit: Unit;
  color: string;
};

export const SERIES: Series[] = [
  {
    key: "uploadBytes",
    name: "Upload size",
    unit: "bytes",
    color: "var(--color-series-upload-bytes)"
  },
  {
    key: "downloadBytes",
    name: "Download size",
    unit: "bytes",
    color: "var(--color-series-download-bytes)"
  },
  {
    key: "uploadCount",
    name: "Upload count",
    unit: "count",
    color: "var(--color-series-upload-count)"
  },
  {
    key: "downloadCount",
    name: "Download count",
    unit: "count",
    color: "var(--color-series-download-count)"
  },
  {
    key: "uploadSpeed",
    name: "Upload speed",
    unit: "speed",
    color: "var(--color-series-upload-speed)"
  },
  {
    key: "downloadSpeed",
    name: "Download speed",
    unit: "speed",
    color: "var(--color-series-download-speed)"
  },
  {
    key: "uploadErrors",
    name: "Upload errors",
    unit: "count",
    color: "var(--color-series-upload-errors)"
  },
  {
    key: "downloadErrors",
    name: "Download errors",
    unit: "count",
    color: "var(--color-series-download-errors)"
  },
  {
    key: "uploadErrorRate",
    name: "Upload error rate",
    unit: "rate",
    color: "var(--color-series-upload-error-rate)"
  },
  {
    key: "downloadErrorRate",
    name: "Download error rate",
    unit: "rate",
    color: "var(--color-series-download-error-rate)"
  },
  {
    key: "uploadWait",
    name: "Upload queue wait",
    unit: "seconds",
    color: "var(--color-series-upload-wait)"
  },
  {
    key: "shareRatio",
    name: "Share ratio",
    unit: "ratio",
    color: "var(--color-series-share-ratio)"
  }
];
