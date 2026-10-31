import type { Schemas } from "#lib/api/client.js";

import type { Control, Field } from "./sections";
import { columnsOf, tableDraft, tableValue } from "./tables";

export type ConfigView = Schemas["ConfigView"];
export type FieldView = Schemas["FieldView"];

/** How a draft value is held while edited: a switch's boolean, else the text in the input */
export type Draft = string | boolean;

/** What saving a draft asks for: a value to write, the key taken out, or why it can't */
export type Parsed = { set: unknown } | { unset: true } | { error: string };

/** A secret the daemon holds, as it shows it */
export const REDACTED = "*****";

/**
 * The control for an option: the description's, else a select of the names the daemon gives for
 * it, else what the daemon's kind for it calls for
 */
export const controlOf = (field: Field, view: FieldView | undefined): Control => {
  if (field.control) return field.control;
  if (view?.secret) return "secret";
  if (view?.choices) return "select";
  const kind = view?.kind;
  if (kind === undefined) return "text";
  // a list of tables or maps can't be a line each, so it is edited as a table
  if (typeof kind === "object")
    return typeof kind.list === "object" || kind.list === "map" || kind.list === "table"
      ? "table-list"
      : "list";
  if (kind === "bool") return "bool";
  if (kind === "integer" || kind === "float") return "number";
  if (kind === "duration" || kind === "size") return kind;
  if (kind === "map" || kind === "table") return "map";
  return "text";
};

const isList = (control: Control) =>
  control === "list" || control === "regex-list" || control === "cidr-list";

/** The running value as the control edits it */
export const toDraft = (control: Control, value: unknown): Draft => {
  if (control === "bool") return value === true;
  if (isList(control)) return Array.isArray(value) ? value.map(String).join("\n") : "";
  if (control === "map" || control === "table-list") return tableDraft(control, value);
  return value === null || value === undefined ? "" : String(value);
};

/** One element per line, blank lines dropped */
const lines = (text: string) =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

/** A CIDR network or a single address, as far as a quick look can tell */
const CIDR = /^(?:[0-9]{1,3}(?:\.[0-9]{1,3}){3}|[0-9a-fA-F:]*:[0-9a-fA-F:.]*)(?:\/[0-9]{1,3})?$/;

/**
 * What saving a draft writes. An emptied input takes the key out, so the next layer's value or the
 * default applies; the daemon checks everything again, this only catches what a person can fix
 * before sending
 */
export const fromDraft = (
  field: Field,
  control: Control,
  draft: Draft,
  view?: FieldView
): Parsed => {
  if (control === "bool") return { set: draft === true };
  const text = String(draft);
  if (isList(control)) {
    const items = lines(text);
    if (control === "regex-list") {
      for (const item of items) {
        try {
          new RegExp(item);
        } catch {
          return { error: `${item} is not a regular expression` };
        }
      }
    }
    if (control === "cidr-list") {
      const bad = items.find((item) => !CIDR.test(item));
      if (bad) return { error: `${bad} is not a network such as 192.168.1.0/24` };
    }
    return { set: items };
  }
  if (control === "secret") return { set: text };
  if (control === "map" || control === "table-list") {
    return tableValue(control, text, columnsOf(field, view?.entries ?? []));
  }
  if (text.trim() === "") return { unset: true };
  if (control === "auto-count" && text.trim().toLowerCase() === "auto") return { set: "auto" };
  if (control === "number" || control === "auto-count") {
    const n = Number(text);
    if (!Number.isFinite(n))
      return { error: control === "auto-count" ? "must be auto or a number" : "must be a number" };
    if ((view?.kind === "integer" || control === "auto-count") && !Number.isInteger(n))
      return { error: "must be a whole number" };
    if (field.min !== undefined && n < field.min) return { error: `must be at least ${field.min}` };
    if (field.max !== undefined && n > field.max) return { error: `must be at most ${field.max}` };
    return { set: n };
  }
  const choices = view?.choices;
  if (control === "select" && choices && !choices.includes(text)) {
    return { error: `must be one of ${choices.join(", ")}` };
  }
  return { set: text };
};

/**
 * The daemon's messages for each key, from a 400's detail of `<key>: <message>; ...`, several for
 * one key joined as they came
 */
export const errorsByKey = (detail: string | null | undefined, keys: string[]) => {
  const found = new Map<string, string>();
  for (const part of (detail ?? "").split("; ")) {
    const key = keys.find((k) => part.startsWith(`${k}: `));
    if (!key) continue;
    const message = part.slice(key.length + 2);
    found.set(key, found.has(key) ? `${found.get(key)}; ${message}` : message);
  }
  return found;
};
