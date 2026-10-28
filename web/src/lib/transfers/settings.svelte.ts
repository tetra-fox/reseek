import { fields, stored } from "#lib/stored.svelte.js";

import { type ColumnId, columnsFor } from "./columns";
import { type ChipId, type Direction, STATE_CHIPS } from "./states";
import type { GroupBy, SortKey } from "./view";

export type ViewSettings = {
  groupBy: GroupBy;
  density: "comfortable" | "compact";
  columns: { id: ColumnId; width: number | string; visible: boolean }[];
  sort: { id: SortKey; dir: "asc" | "desc" };
  filters: { states: ChipId[] };
};

const GROUPINGS: GroupBy[] = ["user", "folder", "batch", "flat"];
const SORTS: SortKey[] = [
  "file",
  "user",
  "size",
  "progress",
  "speed",
  "eta",
  "requested",
  "started",
  "ended",
  "attempts"
];

/** The settings a direction starts with, and goes back to on reset */
export const defaults = (direction: Direction): ViewSettings => ({
  groupBy: "user",
  density: "comfortable",
  columns: columnsFor(direction).map(({ id, width, visible }) => ({ id, width, visible })),
  sort: { id: "file", dir: "asc" },
  filters: { states: [] }
});

/**
 * What was saved as far as it fits: every column of the direction once, in the saved order with
 * new columns after, and anything unknown dropped. The text filter is never saved
 */
const read = (direction: Direction, saved: unknown): ViewSettings => {
  const base = defaults(direction);
  const s = fields(saved);
  const savedColumns = Array.isArray(s.columns) ? s.columns.map(fields) : [];
  const columns = savedColumns
    .flatMap((c) => {
      const known = base.columns.find((b) => b.id === c.id);
      if (!known) return [];
      return [
        {
          id: known.id,
          width: typeof c.width === "number" && c.width >= 40 ? c.width : known.width,
          visible: typeof c.visible === "boolean" ? c.visible : known.visible
        }
      ];
    })
    .filter((c, i, all) => all.findIndex((o) => o.id === c.id) === i);
  for (const c of base.columns) if (!columns.some((o) => o.id === c.id)) columns.push(c);
  const sort = fields(s.sort);
  const states = fields(s.filters).states;
  return {
    groupBy: GROUPINGS.includes(s.groupBy as GroupBy) ? (s.groupBy as GroupBy) : base.groupBy,
    density: s.density === "compact" ? "compact" : "comfortable",
    columns,
    sort: {
      id: SORTS.includes(sort.id as SortKey) ? (sort.id as SortKey) : base.sort.id,
      dir: sort.dir === "desc" ? "desc" : "asc"
    },
    filters: {
      states: Array.isArray(states)
        ? STATE_CHIPS.map((c) => c.id).filter((id) => states.includes(id))
        : []
    }
  };
};

/** How each transfers screen is laid out in this browser */
export const views = {
  download: stored("transfers.view.download", (saved) => read("download", saved)),
  upload: stored("transfers.view.upload", (saved) => read("upload", saved))
};
