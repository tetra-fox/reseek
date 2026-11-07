import {
  type ColumnDef,
  columnOrderingFeature,
  columnResizingFeature,
  type columnResizingState,
  type ColumnSizingState,
  columnSizingFeature,
  columnVisibilityFeature,
  createSortedRowModel,
  createTable,
  functionalUpdate,
  type SvelteTable,
  type Header,
  rowSortingFeature,
  type SortingState,
  tableFeatures,
  type Updater
} from "@tanstack/svelte-table";
import { flushSync } from "svelte";

import { cssWidth, sumWidths } from "#lib/format.js";
import { fields, stored } from "#lib/stored.svelte.js";

const features = tableFeatures({
  rowSortingFeature,
  columnSizingFeature,
  columnResizingFeature,
  columnVisibilityFeature,
  columnOrderingFeature,
  sortedRowModel: createSortedRowModel(),
  sortFns: {}
});

/** A column of a grid */
export type GridColumn<T> = {
  id: string;
  label: string;
  /**
   * the width it has until the user drags it, a CSS length that grows with the text: a column of
   * figures holds its widest value, the others are in rem
   */
  width: string;
  /** takes what is left of the row on top of its width */
  grow?: boolean;
  /** what sorting by it orders by; a column without one doesn't sort */
  value?: (row: T) => number | string;
  /** sorts largest first on its first click */
  descFirst?: boolean;
  /** sorts as another column does, whose order its header shows and sets */
  sortsAs?: string;
  /** its figures line up on the right */
  end?: boolean;
  /** its header names it to screen readers only, as a column of checkboxes or of row actions does */
  quiet?: boolean;
  /** its edge can't be dragged */
  fixed?: boolean;
  /** always shown, never offered in the Columns menu */
  always?: boolean;
};

/** A grid's sort: the column and the direction, or none */
export type GridSort = { id: string; dir: "asc" | "desc" } | null;

type Spec<T extends object> = {
  columns: () => GridColumn<T>[];
  data: () => T[];
  /** the sort the screen keeps */
  sort: () => GridSort;
  setSort: (sort: GridSort) => void;
  /** the widths in pixels the user dragged columns to, which the screen keeps */
  dragged: () => Partial<Record<string, number>>;
  drag: (id: string, width: number) => void;
  /** the ids of the columns shown, in order; every column when not given */
  shown?: () => string[];
  /** a header click on a column already sorted its second way clears the sort */
  clearable?: boolean;
  /** the column ties are ordered by, ascending */
  ties?: string;
};

/** A grid's sort and the widths its columns were dragged to, as a screen keeps them */
export type GridView = { sort: GridSort; dragged: Record<string, number> };

/**
 * A grid's view kept in this browser under `key`: the sort, by one of the columns `sortable`
 * names, `sort` until the user picks one, and the widths the user dragged columns to. Anything
 * saved that no longer fits is dropped. Module-level only, as `stored` is
 */
export const gridView = (key: string, sortable: string[], sort: GridSort): GridView =>
  stored(key, (saved) => {
    const s = fields(saved);
    const kept = fields(s.sort);
    return {
      sort:
        typeof kept.id === "string" && sortable.includes(kept.id)
          ? { id: kept.id, dir: kept.dir === "desc" ? "desc" : "asc" }
          : s.sort === null
            ? null
            : sort,
      dragged: Object.fromEntries(
        Object.entries(fields(s.dragged)).filter(
          (e): e is [string, number] => typeof e[1] === "number" && e[1] >= 40
        )
      )
    };
  });

/** The parts of a grid's spec that keep its sort and widths in `view` */
export const keptIn = (view: GridView) => ({
  sort: () => view.sort,
  setSort: (sort: GridSort) => (view.sort = sort),
  dragged: () => view.dragged,
  drag: (id: string, width: number) => (view.dragged[id] = width)
});

/** Strings in the user's language order, numbers in them by value: "2.flac" before "10.flac" */
const collator = new Intl.Collator(undefined, { numeric: true });
const compare = (a: unknown, b: unknown) =>
  typeof a === "string" && typeof b === "string"
    ? collator.compare(a, b)
    : (a as number) - (b as number);

/**
 * A grid of rows and columns on TanStack Table: it sorts the rows, shows and orders the columns
 * and sizes them, with the sort and the dragged widths kept by the screen. A column keeps its
 * default CSS width until the user drags its edge; the drag starts from the width it has on
 * screen and is saved in whole pixels, never below 40.
 *
 * While a column's values change in place (a transfer's speed), the rows are sorted again, as the
 * sort's values are read whenever the rows are
 */
export class Grid<T extends object> {
  readonly table: SvelteTable<typeof features, T>;
  #spec: Spec<T>;
  /** the width a drag started from, the table's until the drag ends */
  #start = $state<{ id: string; width: number } | null>(null);
  #moved = false;
  #resizing = $state<columnResizingState>({
    startOffset: null,
    startSize: null,
    deltaOffset: null,
    deltaPercentage: null,
    isResizingColumn: false,
    columnSizingStart: []
  });

  constructor(spec: Spec<T>) {
    this.#spec = spec;
    // the table reads its options through getters, which these give the current values
    const defs = () => this.#defs;
    const rows = () => this.#rows;
    const sorting = () => this.#sorting;
    const sizing = () => this.#sizing;
    const resizing = () => this.#resizing;
    const visibility = () => this.#visibility;
    const order = () => this.#order;
    this.table = createTable({
      features,
      get columns() {
        return defs();
      },
      get data() {
        return rows();
      },
      enableMultiSort: false,
      sortDescFirst: false,
      get enableSortingRemoval() {
        return !!spec.clearable;
      },
      columnResizeMode: "onChange",
      state: {
        get sorting() {
          return sorting();
        },
        get columnSizing() {
          return sizing();
        },
        get columnResizing() {
          return resizing();
        },
        get columnVisibility() {
          return visibility();
        },
        get columnOrder() {
          return order();
        }
      },
      onSortingChange: (updater: Updater<SortingState>) => {
        const first = functionalUpdate(updater, this.#sorting)[0];
        spec.setSort(first ? { id: first.id, dir: first.desc ? "desc" : "asc" } : null);
      },
      onColumnSizingChange: (updater: Updater<ColumnSizingState>) => {
        // a press on an edge that never moved changes nothing
        if (!this.#moved) return;
        const before = this.#sizing;
        const after = functionalUpdate(updater, before);
        for (const [id, width] of Object.entries(after)) {
          if (width !== before[id]) spec.drag(id, Math.max(40, Math.round(width)));
        }
      },
      onColumnResizingChange: (updater: Updater<columnResizingState>) => {
        this.#resizing = functionalUpdate(updater, this.#resizing);
        if (this.#resizing.deltaOffset) this.#moved = true;
        if (!this.#resizing.isResizingColumn) this.#start = null;
      }
    });
  }

  /** the rows in the grid's order */
  readonly rows: T[] = $derived.by(() => this.table.getRowModel().rows.map((row) => row.original));

  /** the columns shown, in order */
  readonly columns: GridColumn<T>[] = $derived.by(() => {
    const byId = new Map(this.#spec.columns().map((c) => [c.id, c]));
    return this.table.getVisibleLeafColumns().map((c) => byId.get(c.id)!);
  });

  /** the CSS grid columns of a row */
  readonly template: string = $derived.by(() =>
    this.columns
      .map((c) =>
        c.grow ? `minmax(${cssWidth(this.widthOf(c))}, 1fr)` : cssWidth(this.widthOf(c))
      )
      .join(" ")
  );

  /** the narrowest a row gets, its columns' widths together */
  readonly width: string = $derived.by(() => sumWidths(this.columns.map((c) => this.widthOf(c))));

  /** A column's width: the pixels it was dragged to, else its default */
  widthOf(column: GridColumn<T>): number | string {
    return (!column.fixed && this.table.atoms.columnSizing.get()[column.id]) || column.width;
  }

  /** How the grid is sorted by a column now */
  sortOf(column: GridColumn<T>): "asc" | "desc" | null {
    const sort = this.#spec.sort();
    return sort?.id === (column.sortsAs ?? column.id) ? sort.dir : null;
  }

  /** Whether a click on the column's header sorts */
  sorts(column: GridColumn<T>): boolean {
    return !!(column.value || column.sortsAs);
  }

  /** A header click: sorts by the column, or turns its order around */
  toggle(column: GridColumn<T>) {
    this.table.getColumn(column.sortsAs ?? column.id)?.toggleSorting();
  }

  /** The header of a column, for its edge to be dragged */
  header(id: string): Header<typeof features, T, unknown> | undefined {
    return this.table.getFlatHeaders().find((h) => h.column.id === id);
  }

  /**
   * Starts dragging a column's edge, from a press on it. The table takes the width the column
   * has on screen as the drag's start, whatever it was set as
   */
  resize(id: string, e: MouseEvent | TouchEvent) {
    const header = this.header(id);
    const cell = (e.currentTarget as Element).closest("[role=columnheader]");
    if (!header || !cell) return;
    this.#moved = false;
    this.#start = { id, width: Math.round(cell.getBoundingClientRect().width) };
    // the table reads its options before the drag starts, so they must hold the start now
    flushSync();
    header.getResizeHandler()(e);
  }

  // the table's options, each kept as one value until what it is made from changes, so the
  // table recomputes only what a change touches

  #defs = $derived.by(() =>
    this.#spec.columns().map((c): ColumnDef<typeof features, T> => ({
      id: c.id,
      accessorFn: c.value ?? (() => 0),
      enableSorting: !!c.value,
      sortDescFirst: !!c.descFirst,
      sortFn: (a, b, id) => compare(a.getValue(id), b.getValue(id)),
      enableResizing: !c.fixed,
      minSize: 40
    }))
  );

  #sorting: SortingState = $derived.by(() => {
    const sort = this.#spec.sort();
    if (!sort) return [];
    const ties = this.#spec.ties;
    return [
      { id: sort.id, desc: sort.dir === "desc" },
      ...(ties && ties !== sort.id ? [{ id: ties, desc: false }] : [])
    ];
  });

  #sizing: ColumnSizingState = $derived.by(() => {
    const start = this.#start;
    const dragged = Object.fromEntries(
      Object.entries(this.#spec.dragged()).filter((e): e is [string, number] => e[1] !== undefined)
    );
    return start ? { ...dragged, [start.id]: start.width } : dragged;
  });

  #order: string[] = $derived.by(
    () => this.#spec.shown?.() ?? this.#spec.columns().map((c) => c.id)
  );

  #visibility: Record<string, boolean> = $derived.by(() =>
    Object.fromEntries(this.#spec.columns().map((c) => [c.id, this.#order.includes(c.id)]))
  );

  /** the rows, with the sorted columns' values read so a change to one sorts them again */
  #rows: T[] = $derived.by(() => {
    const rows = this.#spec.data();
    const columns = this.#spec.columns();
    for (const sort of this.#sorting) {
      const value = columns.find((c) => c.id === sort.id)?.value;
      if (value) for (const row of rows) value(row);
    }
    return rows.slice();
  });
}
