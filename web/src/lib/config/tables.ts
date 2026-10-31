import type { Schemas } from "#lib/api/client.js";

import type { Control, Field } from "./sections";

/** What a table's cell edits, from the kind the daemon reports for the field */
export type CellKind = "text" | "secret" | "number" | "bool" | "select" | "list";

/** A field of a table's rows, as a column or a labeled input */
export type Column = {
  /** the dotted key inside the row; "" for a row that is one value */
  key: string;
  label: string;
  /** what the field is, in the daemon's words */
  doc: string;
  kind: CellKind;
  /** the names a select offers */
  choices: string[];
  /** what a row holds while the field is not set, null for nothing */
  default: unknown;
};

/** A value's fields as they are held while edited */
type Fields = Record<string, unknown>;

/** A row being edited. A list's rows have no name */
export type Row = { id: number; name: string; value: Fields };

/** What saving a table writes, or why it can't */
type Parsed = { set: unknown } | { error: string };

/** What a secret the daemon holds is shown as */
const REDACTED = "*****";

const kindOf = (entry: Schemas["EntryView"]): CellKind => {
  if (entry.secret) return "secret";
  if (entry.choices) return "select";
  if (entry.kind === "bool") return "bool";
  if (entry.kind === "integer" || entry.kind === "float") return "number";
  return typeof entry.kind === "object" ? "list" : "text";
};

/** The columns of a table: the fields the daemon reports for its rows, with the form's labels */
export const columnsOf = (field: Field, entries: Schemas["EntryView"][]): Column[] => {
  if (!entries.length)
    return [{ key: "", label: "Value", doc: "", kind: "text", choices: [], default: null }];
  return entries.map((entry) => ({
    key: entry.key,
    label: field.columns?.[entry.key] ?? entry.key,
    doc: entry.doc,
    kind: kindOf(entry),
    choices: entry.choices ?? [],
    default: entry.default
  }));
};

/** A column's default as a cell shows it while empty */
export const defaultText = (column: Column): string => {
  const held = column.default;
  if (Array.isArray(held)) return held.join(", ");
  return held === null || held === undefined ? "" : String(held);
};

/** `value` with its keys in order and no nulls, so two drafts that say the same read the same */
const settled = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(settled);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value)
      .filter(([, inner]) => inner !== null && inner !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : 1))
      .map(([key, inner]) => [key, settled(inner)])
  );
};

/** The draft of a table: a list's elements, or a map's entries as name and value pairs */
export const tableDraft = (control: Control, value: unknown): string => {
  if (control === "table-list") return JSON.stringify(settled(value ?? []));
  const entries = Object.entries(value ?? {});
  return JSON.stringify(entries.map(([name, inner]) => [name, settled(inner)]));
};

/** The rows a draft holds, numbered from `first` */
export const rowsOf = (control: Control, draft: string, first = 0): Row[] => {
  const held = JSON.parse(draft) as unknown[];
  return held.map((held, i) => {
    const [name, value] = control === "map" ? (held as [string, Fields]) : ["", held as Fields];
    return { id: first + i, name, value };
  });
};

/** The draft that holds `rows` */
export const draftOf = (control: Control, rows: Row[]): string =>
  JSON.stringify(
    rows.map((row) => (control === "map" ? [row.name, settled(row.value)] : settled(row.value)))
  );

/** The value of a row's field, the whole value for a row that is one value */
export const cellOf = (row: Row, key: string): unknown =>
  key === ""
    ? row.value
    : key.split(".").reduce<unknown>((at, part) => (at as Fields | undefined)?.[part], row.value);

/** `fields` without a table that is left empty at `parts`, or with `cell` there */
const put = (fields: Fields, [part, ...rest]: string[], cell: unknown): Fields => {
  const inner = rest.length ? put((fields[part] ?? {}) as Fields, rest, cell) : cell;
  const emptied =
    inner === undefined || (rest.length > 0 && Object.keys(inner as Fields).length === 0);
  const others = Object.fromEntries(Object.entries(fields).filter(([key]) => key !== part));
  return emptied ? others : { ...others, [part]: inner };
};

/**
 * `row` with its field `key` holding `cell`. Nothing left in a cell takes the field out, so the
 * daemon's default for it applies
 */
export const withCell = (row: Row, key: string, cell: unknown): Row => {
  const emptied = cell === "" || (Array.isArray(cell) && !cell.length);
  const held = emptied ? undefined : cell;
  if (key === "") return { ...row, value: (held ?? "") as Fields };
  return { ...row, value: put(row.value, key.split("."), held) };
};

/** Whether a cell shows a secret the daemon holds and does not send */
export const isHidden = (cell: unknown) => cell === REDACTED;

/** A number typed into a cell, kept as it was typed when it is none */
export const typedNumber = (text: string): number | string => {
  const n = Number(text);
  return text.trim() === "" || !Number.isFinite(n) ? text : n;
};

/** What is wrong with each row that a person can fix before saving, by its id */
export const rowProblems = (control: Control, rows: Row[], columns: Column[]) => {
  const problems = new Map<number, string>();
  const named = new Map<string, number>();
  for (const { name } of rows) named.set(name.trim(), (named.get(name.trim()) ?? 0) + 1);
  for (const row of rows) {
    const number = columns.find(
      (c) => c.kind === "number" && typeof cellOf(row, c.key) === "string"
    );
    if (control === "map" && !row.name.trim()) problems.set(row.id, "Give this row a name");
    else if (control === "map" && named.get(row.name.trim())! > 1)
      problems.set(row.id, "Another row has this name");
    else if (control === "table-list" && !Object.keys(row.value).length)
      problems.set(row.id, "This row is empty");
    else if (number) problems.set(row.id, `${number.label} must be a number`);
  }
  return problems;
};

/** What saving a table's draft writes, or the first thing wrong with one of its rows */
export const tableValue = (control: Control, draft: string, columns: Column[]): Parsed => {
  const rows = rowsOf(control, draft);
  const [problem] = rowProblems(control, rows, columns).values();
  if (problem) return { error: problem };
  if (control === "table-list") return { set: rows.map((row) => row.value) };
  return { set: Object.fromEntries(rows.map((row) => [row.name.trim(), row.value])) };
};

/**
 * The daemon's messages about a table, by the row each names, which it does by the field's path
 * under `key`; what names no row is in `rest`
 */
export const rowErrors = (
  key: string,
  control: Control,
  rows: Row[],
  error: string | undefined
) => {
  const byRow = new Map<number, string>();
  const rest: string[] = [];
  for (const message of error?.split("; ").filter(Boolean) ?? []) {
    const row = rows.find((row, i) => {
      if (control !== "map") return message.includes(`${key}[${i}]`);
      const path = `${key}.${row.name.trim()}`;
      return message.includes(`${path}.`) || message.includes(`${path} `);
    });
    if (row) byRow.set(row.id, [byRow.get(row.id), message].filter(Boolean).join(" "));
    else rest.push(message);
  }
  return { byRow, rest };
};
