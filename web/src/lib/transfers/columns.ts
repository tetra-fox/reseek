import { SPARKLINE_WIDTH } from "#lib/components/transfers/Sparkline.svelte";
import { columnWidth, WIDEST } from "#lib/format.js";

import type { Direction } from "./states";
import type { SortKey } from "./view";

export type ColumnId =
  | "select"
  | "file"
  | "user"
  | "size"
  | "progress"
  | "speed"
  | "eta"
  | "transferred"
  | "queue"
  | "requested"
  | "started"
  | "ended"
  | "elapsed"
  | "attempts"
  | "batch"
  | "group"
  | "actions";

export type Column = {
  id: ColumnId;
  label: string;
  /** shown until the user hides it */
  visible: boolean;
  /**
   * pixels, or for a column of figures a length that holds its widest value; the file column
   * takes what is left on top of it
   */
  width: number | string;
  /** what sorting by it orders by, none for the selector and the actions */
  sort?: SortKey;
  /** the only direction it is offered in */
  only?: Direction;
  /** always shown, never in the picker */
  fixed?: boolean;
};

/** The table's columns in order, with their defaults */
export const COLUMNS: Column[] = [
  { id: "select", label: "Select", visible: true, width: 40, fixed: true },
  { id: "file", label: "File", visible: true, width: 240, sort: "file", fixed: true },
  { id: "user", label: "User", visible: true, width: 150, sort: "user" },
  { id: "size", label: "Size", visible: true, width: columnWidth(WIDEST.size), sort: "size" },
  { id: "progress", label: "Progress", visible: true, width: 180, sort: "progress" },
  {
    id: "speed",
    label: "Speed",
    visible: true,
    // the speed with the line of recent speeds and its gap before it
    width: columnWidth(WIDEST.speed, `${SPARKLINE_WIDTH}px + 0.375rem`),
    sort: "speed"
  },
  { id: "eta", label: "ETA", visible: true, width: columnWidth(WIDEST.duration), sort: "eta" },
  {
    id: "transferred",
    label: "Transferred",
    visible: false,
    width: columnWidth(WIDEST.transferred),
    sort: "size"
  },
  { id: "queue", label: "Queue", visible: false, width: 120 },
  { id: "requested", label: "Requested", visible: false, width: 130, sort: "requested" },
  { id: "started", label: "Started", visible: false, width: 130, sort: "started" },
  { id: "ended", label: "Ended", visible: false, width: 130, sort: "ended" },
  { id: "elapsed", label: "Elapsed", visible: false, width: columnWidth(WIDEST.duration) },
  { id: "attempts", label: "Attempts", visible: false, width: 80, sort: "attempts" },
  { id: "batch", label: "Batch", visible: false, width: 100, only: "download" },
  { id: "group", label: "Group", visible: false, width: 110, only: "upload" },
  { id: "actions", label: "Actions", visible: true, width: 160, fixed: true }
];

/** The columns a direction offers */
export const columnsFor = (direction: Direction) =>
  COLUMNS.filter((c) => !c.only || c.only === direction);

/**
 * The columns a phone shows, with the widths it gives them: a file's name, its size, how far it
 * is, and the menu that holds every action, so nothing needs a sideways scroll
 */
export const PHONE_COLUMNS: Partial<Record<ColumnId, string>> = {
  file: "4rem",
  size: columnWidth(WIDEST.size),
  progress: "6.5rem",
  actions: "2.5rem"
};
