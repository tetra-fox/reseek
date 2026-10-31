import type { Unit } from "./chart";
import type { Direction, Point } from "./reports";

/** A line of a history chart: what it plots, its name and color token */
export type Series = {
  key: Exclude<keyof Point, "timestamp">;
  name: string;
  /** the toggle that shows and hides it, on a line with a counterpart in the other direction */
  direction?: Direction;
  color: string;
};

/** What a history chart plots: lines of one unit, which share its axis */
export type ChartView = { unit: Unit; series: Series[] };

export type ViewId = "size" | "count" | "speed" | "errorRate" | "wait" | "ratio";

/** The choices of Transfers over time. Error counts are the Errors chart's */
export const VIEWS: (ChartView & { id: ViewId; label: string })[] = [
  {
    id: "size",
    label: "Size",
    unit: "bytes",
    series: [
      {
        key: "uploadBytes",
        name: "Upload size",
        direction: "Upload",
        color: "var(--color-series-upload-bytes)"
      },
      {
        key: "downloadBytes",
        name: "Download size",
        direction: "Download",
        color: "var(--color-series-download-bytes)"
      }
    ]
  },
  {
    id: "count",
    label: "Count",
    unit: "count",
    series: [
      {
        key: "uploadCount",
        name: "Upload count",
        direction: "Upload",
        color: "var(--color-series-upload-count)"
      },
      {
        key: "downloadCount",
        name: "Download count",
        direction: "Download",
        color: "var(--color-series-download-count)"
      }
    ]
  },
  {
    id: "speed",
    label: "Speed",
    unit: "speed",
    series: [
      {
        key: "uploadSpeed",
        name: "Upload speed",
        direction: "Upload",
        color: "var(--color-series-upload-speed)"
      },
      {
        key: "downloadSpeed",
        name: "Download speed",
        direction: "Download",
        color: "var(--color-series-download-speed)"
      }
    ]
  },
  {
    id: "errorRate",
    label: "Error rate",
    unit: "rate",
    series: [
      {
        key: "uploadErrorRate",
        name: "Upload error rate",
        direction: "Upload",
        color: "var(--color-series-upload-error-rate)"
      },
      {
        key: "downloadErrorRate",
        name: "Download error rate",
        direction: "Download",
        color: "var(--color-series-download-error-rate)"
      }
    ]
  },
  {
    id: "wait",
    label: "Queue wait",
    unit: "seconds",
    series: [
      { key: "uploadWait", name: "Upload queue wait", color: "var(--color-series-upload-wait)" }
    ]
  },
  {
    id: "ratio",
    label: "Share ratio",
    unit: "ratio",
    series: [{ key: "shareRatio", name: "Share ratio", color: "var(--color-series-share-ratio)" }]
  }
];

/** The chart of the Errors tab */
export const ERRORS: ChartView = {
  unit: "count",
  series: [
    {
      key: "uploadErrors",
      name: "Upload errors",
      direction: "Upload",
      color: "var(--color-series-upload-errors)"
    },
    {
      key: "downloadErrors",
      name: "Download errors",
      direction: "Download",
      color: "var(--color-series-download-errors)"
    }
  ]
};
