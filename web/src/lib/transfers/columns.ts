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
  /** pixels; the file column takes what is left on top of it */
  width: number;
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
  { id: "file", label: "File", visible: true, width: 280, sort: "file", fixed: true },
  { id: "user", label: "User", visible: true, width: 150, sort: "user" },
  { id: "size", label: "Size", visible: true, width: 90, sort: "size" },
  { id: "progress", label: "Progress", visible: true, width: 200, sort: "progress" },
  { id: "speed", label: "Speed", visible: true, width: 130, sort: "speed" },
  { id: "eta", label: "ETA", visible: true, width: 80, sort: "eta" },
  { id: "transferred", label: "Transferred", visible: false, width: 120, sort: "size" },
  { id: "queue", label: "Queue", visible: false, width: 120 },
  { id: "requested", label: "Requested", visible: false, width: 130, sort: "requested" },
  { id: "started", label: "Started", visible: false, width: 130, sort: "started" },
  { id: "ended", label: "Ended", visible: false, width: 130, sort: "ended" },
  { id: "elapsed", label: "Elapsed", visible: false, width: 90 },
  { id: "attempts", label: "Attempts", visible: false, width: 80, sort: "attempts" },
  { id: "batch", label: "Batch", visible: false, width: 100, only: "download" },
  { id: "group", label: "Group", visible: false, width: 110, only: "upload" },
  { id: "actions", label: "Actions", visible: true, width: 150, fixed: true }
];

/** The columns a direction offers */
export const columnsFor = (direction: Direction) =>
  COLUMNS.filter((c) => !c.only || c.only === direction);
