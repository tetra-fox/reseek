import { FILE_COLUMNS } from "#lib/files/columns.js";
import { columnWidth, formatCount, getDirectoryName, getFileName, WIDEST } from "#lib/format.js";
import type { GridColumn } from "#lib/lists/table.svelte.js";
import { fields, stored } from "#lib/stored.svelte.js";
import { scopedKey } from "#lib/tree/folders.js";

import { type Result, type ResultFile, resultFiles } from "./results";

/** One file of the results' list layout, with what its user offers */
export type ResultRow = {
  /**
   * the result's key and the file's path, which no two rows share: the file's id in the
   * selection, the same in the tree layout
   */
  key: string;
  username: string;
  country: string | null;
  speed: number;
  queue: number;
  free: boolean;
  file: ResultFile;
  /** the folder the file is in, "" for a bare file name, which some peers send */
  folder: string;
};

/** The results as one row per file, in the order the results are in */
export const resultRows = (results: Result[]): ResultRow[] =>
  results.flatMap(({ key, response }) =>
    resultFiles(response).map((file) => ({
      key: scopedKey(key, file.filename),
      username: response.username,
      country: response.country ?? null,
      speed: response.uploadSpeed,
      queue: response.queueLength,
      free: response.hasFreeUploadSlot,
      file,
      folder: getDirectoryName(file.filename)
    }))
  );

export type ResultColumn =
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
 * The list layout's columns in order, with their default widths as CSS lengths that grow with the
 * text: a column of figures holds its widest value, the others are in rem. The file and its
 * folder take what the row has left, from a narrow least width, so the row fits a wide screen
 * with the actions in view. The checkbox and the actions sit either side. The user, the file and
 * its size always show: a file's cells put its size under its name when there is no size column,
 * which a line of the list has no room for
 */
export const RESULT_COLUMNS: (GridColumn<ResultRow> & {
  id: ResultColumn | "select" | "actions";
})[] = [
  { id: "select", label: "Select", width: "2.5rem", quiet: true, fixed: true, always: true },
  {
    id: "user",
    label: "User",
    width: "8.75rem",
    always: true,
    value: (r) => r.username.toLowerCase()
  },
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
    // a user's queue runs to thousands, not the millions a count of files can
    width: columnWidth(formatCount(99_999)),
    end: true,
    value: (r) => r.queue
  },
  { id: "free", label: "Free slot", width: "4.5rem", value: (r) => (r.free ? 1 : 0) },
  {
    id: "file",
    label: "File",
    width: "8rem",
    grow: true,
    always: true,
    value: (r) => getFileName(r.file.filename).toLowerCase()
  },
  {
    id: "size",
    label: "Size",
    width: columnWidth(WIDEST.size),
    end: true,
    always: true,
    value: (r) => r.file.size
  },
  {
    id: "bitrate",
    label: FILE_COLUMNS.quality.label,
    width: "7rem",
    value: (r) => r.file.bitRate ?? -1
  },
  {
    id: "length",
    label: "Length",
    width: columnWidth(WIDEST.length),
    end: true,
    value: (r) => r.file.length ?? -1
  },
  {
    id: "folder",
    label: "Folder",
    width: "8rem",
    grow: true,
    value: (r) => r.folder.toLowerCase()
  },
  { id: "actions", label: "Actions", width: "4.5rem", quiet: true, fixed: true, always: true }
];

export type ResultListSort = { id: ResultColumn; dir: "asc" | "desc" } | null;

/** The columns hidden until the user shows them: the folder, which the tree layout shows */
const HIDDEN: ResultColumn[] = ["folder"];

const readResultList = (saved: unknown) => {
  const s = fields(saved);
  const sort = fields(s.sort);
  const dragged = fields(s.dragged);
  const hidden = Array.isArray(s.hidden) ? s.hidden : HIDDEN;
  const known = RESULT_COLUMNS.find((c) => c.value && c.id === sort.id);
  return {
    sort: (known
      ? { id: known.id, dir: sort.dir === "desc" ? "desc" : "asc" }
      : null) as ResultListSort,
    dragged: Object.fromEntries(
      RESULT_COLUMNS.filter((c) => !c.fixed).flatMap((c) => {
        const w = dragged[c.id];
        return typeof w === "number" && w >= 40 ? [[c.id, w]] : [];
      })
    ) as Partial<Record<ResultColumn, number>>,
    hidden: RESULT_COLUMNS.filter((c) => !c.always && hidden.includes(c.id)).map(
      (c) => c.id
    ) as ResultColumn[]
  };
};

/**
 * The list layout's sort, the columns the user hid and the widths they dragged columns to, in
 * pixels, in this browser, once kept as `reseek.search.dense`. Only a drag is kept, so a column
 * never dragged follows its default as that changes
 */
export const resultList = stored("reseek.search.list", readResultList, "reseek.search.dense");

/** The list layout as it is with nothing saved, which Reset view goes back to */
export const resultListDefaults = () => readResultList(undefined);
