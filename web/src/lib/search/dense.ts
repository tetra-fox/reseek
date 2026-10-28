import { columnWidth, getDirectoryName, getFileName, WIDEST } from "#lib/format.js";
import { fields, stored } from "#lib/stored.svelte.js";

import { type Result, type ResultFile, resultFiles } from "./results";

/** One file of the dense results view, with what its user offers */
export type DenseRow = {
  /** the card's key and the file's path, which no two rows share */
  key: string;
  username: string;
  country: string | null;
  speed: number;
  queue: number;
  free: boolean;
  file: ResultFile;
  folder: string;
};

/** The results as one row per file, in the order the results are in */
export const denseRows = (results: Result[]): DenseRow[] =>
  results.flatMap(({ key, response }) =>
    resultFiles(response).map((file) => ({
      key: `${key}\u0000${file.filename}`,
      username: response.username,
      country: response.country ?? null,
      speed: response.uploadSpeed,
      queue: response.queueLength,
      free: response.hasFreeUploadSlot,
      file,
      folder: getDirectoryName(file.filename)
    }))
  );

export type DenseColumn =
  | "user"
  | "speed"
  | "queue"
  | "free"
  | "file"
  | "size"
  | "bitrate"
  | "length"
  | "folder";

/**
 * The dense view's columns in order, with their default widths: pixels, or for a column of
 * figures a length that holds its widest value
 */
export const DENSE_COLUMNS: {
  id: DenseColumn;
  label: string;
  width: number | string;
  end?: boolean;
}[] = [
  { id: "user", label: "User", width: 140 },
  { id: "speed", label: "Speed", width: columnWidth(WIDEST.speed), end: true },
  { id: "queue", label: "Queue", width: columnWidth(WIDEST.count), end: true },
  { id: "free", label: "Free slot", width: 72 },
  { id: "file", label: "File", width: 280 },
  { id: "size", label: "Size", width: columnWidth(WIDEST.size), end: true },
  { id: "bitrate", label: "Bitrate", width: 110 },
  { id: "length", label: "Length", width: columnWidth(WIDEST.length), end: true },
  { id: "folder", label: "Folder", width: 260 }
];

const VALUE: Record<DenseColumn, (row: DenseRow) => number | string> = {
  user: (r) => r.username.toLowerCase(),
  speed: (r) => r.speed,
  queue: (r) => r.queue,
  free: (r) => (r.free ? 1 : 0),
  file: (r) => getFileName(r.file.filename).toLowerCase(),
  size: (r) => r.file.size,
  bitrate: (r) => r.file.bitRate ?? -1,
  length: (r) => r.file.length ?? -1,
  folder: (r) => r.folder.toLowerCase()
};

export type DenseSort = { id: DenseColumn; dir: "asc" | "desc" } | null;

/** Rows in a column's order, ties in the order they came; as they came without a sort */
export const sortDense = (rows: DenseRow[], sort: DenseSort): DenseRow[] => {
  if (!sort) return rows;
  const value = VALUE[sort.id];
  const sign = sort.dir === "asc" ? 1 : -1;
  return rows.toSorted((a, b) => {
    const x = value(a);
    const y = value(b);
    return (
      sign * (typeof x === "string" ? x.localeCompare(y as string) : (x as number) - (y as number))
    );
  });
};

/** The next sort a click on a column header gives: ascending, descending, then none */
export const nextSort = (sort: DenseSort, id: DenseColumn): DenseSort => {
  if (sort?.id !== id) return { id, dir: id === "speed" ? "desc" : "asc" };
  const first = id === "speed" ? "desc" : "asc";
  return sort.dir === first ? { id, dir: first === "asc" ? "desc" : "asc" } : null;
};

/**
 * The dense view's sort and the widths the user dragged columns to, in pixels, in this browser.
 * Only a drag is kept, so a column never dragged follows its default as that changes. Widths saved
 * under `widths` by older builds held every default too, so they are not read
 */
export const denseView = stored("reseek.search.dense", (saved) => {
  const s = fields(saved);
  const sort = fields(s.sort);
  const dragged = fields(s.dragged);
  const known = DENSE_COLUMNS.find((c) => c.id === sort.id);
  return {
    sort: (known ? { id: known.id, dir: sort.dir === "desc" ? "desc" : "asc" } : null) as DenseSort,
    dragged: Object.fromEntries(
      DENSE_COLUMNS.flatMap((c) => {
        const w = dragged[c.id];
        return typeof w === "number" && w >= 40 ? [[c.id, w]] : [];
      })
    ) as Partial<Record<DenseColumn, number>>
  };
});

/** A dense column's width: pixels the user dragged it to, else its default length */
export const denseWidth = (column: (typeof DENSE_COLUMNS)[number]): number | string =>
  denseView.dragged[column.id] ?? column.width;
