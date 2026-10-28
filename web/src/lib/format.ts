const UNITS = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"] as const;

/**
 * A byte count in binary units with the familiar short labels, trailing zeros dropped: 1024 as
 * "1 KB", 1572864 as "1.5 MB". `padding` left-pads the number, for columns that must not jump
 */
export const formatBytes = (bytes: number, decimals = 2, padding = 0, padChar = " "): string => {
  if (!Number.isFinite(bytes) || bytes < 1) return "0 B".padStart(padding + 2, padChar);
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), UNITS.length - 1);
  const n = parseFloat((bytes / 1024 ** i).toFixed(Math.max(decimals, 0)));
  return `${String(n).padStart(padding, padChar)} ${UNITS[i]}`;
};

/** A byte count in one given unit: a number, or for "B" the text "<n> B" */
export const formatBytesAsUnit = (
  bytes: number,
  unit: (typeof UNITS)[number],
  decimals = 2
): number | string => {
  if (unit === "B") return `${bytes} B`;
  return parseFloat((bytes / 1024 ** UNITS.indexOf(unit)).toFixed(decimals));
};

/**
 * Bytes moved of a size in the size's unit, both to one decimal: "3.2/10.5 MB", "0 B/0 B"
 * for an empty file
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

/** A length of a track: "MM:SS", or "H:MM:SS" from an hour on with no upper bound */
export const formatSeconds = (seconds: number | null | undefined): string => {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) {
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

/** A length of time in its two largest non-zero units: 5400 as "1h 30m", 90061 as "1d 1h" */
export const formatDuration = (seconds: number): string => {
  let left = Math.floor(seconds);
  if (!Number.isFinite(left) || left <= 0) return "0s";
  const parts: string[] = [];
  for (const [, size, letter] of SPANS) {
    if (left >= size) {
      parts.push(`${Math.floor(left / size)}${letter}`);
      left %= size;
    }
    if (parts.length === 2) break;
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

/** A fraction as a percentage to one decimal, 0.256 as "25.6%" */
export const formatPercent = (fraction: number): string =>
  `${(Number.isFinite(fraction) ? fraction * 100 : 0).toFixed(1)}%`;

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

/** "3 hours ago", relative to `now`, for an instant from the API */
export const ago = (iso: string, now = Date.now()): string => {
  const secs = Math.round((Date.parse(iso) - now) / 1000);
  const [unit, size] = SPANS.find(([, s]) => Math.abs(secs) >= s) ?? ["second", 1];
  return relative.format(Math.round(secs / size), unit);
};

/** An instant from the API in the browser's time zone, for a details view */
export const formatDate = (iso: string): string => new Date(iso).toLocaleString();

/** An instant in the browser's time zone, short, for tables: "Mar 14, 09:05" */
export const dateTime = (iso: string): string =>
  new Date(iso).toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  });

/** The time of day of an instant in the browser's time zone: "09:05" */
export const shortTime = (iso: string): string =>
  new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** The day of an instant, written out, for the line between two days of chat */
export const formatDay = (iso: string): string =>
  new Date(iso).toLocaleDateString([], {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric"
  });

/** A chat message's time: the time of day for today, with the date before today */
export const chatTime = (iso: string, now = Date.now()): string =>
  new Date(iso).toDateString() === new Date(now).toDateString() ? shortTime(iso) : dateTime(iso);

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
 * The directory part of a remote path, by the separator it uses: `/` when it has one, else `\`.
 * A separator at the very start is not a cut, so "\\foo" stays as it is
 */
export const getDirectoryName = (path: string): string => {
  const separator = path.includes("/") ? "/" : "\\";
  const cut = path.lastIndexOf(separator);
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
  `${n.toLocaleString()} ${n === 1 ? one : many}`;

const LIST = new Intl.ListFormat("en", { type: "conjunction" });

/** Parts joined as a sentence does, "a, b, and c" */
export const listText = (parts: string[]) => LIST.format(parts);

export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Hands `data` to the browser as a file to save */
export const downloadFile = (
  data: BlobPart,
  filename: string,
  mime = "application/octet-stream"
) => {
  const url = URL.createObjectURL(new Blob([data], { type: mime }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
};

/** The widest text each formatter writes, which a column of its values must hold */
export const WIDEST = {
  size: formatBytes(1023.99 * 1024 ** 3),
  speed: formatSpeed(1023.9 * 1024 ** 2),
  count: (99_999).toLocaleString(),
  length: formatSeconds(99 * 3600 + 59 * 60 + 59),
  duration: formatDuration(23 * 3600 + 59 * 60),
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
