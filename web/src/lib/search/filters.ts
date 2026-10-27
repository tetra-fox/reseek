/**
 * The result filter typed above a search's results: words a file name must or must not
 * contain, and tokens that keep only files of a kind or above a size, length or quality
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
};

type Minimum = "minBitRate" | "minBitDepth" | "minFileSize" | "minLength" | "minFilesInFolder";

/** The minimum tokens by both of their spellings */
const MINIMUMS: [RegExp, Minimum][] = [
  [/^(?:minbr|minbitrate):(\d+)$/, "minBitRate"],
  [/^(?:minbd|minbitdepth):(\d+)$/, "minBitDepth"],
  [/^(?:minfs|minfilesize):(\d+)$/, "minFileSize"],
  [/^(?:minlen|minlength):(\d+)$/, "minLength"],
  [/^(?:minfif|minfilesinfolder):(\d+)$/, "minFilesInFolder"]
];

const FLAGS = {
  iscbr: "isCBR",
  isvbr: "isVBR",
  islossless: "isLossless",
  islossy: "isLossy"
} as const;

/** What the help under the filter box shows, which is also its placeholder */
export const FILTER_HELP =
  "lackluster container -bothersome iscbr|isvbr islossless|islossy minbitrate:320 minbitdepth:24 minfilesize:10 minfilesinfolder:8 minlength:5000";

/**
 * Reads a filter. Tokens are split on whitespace and compared in lower case. A `min…:N` token
 * sets that minimum (the first of a kind wins, 0 leaves it off), the four flags set their kind,
 * any other token with a colon is ignored, `-word` excludes and every other word includes
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
    minFilesInFolder: 0
  };
  const seen = new Set<Minimum>();
  for (const token of text.toLowerCase().split(/\s+/)) {
    if (!token) continue;
    if (token in FLAGS) {
      filters[FLAGS[token as keyof typeof FLAGS]] = true;
      continue;
    }
    const minimum = MINIMUMS.find(([pattern]) => pattern.test(token));
    if (minimum) {
      const [pattern, field] = minimum;
      if (!seen.has(field)) filters[field] = Number.parseInt(pattern.exec(token)![1], 10);
      seen.add(field);
      continue;
    }
    if (token.includes(":")) continue;
    if (token.startsWith("-")) {
      if (token.length > 1) filters.exclude.push(token.slice(1));
    } else {
      filters.include.push(token);
    }
  }
  return filters;
};

/** The attributes the filter looks at; a missing one never fails a minimum */
export type FilterFile = {
  filename: string;
  size: number;
  bitRate?: number | null;
  bitDepth?: number | null;
  sampleRate?: number | null;
  length?: number | null;
  isVariableBitRate?: boolean | null;
};

const below = (value: number | null | undefined, minimum: number) =>
  value !== null && value !== undefined && value < minimum;

/** Whether one file passes the filter's kinds, minimums and words */
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
};

/**
 * A response with only the files that pass, its counts set to what is left. A response with
 * fewer files than `minfif` asks for, locked ones counted, keeps none
 */
export const filterResponse = <F extends FilterFile, R extends Filterable<F>>(
  filters: Filters,
  response: R
): R => {
  if (response.fileCount + response.lockedFileCount < filters.minFilesInFolder) {
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
