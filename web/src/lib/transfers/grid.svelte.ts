import { Grid } from "#lib/table.svelte.js";
import { viewport } from "#lib/viewport.svelte.js";

import { type ColumnId, columnsFor, PHONE_COLUMNS } from "./columns";
import type { ViewSettings } from "./settings.svelte";
import type { Direction, Transfer } from "./states";
import type { SortKey } from "./view";

/**
 * A transfers screen's grid: `transfers` in the screen's sort, ties by file name, with the
 * columns it shows and the widths dragged, all kept in its view settings. The user column is
 * hidden while each user is a group, where its order has no effect, and a phone shows the file
 * and the menu alone, at widths of its own
 */
export const transferGrid = (
  direction: Direction,
  view: ViewSettings,
  transfers: () => Transfer[]
): Grid<Transfer> =>
  new Grid<Transfer>({
    columns: () =>
      columnsFor(direction).map((c) =>
        viewport.phone && PHONE_COLUMNS[c.id]
          ? { ...c, width: PHONE_COLUMNS[c.id]!, fixed: true }
          : c
      ),
    data: transfers,
    sort: () => view.sort,
    setSort: (sort) => {
      const column = columnsFor(direction).find((c) => c.id === sort?.id);
      if (sort && column?.sort) view.sort = { id: column.sort, dir: sort.dir };
    },
    dragged: () =>
      Object.fromEntries(view.columns.flatMap((c) => (c.dragged ? [[c.id, c.dragged]] : []))),
    drag: (id, width) => {
      const column = view.columns.find((c) => c.id === id);
      if (column) column.dragged = width;
    },
    shown: () =>
      view.columns
        .filter((c) => c.visible && !(c.id === "user" && view.groupBy === "user"))
        .filter((c) => !viewport.phone || c.id in PHONE_COLUMNS)
        .map((c) => c.id),
    ties: "file" satisfies ColumnId & SortKey
  });
