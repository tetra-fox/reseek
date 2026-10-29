import { columnWidth, WIDEST } from "#lib/format.js";
import { TICK_REM } from "#lib/tree/indent.js";

/** The cells a remote file's row can have, in the order they are drawn */
export type FileColumn = "tick" | "name" | "size" | "quality" | "length" | "menu";

/** Each column's header label and its width as a grid track */
export const FILE_COLUMNS: Record<FileColumn, { label: string; width: string }> = {
  tick: { label: "Select", width: `${TICK_REM}rem` },
  name: { label: "Name", width: "minmax(0, 1fr)" },
  size: { label: "Size", width: columnWidth(WIDEST.size) },
  quality: { label: "Quality", width: "9rem" },
  length: { label: "Length", width: columnWidth(WIDEST.length) },
  menu: { label: "Actions", width: "2.5rem" }
};

/**
 * The columns of a list of remote files. A list whose files say nothing of their quality has no
 * quality column; a phone shows the tick, the name with the size under it, and the menu
 */
export const fileColumns = ({ quality, phone }: { quality: boolean; phone: boolean }) =>
  (phone
    ? ["tick", "name", "menu"]
    : ["tick", "name", "size", ...(quality ? ["quality"] : []), "length", "menu"]) as FileColumn[];

/** The grid template of the columns */
export const fileTemplate = (columns: FileColumn[]) =>
  columns.map((c) => FILE_COLUMNS[c].width).join(" ");
