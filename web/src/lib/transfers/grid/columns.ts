import { columnWidth, getFileName, WIDEST } from "#lib/format.js";
import type { GridColumn } from "#lib/lists/table.svelte.js";
import type { Direction, Transfer } from "#lib/transfers/states.js";

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
  | "job"
  | "group"
  | "actions";

export type Column = GridColumn<Transfer> & {
  id: ColumnId;
  /** shown until the user hides it */
  visible: boolean;
  /** what sorting by it orders by, none for the selector and the actions */
  sort?: SortKey;
  /** the only direction it is offered in */
  only?: Direction;
  /** always shown, never in the picker */
  always?: boolean;
};

/** How wide the select column is, in rem; a phone leaves it out */
export const SELECT_REM = 2.5;

/** How wide a transfer's speed sparkline is drawn, in pixels, which its column makes room for */
export const SPARKLINE_WIDTH = 48;

const time = (iso: string | null | undefined) => (iso ? Date.parse(iso) : 0);

/** What each sort orders the transfers by */
const VALUE: Record<SortKey, (t: Transfer) => number | string> = {
  file: (t) => getFileName(t.filename).toLowerCase(),
  user: (t) => t.username.toLowerCase(),
  size: (t) => t.size,
  progress: (t) => t.percentComplete,
  speed: (t) => t.averageSpeed,
  eta: (t) => t.remainingTime ?? Number.MAX_SAFE_INTEGER,
  requested: (t) => time(t.requestedAt),
  started: (t) => time(t.startedAt),
  ended: (t) => time(t.endedAt),
  attempts: (t) => t.attempts
};

/** The table's columns in order, with their defaults */
export const COLUMNS: Column[] = (
  [
    {
      id: "select",
      label: "Select",
      visible: true,
      width: `${SELECT_REM}rem`,
      always: true,
      quiet: true,
      fixed: true
    },
    {
      id: "file",
      label: "File",
      visible: true,
      width: "15rem",
      sort: "file",
      always: true,
      grow: true
    },
    { id: "user", label: "User", visible: true, width: "9.5rem", sort: "user" },
    {
      id: "size",
      label: "Size",
      visible: true,
      width: columnWidth(WIDEST.size),
      sort: "size",
      end: true
    },
    { id: "progress", label: "Progress", visible: true, width: "11.25rem", sort: "progress" },
    {
      id: "speed",
      label: "Speed",
      visible: true,
      // the speed with the line of recent speeds and its gap before it
      width: columnWidth(WIDEST.speed, `${SPARKLINE_WIDTH}px + 0.375rem`),
      sort: "speed",
      end: true
    },
    {
      id: "eta",
      label: "ETA",
      visible: true,
      width: columnWidth(WIDEST.duration),
      sort: "eta",
      end: true
    },
    {
      id: "transferred",
      label: "Transferred",
      visible: false,
      width: columnWidth(WIDEST.transferred),
      sort: "size",
      sortsAs: "size",
      end: true
    },
    { id: "queue", label: "Queue", visible: false, width: "7.5rem" },
    { id: "requested", label: "Requested", visible: false, width: "8.25rem", sort: "requested" },
    { id: "started", label: "Started", visible: false, width: "8.25rem", sort: "started" },
    { id: "ended", label: "Ended", visible: false, width: "8.25rem", sort: "ended" },
    {
      id: "elapsed",
      label: "Elapsed",
      visible: false,
      width: columnWidth(WIDEST.duration),
      end: true
    },
    {
      id: "attempts",
      label: "Attempts",
      visible: false,
      width: "5.5rem",
      sort: "attempts",
      end: true
    },
    { id: "job", label: "Download job", visible: false, width: "6.25rem", only: "download" },
    { id: "group", label: "Group", visible: false, width: "7rem", only: "upload" },
    {
      id: "actions",
      label: "Actions",
      visible: true,
      width: "10rem",
      always: true,
      quiet: true,
      fixed: true
    }
  ] satisfies Column[]
).map((c: Column) => ({ ...c, value: c.sort && !c.sortsAs ? VALUE[c.sort] : undefined }));

/** The columns a direction offers */
export const columnsFor = (direction: Direction) =>
  COLUMNS.filter((c) => !c.only || c.only === direction);

/**
 * The columns a phone shows, with the widths it gives them: the file, with its size and progress
 * drawn under its name, and the menu that holds every action, so nothing needs a sideways scroll
 */
export const PHONE_COLUMNS: Partial<Record<ColumnId, string>> = {
  file: "0rem",
  actions: "2.5rem"
};

/** A grid template of the columns `ids`, each at its default width, the file taking the rest */
export const templateOf = (ids: ColumnId[]) =>
  ids
    .map((id) => {
      const width = COLUMNS.find((c) => c.id === id)!.width;
      return id === "file" ? `minmax(${width}, 1fr)` : width;
    })
    .join(" ");
