import { MARK_FILTERS, type MarkFilter } from "#lib/analysis/marks.js";
import { fields, stored } from "#lib/stored.svelte.js";
import { type ChipId, type Direction, STATE_CHIPS } from "#lib/transfers/states.js";

import { type ColumnId, columnsFor } from "./columns";
import type { GroupBy, SortKey } from "./view";

export type ViewSettings = {
  groupBy: GroupBy;
  density: "comfortable" | "compact";
  /** every column in the order shown, with the width in pixels the user dragged it to, if any */
  columns: { id: ColumnId; visible: boolean; dragged?: number }[];
  sort: { id: SortKey; dir: "asc" | "desc" };
  /** the state chips chosen, and the analysis chip on the downloads screen, null for none */
  filters: { states: ChipId[]; analysis: MarkFilter | null };
};

const GROUPINGS: GroupBy[] = ["user", "folder", "job", "flat"];
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
  columns: columnsFor(direction).map(({ id, visible }) => ({ id, visible })),
  sort: { id: "file", dir: "asc" },
  filters: { states: [], analysis: null }
});

/**
 * What was saved as far as it fits: every column of the direction once, in the saved order with
 * new columns after, and anything unknown dropped. A column keeps only a width the user dragged,
 * so one never dragged follows its default as that changes. The text filter is never saved
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
          visible: typeof c.visible === "boolean" ? c.visible : known.visible,
          ...(typeof c.dragged === "number" && c.dragged >= 40 ? { dragged: c.dragged } : {})
        }
      ];
    })
    .filter((c, i, all) => all.findIndex((o) => o.id === c.id) === i);
  for (const c of base.columns) if (!columns.some((o) => o.id === c.id)) columns.push(c);
  const sort = fields(s.sort);
  const { states, analysis } = fields(s.filters);
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
        : [],
      analysis: MARK_FILTERS.find((f) => f.id === analysis)?.id ?? null
    }
  };
};

/** How each transfers screen is laid out in this browser */
export const views = {
  download: stored("transfers.view.download", (saved) => read("download", saved)),
  upload: stored("transfers.view.upload", (saved) => read("upload", saved))
};
