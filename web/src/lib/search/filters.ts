/**
 * The result filter typed above a search's results: words a file name must or must not
 * contain, and tokens that keep only files of a kind, a type, a country, or with a size, bit
 * rate, bit depth or length in some range
 */
export type Filters = {
  include: string[];
  exclude: string[];
  isCBR: boolean;
  isVBR: boolean;
  isLossless: boolean;
  isLossy: boolean;
  minBitRate: number;
  minBitDepth: number;
  minFileSize: number;
  minLength: number;
  minFilesInFolder: number;
  /** `size:`, `bitrate:`, `bitdepth:` and `length:` tokens, every one of which must hold */
  conditions: Condition[];
  /** `type:` tokens, each a set of extensions */
  types: Choice[];
  /** `country:` tokens, each a set of country codes */
  countries: Choice[];
};

/** The attributes a field token compares */
export type Field = "size" | "bitrate" | "bitdepth" | "length";

/** One way a value can pass: at least, more than, at most, less than, exactly, not, or between */
export type Comparison =
  | { op: ">=" | ">" | "<=" | "<" | "=" | "!="; value: number }
  | { op: "range"; from: number; to: number };

/** A field token: the value passes when any of its alternatives does, the reverse when negated */
export type Condition = { field: Field; alternatives: Comparison[]; negated: boolean };

/** A `type:` or `country:` token: one of the values, or with `!` none of them */
export type Choice = { values: string[]; negated: boolean };

type Minimum = "minBitRate" | "minBitDepth" | "minFileSize" | "minLength" | "minFilesInFolder";

/** The minimum tokens by both of their spellings */
const MINIMUMS: [RegExp, Minimum][] = [
  [/^(?:minbr|minbitrate):(\d+)$/, "minBitRate"],
  [/^(?:minbd|minbitdepth):(\d+)$/, "minBitDepth"],
  [/^(?:minfs|minfilesize):(\d+)$/, "minFileSize"],
  [/^(?:minlen|minlength):(\d+)$/, "minLength"],
  [/^(?:minfif|minfilesinfolder):(\d+)$/, "minFilesInFolder"]
];

const FLAGS = new Map<string, "isCBR" | "isVBR" | "isLossless" | "isLossy">([
  ["iscbr", "isCBR"],
  ["isvbr", "isVBR"],
  ["islossless", "isLossless"],
  ["islossy", "isLossy"]
]);

/** Field token names, short and long */
const FIELDS = new Map<string, Field>([
  ["size", "size"],
  ["bitrate", "bitrate"],
  ["br", "bitrate"],
  ["bitdepth", "bitdepth"],
  ["bd", "bitdepth"],
  ["length", "length"],
  ["len", "length"],
  ["duration", "length"]
]);

const CHOICES = new Map<string, "types" | "countries">([
  ["type", "types"],
  ["ext", "types"],
  ["country", "countries"]
]);

/** Size units, in binary multiples like the sizes the results show */
const SIZE_UNITS: Record<string, number> = {
  "": 1,
  "b": 1,
  "k": 1024,
  "kb": 1024,
  "kib": 1024,
  "m": 1024 ** 2,
  "mb": 1024 ** 2,
  "mib": 1024 ** 2,
  "g": 1024 ** 3,
  "gb": 1024 ** 3,
  "gib": 1024 ** 3,
  "t": 1024 ** 4,
  "tb": 1024 ** 4,
  "tib": 1024 ** 4
};

/** A field's value as typed: a size with its unit, a length as seconds or `M:SS`, a number */
const readNumber = (field: Field, text: string): number | null => {
  if (field === "size") {
    const match = /^(\d+(?:\.\d+)?)([a-z]*)$/.exec(text);
    const unit = match ? SIZE_UNITS[match[2]] : undefined;
    return match && unit ? Math.round(Number.parseFloat(match[1]) * unit) : null;
  }
  if (field === "length") {
    const clock = /^(?:(\d+):)?(\d+):(\d{1,2})$/.exec(text);
    if (clock) {
      const [, hours, minutes, seconds] = clock;
      return Number(hours ?? 0) * 3600 + Number(minutes) * 60 + Number(seconds);
    }
  }
  return /^\d+$/.test(text) ? Number.parseInt(text, 10) : null;
};

/** One alternative of a field's value: `>N`, `<=N`, `=N`, `!N`, `A..B`, or `N` for at least N */
const readComparison = (field: Field, text: string): Comparison | null => {
  const range = /^(.*)\.\.(.*)$/.exec(text);
  if (range) {
    const from = range[1] ? readNumber(field, range[1]) : 0;
    const to = range[2] ? readNumber(field, range[2]) : Number.POSITIVE_INFINITY;
    if (from === null || to === null || (!range[1] && !range[2])) return null;
    return { op: "range", from, to };
  }
  const [, sign, rest] = /^(>=|<=|>|<|=|!)?(.*)$/.exec(text)!;
  const value = readNumber(field, rest);
  if (value === null) return null;
  const op = sign === "!" ? "!=" : ((sign ?? ">=") as ">=" | ">" | "<=" | "<" | "=");
  return { op, value };
};

/**
 * Adds a token `name:value`, with `!` or `-` before it for "not", as a field or a choice. False
 * for a name it doesn't know or a value it can't read, which leaves the token out
 */
const readToken = (token: string, filters: Filters): boolean => {
  const negated = token.startsWith("!") || token.startsWith("-");
  const body = negated ? token.slice(1) : token;
  const cut = body.indexOf(":");
  if (cut <= 0) return false;
  const name = body.slice(0, cut);
  const value = body.slice(cut + 1);
  const alternatives = value.split("|").filter(Boolean);
  if (!alternatives.length) return false;
  const field = FIELDS.get(name);
  if (field) {
    const read = alternatives.map((a) => readComparison(field, a));
    if (read.some((c) => c === null)) return false;
    filters.conditions.push({ field, alternatives: read as Comparison[], negated });
    return true;
  }
  const choice = CHOICES.get(name);
  if (!choice) return false;
  const values = alternatives.map((a) => (choice === "types" ? a.replace(/^\./, "") : a));
  filters[choice].push({ values, negated });
  return true;
};

/** Every token the filter box understands, for its help */
export const FILTER_TOKENS: { token: string; meaning: string }[] = [
  { token: "word", meaning: "The path contains the word." },
  { token: "-word", meaning: "The path does not contain the word." },
  {
    token: "type:flac|mp3",
    meaning: "The file type is one of those listed. !type:mp3 excludes a type."
  },
  { token: "size:10m", meaning: "At least 10 MB. Units: k, m, g." },
  { token: "size:<1g", meaning: "Less than 1 GB. Also >, <=, = and ! (not)." },
  { token: "size:5m..50m", meaning: "Between 5 and 50 MB." },
  { token: "bitrate:320", meaning: "At least 320 kbps. Short form: br:." },
  { token: "bitrate:=256|=320", meaning: "Exactly 256 or 320 kbps." },
  { token: "bitdepth:24", meaning: "At least 24 bits. Short form: bd:." },
  { token: "length:3:00..6:00", meaning: "Between 3 and 6 minutes. Seconds are also accepted." },
  {
    token: "country:NL|BE",
    meaning: "The user is in one of the listed countries. !country:NL excludes a country."
  },
  { token: "islossless, islossy", meaning: "Lossless or lossy files only." },
  { token: "iscbr, isvbr", meaning: "Constant or variable bit rate only." },
  { token: "minbr:, minbd:, minfs:, minlen:", meaning: "Older minimum filters, still accepted." },
  { token: "minfif:8", meaning: "Users with at least 8 matching files." }
];

/** The filter box's placeholder: a few tokens to show what it takes */
export const FILTER_PLACEHOLDER =
  "album -live type:flac|mp3 bitrate:320 size:<1g length:3:00..8:00";

/**
 * Reads a filter. Tokens are split on whitespace and compared in lower case. A `min…:N` token
 * sets that minimum (the first of a kind wins, 0 leaves it off), the four flags set their kind,
 * `field:value`, `type:` and `country:` tokens add a condition, any other token with a colon is
 * ignored, `-word` excludes and every other word includes
 */
export const parseFilters = (text: string): Filters => {
  const filters: Filters = {
    include: [],
    exclude: [],
    isCBR: false,
    isVBR: false,
    isLossless: false,
    isLossy: false,
    minBitRate: 0,
    minBitDepth: 0,
    minFileSize: 0,
    minLength: 0,
    minFilesInFolder: 0,
    conditions: [],
    types: [],
    countries: []
  };
  const seen = new Set<Minimum>();
  for (const token of text.toLowerCase().split(/\s+/)) {
    if (!token) continue;
    const flag = FLAGS.get(token);
    if (flag) {
      filters[flag] = true;
      continue;
    }
    const minimum = MINIMUMS.find(([pattern]) => pattern.test(token));
    if (minimum) {
      const [pattern, field] = minimum;
      if (!seen.has(field)) filters[field] = Number.parseInt(pattern.exec(token)![1], 10);
      seen.add(field);
      continue;
    }
    if (token.includes(":")) {
      readToken(token, filters);
      continue;
    }
    if (token.startsWith("-")) {
      if (token.length > 1) filters.exclude.push(token.slice(1));
    } else {
      filters.include.push(token);
    }
  }
  return filters;
};

/** The attributes the filter looks at; a missing one never fails a numeric condition */
export type FilterFile = {
  filename: string;
  size: number;
  extension?: string | null;
  bitRate?: number | null;
  bitDepth?: number | null;
  sampleRate?: number | null;
  length?: number | null;
  isVariableBitRate?: boolean | null;
};

const below = (value: number | null | undefined, minimum: number) =>
  value !== null && value !== undefined && value < minimum;

const passes = (c: Comparison, value: number) => {
  switch (c.op) {
    case ">=":
      return value >= c.value;
    case ">":
      return value > c.value;
    case "<=":
      return value <= c.value;
    case "<":
      return value < c.value;
    case "=":
      return value === c.value;
    case "!=":
      return value !== c.value;
    case "range":
      return value >= c.from && value <= c.to;
  }
};

const VALUE: Record<Field, (file: FilterFile) => number | null | undefined> = {
  size: (f) => f.size,
  bitrate: (f) => f.bitRate,
  bitdepth: (f) => f.bitDepth,
  length: (f) => f.length
};

/** A file's type: its extension, or the end of its name after the last dot */
export const typeOf = (file: { filename: string; extension?: string | null }) => {
  if (file.extension) return file.extension.toLowerCase().replace(/^\./, "");
  const name = file.filename.split(/[/\\]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot + 1).toLowerCase();
};

/** Whether `value` is chosen by a `type:` or `country:` token; an unknown value is chosen by none */
const chosen = (choice: Choice, value: string | null) =>
  (value !== null && choice.values.includes(value)) !== choice.negated;

/** Whether one file passes the filter's kinds, types, numbers and words */
export const fileMatches = (filters: Filters, file: FilterFile): boolean => {
  const vbr = file.isVariableBitRate;
  if (filters.isCBR && (vbr === null || vbr === undefined || vbr)) return false;
  if (filters.isVBR && (vbr === null || vbr === undefined || !vbr)) return false;
  const lossless = !!file.sampleRate && !!file.bitDepth;
  if (filters.isLossless && !lossless) return false;
  if (filters.isLossy && (!!file.sampleRate || !!file.bitDepth)) return false;
  if (below(file.bitRate, filters.minBitRate)) return false;
  if (below(file.bitDepth, filters.minBitDepth)) return false;
  if (below(file.size, filters.minFileSize)) return false;
  if (below(file.length, filters.minLength)) return false;
  for (const condition of filters.conditions) {
    const value = VALUE[condition.field](file);
    if (value === null || value === undefined) continue;
    if (condition.alternatives.some((c) => passes(c, value)) === condition.negated) return false;
  }
  if (filters.types.length) {
    const type = typeOf(file);
    if (!filters.types.every((choice) => chosen(choice, type))) return false;
  }
  const name = file.filename.toLowerCase();
  return (
    filters.include.every((term) => name.includes(term)) &&
    !filters.exclude.some((term) => name.includes(term))
  );
};

type Filterable<F> = {
  files: F[];
  lockedFiles: F[];
  fileCount: number;
  lockedFileCount: number;
  country?: string | null;
};

/** Whether a response's user passes the `country:` tokens; one with no country passes only `!` */
export const countryMatches = (filters: Filters, country: string | null | undefined) =>
  filters.countries.every((choice) => chosen(choice, country ? country.toLowerCase() : null));

/**
 * A response with only the files that pass, its counts set to what is left. A response with
 * fewer files than `minfif` asks for, locked ones counted, or from a country the filter leaves
 * out, keeps none
 */
export const filterResponse = <F extends FilterFile, R extends Filterable<F>>(
  filters: Filters,
  response: R
): R => {
  if (
    response.fileCount + response.lockedFileCount < filters.minFilesInFolder ||
    !countryMatches(filters, response.country)
  ) {
    return { ...response, files: [], lockedFiles: [], fileCount: 0, lockedFileCount: 0 };
  }
  const files = response.files.filter((f) => fileMatches(filters, f));
  const lockedFiles = response.lockedFiles.filter((f) => fileMatches(filters, f));
  return {
    ...response,
    files,
    lockedFiles,
    fileCount: files.length,
    lockedFileCount: lockedFiles.length
  };
};
