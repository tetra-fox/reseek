import type { Schemas } from "#lib/api/client.js";

export type LogRecord = Schemas["LogRecord"];

/** A log record as the live log shows it, numbered in the order the page got it */
export type Line = { seq: number; record: LogRecord };

/** The levels, their short names, how their rows look and how their filter chip looks when on */
export const LEVELS: { level: Schemas["LogLevel"]; short: string; row: string; chip: string }[] = [
  { level: "Verbose", short: "VRB", row: "text-fg-subtle", chip: "bg-surface-muted text-fg-muted" },
  { level: "Debug", short: "DBG", row: "text-fg-subtle", chip: "bg-surface-muted text-fg-muted" },
  { level: "Information", short: "INF", row: "", chip: "bg-info-soft text-info-soft-fg" },
  {
    level: "Warning",
    short: "WRN",
    row: "bg-warning-soft/50",
    chip: "bg-warning-soft text-warning-soft-fg"
  },
  {
    level: "Error",
    short: "ERR",
    row: "bg-danger-soft/50",
    chip: "bg-danger-soft text-danger-soft-fg"
  },
  {
    level: "Fatal",
    short: "FTL",
    row: "bg-danger-soft/50",
    chip: "bg-danger-soft text-danger-soft-fg"
  }
];

let next = 0;

/** A record arriving now, as a new line */
export const logLine = (record: LogRecord): Line => ({ seq: ++next, record });

const identity = (r: LogRecord) => `${r.timestamp}|${r.context}|${r.level}|${r.message}`;

/**
 * Lines for records read again (oldest first), the ones already shown keeping their lines so
 * their rows stay; records alike are matched in order
 */
export const numbered = (records: LogRecord[], shown: Line[] = []): Line[] => {
  const kept = new Map<string, Line[]>();
  for (const line of shown.toReversed()) {
    const key = identity(line.record);
    kept.set(key, [...(kept.get(key) ?? []), line]);
  }
  return records.map((record) => kept.get(identity(record))?.shift() ?? logLine(record));
};

/** The time of day of a record, `HH:mm:ss` in the browser's time zone */
export const logTime = (iso: string) => {
  const d = new Date(iso);
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
};
