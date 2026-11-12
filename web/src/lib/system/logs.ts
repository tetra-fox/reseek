import type { Schemas } from "#lib/api/client.js";
import { shortDate } from "#lib/format.js";

export type LogRecord = Schemas["LogRecord"];

/** A log record as the live log shows it, numbered in the order the page got it */
export type Line = { seq: number; record: LogRecord };

/** The levels, their short names and how their rows look */
export const LEVELS: { level: Schemas["LogLevel"]; short: string; row: string }[] = [
  { level: "Verbose", short: "VRB", row: "text-fg-muted" },
  { level: "Debug", short: "DBG", row: "text-fg-muted" },
  { level: "Information", short: "INF", row: "" },
  { level: "Warning", short: "WRN", row: "bg-warning-soft/50" },
  { level: "Error", short: "ERR", row: "bg-danger-soft/50" },
  { level: "Fatal", short: "FTL", row: "bg-danger-soft/50" }
];

let next = 0;

/** A record arriving now, as a new line */
export const logLine = (record: LogRecord): Line => ({ seq: ++next, record });

const identity = (r: LogRecord) => `${r.timestamp}|${r.context}|${r.level}|${r.message}`;

/**
 * Whether `record` is shown in `lines` already: a stream that resumed to follow the log replays
 * records a read just before it brought
 */
export const isShown = (record: LogRecord, lines: Line[]) =>
  lines.some(
    ({ record: r }) =>
      r.timestamp === record.timestamp &&
      r.message === record.message &&
      r.context === record.context &&
      r.level === record.level
  );

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

/**
 * The time of a record, `HH:mm:ss` in the browser's time zone, after the day when it is not
 * today's: "Mar 14 09:05:01"
 */
export const logTime = (iso: string, now = Date.now()) => {
  const d = new Date(iso);
  const time = [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
  return d.toDateString() === new Date(now).toDateString()
    ? time
    : `${shortDate(iso, now)} ${time}`;
};
