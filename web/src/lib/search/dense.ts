import { columnWidth, getDirectoryName, getFileName, WIDEST } from "#lib/format.js";
import type { GridColumn } from "#lib/lists/table.svelte.js";
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
 * The dense view's columns in order, with their default widths as CSS lengths that grow with the
 * text: a column of figures holds its widest value, the others are in rem. The checkbox and the
 * actions sit either side
 */
export const DENSE_COLUMNS: (GridColumn<DenseRow> & { id: DenseColumn | "select" | "actions" })[] =
  [
    { id: "select", label: "Select", width: "2.5rem", quiet: true, fixed: true },
    { id: "user", label: "User", width: "8.75rem", value: (r) => r.username.toLowerCase() },
    {
      id: "speed",
      label: "Speed",
      width: columnWidth(WIDEST.speed),
      end: true,
      descFirst: true,
      value: (r) => r.speed
    },
    {
      id: "queue",
      label: "Queue",
      width: columnWidth(WIDEST.count),
      end: true,
      value: (r) => r.queue
    },
    { id: "free", label: "Free slot", width: "5.5rem", value: (r) => (r.free ? 1 : 0) },
    {
      id: "file",
      label: "File",
      width: "17.5rem",
      value: (r) => getFileName(r.file.filename).toLowerCase()
    },
    {
      id: "size",
      label: "Size",
      width: columnWidth(WIDEST.size),
      end: true,
      value: (r) => r.file.size
    },
    { id: "bitrate", label: "Bitrate", width: "7rem", value: (r) => r.file.bitRate ?? -1 },
    {
      id: "length",
      label: "Length",
      width: columnWidth(WIDEST.length),
      end: true,
      value: (r) => r.file.length ?? -1
    },
    { id: "folder", label: "Folder", width: "16.25rem", value: (r) => r.folder.toLowerCase() },
    { id: "actions", label: "Actions", width: "4.5rem", quiet: true, fixed: true }
  ];

export type DenseSort = { id: DenseColumn; dir: "asc" | "desc" } | null;

/**
 * The dense view's sort and the widths the user dragged columns to, in pixels, in this browser.
 * Only a drag is kept, so a column never dragged follows its default as that changes
 */
export const denseView = stored("reseek.search.dense", (saved) => {
  const s = fields(saved);
  const sort = fields(s.sort);
  const dragged = fields(s.dragged);
  const known = DENSE_COLUMNS.find((c) => c.value && c.id === sort.id);
  return {
    sort: (known ? { id: known.id, dir: sort.dir === "desc" ? "desc" : "asc" } : null) as DenseSort,
    dragged: Object.fromEntries(
      DENSE_COLUMNS.filter((c) => !c.fixed).flatMap((c) => {
        const w = dragged[c.id];
        return typeof w === "number" && w >= 40 ? [[c.id, w]] : [];
      })
    ) as Partial<Record<DenseColumn, number>>
  };
});
