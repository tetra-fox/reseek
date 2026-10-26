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

/** What a file's attributes say about its quality: "16/44.1kHz", "320 Kbps", "245 Kbps, VBR" */
export const formatAttributes = (file: {
  bitRate?: number | null;
  isVariableBitRate?: boolean | null;
  bitDepth?: number | null;
  sampleRate?: number | null;
}): string => {
  const { bitRate, isVariableBitRate, bitDepth, sampleRate } = file;
  if (sampleRate && bitDepth) return `${bitDepth}/${sampleRate / 1000}kHz`;
  if (isVariableBitRate) return bitRate ? `${bitRate} Kbps, VBR` : "VBR";
  if (bitRate) return `${bitRate} Kbps`;
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

/** `n` with the noun for that many, "1 file" or "2,048 files" */
export const plural = (n: number, one: string, many: string): string =>
  `${n.toLocaleString()} ${n === 1 ? one : many}`;

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
