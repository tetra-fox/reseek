const UNITS = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"] as const;

/**
 * A byte count in binary units with the familiar short labels, trailing zeros dropped: 1024 as
 * "1 KB", 1572864 as "1.5 MB". `padding` left-pads the number, for columns that must not jump
 */
export const formatBytes = (bytes: number, decimals = 2, padding = 0, padChar = " "): string => {
  if (!Number.isFinite(bytes) || bytes < 1) return "0 B".padStart(padding + 2, padChar);
  let i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), UNITS.length - 1);
  const rounded = (unit: number) =>
    parseFloat((bytes / 1024 ** unit).toFixed(Math.max(decimals, 0)));
  // rounding can reach the next unit: 1023.999 GB is "1 TB", not "1024 GB"
  if (rounded(i) >= 1024 && i < UNITS.length - 1) i++;
  return `${String(rounded(i)).padStart(padding, padChar)} ${UNITS[i]}`;
};

/**
 * Bytes moved of a size in the size's unit, both to one decimal, whole under 1 KB: "3.2/10.5 MB",
 * "0 B/0 B" for an empty file
 */
export const formatBytesTransferred = (size: number, transferred: number): string => {
  if (!Number.isFinite(size) || size < 1) return "0 B/0 B";
  const i = Math.min(Math.floor(Math.log(size) / Math.log(1024)), UNITS.length - 1);
  const one = (n: number) => (Math.max(n, 0) / 1024 ** i).toFixed(i === 0 ? 0 : 1);
  return `${one(transferred)}/${one(size)} ${UNITS[i]}`;
};

/** A speed in bytes per second, "0 B/s" when there is none */
export const formatSpeed = (bytesPerSecond: number | null | undefined, decimals = 1): string =>
  bytesPerSecond ? `${formatBytes(bytesPerSecond, decimals)}/s` : "0 B/s";

/** A speed padded to one width, for the footer where it updates every second */
export const formatSpeedPadded = (bytesPerSecond: number | null | undefined): string =>
  !bytesPerSecond || bytesPerSecond < 1
    ? "   0 B/s"
    : `${formatBytes(bytesPerSecond, 1, 4, " ")}/s`;

const pad2 = (n: number) => String(n).padStart(2, "0");

/**
 * A length of a track: "MM:SS", or "H:MM:SS" from an hour on with no upper bound. A length of 0
 * is one a peer doesn't know, as a bitrate of 0 is, and shows nothing
 */
export const formatSeconds = (seconds: number | null | undefined): string => {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds <= 0) {
    return "";
  }
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0 ? `${h}:${pad2(m)}:${pad2(s)}` : `${pad2(m)}:${pad2(s)}`;
};

/** A short wait: "42s" under a minute, else minutes to a tenth, "1.5m" */
export const formatWait = (seconds: number | null | undefined): string => {
  if (!seconds) return "0s";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  return `${(seconds / 60).toFixed(1)}m`;
};

// each unit's length in seconds and the letter a duration writes it with
const SPANS: [Intl.RelativeTimeFormatUnit, number, string][] = [
  ["day", 86_400, "d"],
  ["hour", 3_600, "h"],
  ["minute", 60, "m"],
  ["second", 1, "s"]
];

/**
 * A length of time in its largest non-zero units, two unless `units` says: 5400 as "1h 30m",
 * 90061 as "1d 1h", and with three 11525 as "3h 12m 5s"
 */
export const formatDuration = (seconds: number, units = 2): string => {
  let left = Math.floor(seconds);
  if (!Number.isFinite(left) || left <= 0) return "0s";
  const parts: string[] = [];
  for (const [, size, letter] of SPANS) {
    if (left >= size) {
      parts.push(`${Math.floor(left / size)}${letter}`);
      left %= size;
    }
    if (parts.length === units) break;
  }
  return parts.join(" ");
};

const DAY_MS = 86_400_000;
const MONTH_MS = 2_630_016_000;
const YEAR_MS = 31_557_600_000;

// the units an option's duration is written with, as the daemon's humantime reads and writes
// them; "m" is minutes and "M" months
const UNIT_MS: Record<string, number> = Object.fromEntries(
  (
    [
      [1, ["ms", "msec"]],
      [1_000, ["s", "sec", "secs", "second", "seconds"]],
      [60_000, ["m", "min", "mins", "minute", "minutes"]],
      [3_600_000, ["h", "hr", "hrs", "hour", "hours"]],
      [DAY_MS, ["d", "day", "days"]],
      [7 * DAY_MS, ["w", "week", "weeks"]],
      [MONTH_MS, ["M", "month", "months"]],
      [YEAR_MS, ["y", "year", "years"]]
    ] as const
  ).flatMap(([ms, names]) => names.map((n) => [n, ms]))
);

/**
 * A duration the way options write them, "2m", "1h 30m" or "5months 27days", in milliseconds;
 * null for anything else
 */
export const parseDuration = (text: unknown): number | null => {
  if (typeof text !== "string") return null;
  const trimmed = text.trim();
  const parts = [...trimmed.matchAll(/(\d+)\s*([a-zA-Z]+)\s*/g)];
  if (!parts.length || parts.map((p) => p[0]).join("") !== trimmed) return null;
  if (parts.some(([, , unit]) => !(unit in UNIT_MS))) return null;
  return parts.reduce((sum, [, n, unit]) => sum + Number(n) * UNIT_MS[unit], 0);
};

// the units a duration is said in, largest first, with their lengths in milliseconds
const SAID: [string, number][] = [
  ["year", YEAR_MS],
  ["month", MONTH_MS],
  ["day", DAY_MS],
  ["hour", 3_600_000],
  ["minute", 60_000],
  ["second", 1_000]
];

/**
 * A length of time as a person says it, in its largest unit: "30 days", "1 hour", and "about 6
 * months" for 5 months 27 days 19 hours
 */
export const sayDuration = (ms: number): string => {
  const [unit, size] = SAID.find(([, s]) => ms >= s) ?? ["second", 1_000];
  const n = Math.round(ms / size);
  const words = plural(n, unit, `${unit}s`);
  return n * size === ms ? words : `about ${words}`;
};

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

/** How far from now an instant is still said relative to it; past that, its date is clearer */
const RELATIVE_MS = 7 * DAY_MS;

/**
 * "3 hours ago" or "in 2 days", relative to `now`, for an instant from the API; a week or more
 * away, its date: "Mar 14", "Mar 14, 2023"
 */
export const ago = (iso: string, now = Date.now()): string => {
  const ms = Date.parse(iso) - now;
  if (Math.abs(ms) >= RELATIVE_MS) return shortDate(iso, now);
  const secs = Math.round(ms / 1000);
  // Intl says "now", which reads wrong after a verb: "Last run now"
  if (secs === 0) return "just now";
  const [unit, size] = SPANS.find(([, s]) => Math.abs(secs) >= s) ?? ["second", 1];
  return relative.format(Math.round(secs / size), unit);
};

/** An instant from the API in the browser's time zone, for a details view */
export const formatDate = (iso: string): string => new Date(iso).toLocaleString();

// made once: a list row formats several instants and numbers, and each toLocaleString call with
// options builds its formatter anew
const DATE_TIME = new Intl.DateTimeFormat([], {
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit"
});
const DATE_TIME_YEAR = new Intl.DateTimeFormat([], {
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit"
});
const DATE = new Intl.DateTimeFormat([], { month: "short", day: "numeric" });
const DATE_YEAR = new Intl.DateTimeFormat([], { year: "numeric", month: "short", day: "numeric" });
const TIME = new Intl.DateTimeFormat([], { hour: "2-digit", minute: "2-digit" });
const DAY = new Intl.DateTimeFormat([], {
  weekday: "long",
  year: "numeric",
  month: "long",
  day: "numeric"
});
const NUMBER = new Intl.NumberFormat();

/** A count with the browser's digit grouping: "1,200" */
export const formatCount = (n: number): string => NUMBER.format(n);

/** A count that keeps one width, on a badge or in a column: "999+" past 999, or past `most` */
export const badgeCount = (n: number, most = 999): string =>
  n > most ? `${formatCount(most)}+` : formatCount(n);

const thisYear = (date: Date, now: number) => date.getFullYear() === new Date(now).getFullYear();

/**
 * An instant in the browser's time zone, short, for tables: "Mar 14, 09:05", with the year when
 * it is not this year's
 */
export const dateTime = (iso: string, now = Date.now()): string => {
  const date = new Date(iso);
  return (thisYear(date, now) ? DATE_TIME : DATE_TIME_YEAR).format(date);
};

/** The day of an instant, short: "Mar 14", with the year when it is not this year's */
export const shortDate = (iso: string, now = Date.now()): string => {
  const date = new Date(iso);
  return (thisYear(date, now) ? DATE : DATE_YEAR).format(date);
};

/** The time of day of an instant in the browser's time zone: "09:05" */
export const shortTime = (iso: string): string => TIME.format(new Date(iso));

/** The day of an instant, written out, for the line between two days of chat */
export const formatDay = (iso: string): string => DAY.format(new Date(iso));

/** A chat message's time: the time of day for today, with the date before today */
export const chatTime = (iso: string, now = Date.now()): string =>
  new Date(iso).toDateString() === new Date(now).toDateString()
    ? shortTime(iso)
    : dateTime(iso, now);

/** What a file's attributes say about its quality: "16/44.1kHz", "320 kbps", "245 kbps, VBR" */
export const formatAttributes = (file: {
  bitRate?: number | null;
  isVariableBitRate?: boolean | null;
  bitDepth?: number | null;
  sampleRate?: number | null;
}): string => {
  const { bitRate, isVariableBitRate, bitDepth, sampleRate } = file;
  if (sampleRate && bitDepth) return `${bitDepth}/${sampleRate / 1000}kHz`;
  if (isVariableBitRate) return bitRate ? `${bitRate} kbps, VBR` : "VBR";
  if (bitRate) return `${bitRate} kbps`;
  return "";
};

/** The last part of a remote path, after its last backslash or slash */
export const getFileName = (path: string): string => path.split("\\").pop()!.split("/").pop()!;

/**
 * Who files came from, in parts: one or two names, joined by "and", or the first and the words
 * for how many others
 */
export const namedOf = (usernames: string[]) =>
  usernames.length <= 2
    ? { names: usernames, others: "" }
    : { names: usernames.slice(0, 1), others: ` and ${usernames.length - 1} others` };

/** Who files came from, in words */
export const fromWhom = (usernames: string[]) => {
  const { names, others } = namedOf(usernames);
  return names.join(" and ") + others;
};

/**
 * The directory part of a remote path, by the separator it uses: `/` when it has one, else `\`.
 * A separator at the very start is not a cut, so "\\foo" stays as it is; a bare file name, which
 * some peers send, has no directory
 */
export const getDirectoryName = (path: string): string => {
  const separator = path.includes("/") ? "/" : "\\";
  const cut = path.lastIndexOf(separator);
  if (cut === -1) return "";
  return cut > 0 ? path.slice(0, cut) : path;
};

/** `text` cut to `max` characters with "..." after it when it was longer */
export const truncate = (text: string | null | undefined, max: number): string => {
  if (!text) return "";
  return text.length <= max ? text : `${text.slice(0, max)}...`;
};

/** What a figure or a field shows when it has no value */
export const NONE = "—";

/** `n` with the noun for that many, "1 file" or "2,048 files" */
export const plural = (n: number, one: string, many: string): string =>
  `${formatCount(n)} ${n === 1 ? one : many}`;

const LIST = new Intl.ListFormat("en", { type: "conjunction" });

/** Parts joined as a sentence does, "a, b, and c" */
export const listText = (parts: string[]) => LIST.format(parts);

export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * Saves what `url` serves as a file, under `filename` or the name the response gives. A link with
 * `download` saves without navigating: a navigation, which location.assign starts, can cancel the
 * page's open requests, the live stream among them, before the browser sees it is a download
 */
export const saveUrl = (url: string, filename = "") => {
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
};

/** Hands `data` to the browser as a file to save */
export const downloadFile = (
  data: BlobPart,
  filename: string,
  mime = "application/octet-stream"
) => {
  const url = URL.createObjectURL(new Blob([data], { type: mime }));
  saveUrl(url, filename);
  URL.revokeObjectURL(url);
};

/** The widest text each formatter writes, which a column of its values must hold */
export const WIDEST = {
  size: formatBytes(1023.99 * 1024 ** 3),
  speed: formatSpeed(1023.9 * 1024 ** 2),
  // a large library shares a few million files
  count: formatCount(9_999_999),
  length: formatSeconds(99 * 3600 + 59 * 60 + 59),
  // a stalled transfer can be thousands of days from done
  duration: formatDuration(9999 * 86_400 + 23 * 3600),
  transferred: formatBytesTransferred(1023.9 * 1024 ** 3, 1023.9 * 1024 ** 3)
};

/**
 * The width a column of figures needs to show `widest` whole, as a CSS length, with `extra` for
 * anything drawn beside the text: N characters in N ch plus the cells' 1rem of padding. A tabular
 * digit is a little wider than 1ch, which is the plain zero, but the point, space and separators
 * each formatter writes are narrower by more, and both scale with the font. It must be set where
 * the font is the cells', so the ch is theirs
 */
export const columnWidth = (widest: string, extra?: string): string =>
  `calc(${widest.length}ch + 1rem${extra ? ` + ${extra}` : ""})`;

/** A column's width as CSS: a number is the pixels a user dragged it to, a string a length */
export const cssWidth = (width: number | string): string =>
  typeof width === "number" ? `${width}px` : width;

/** The widths of columns side by side, as one CSS length */
export const sumWidths = (widths: (number | string)[]): string =>
  `calc(${widths.map(cssWidth).join(" + ")})`;
