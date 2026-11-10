import { api } from "#lib/api/client.js";
import { listText } from "#lib/format.js";
import { same } from "#lib/lists/keyed.svelte.js";
import { call, type Outcome } from "#lib/request.js";

import { clearing, type Editability, editability, fileOf } from "./editability";
import type { ConfigView } from "./fields";
import { BLOCKING, sections, valueAt } from "./sections";

/**
 * Writes `set` and `unset` into the configuration file `file`, at the revisions of the file and
 * of the options the view read
 */
export const patchFile = (
  view: ConfigView,
  file: string,
  set: Record<string, unknown>,
  unset: string[]
) =>
  call(() =>
    api.PATCH("/api/v0/options/file", {
      body: {
        file,
        revision: view.files.find((f) => f.name === file)?.revision ?? null,
        optionsRevision: view.revision,
        set,
        unset
      }
    })
  );

/** Sets and removes runtime overrides lasting `expiresIn`, at the revision of the options the view read */
export const patchOverrides = (
  view: ConfigView,
  set: Record<string, unknown>,
  unset: string[],
  expiresIn: string | null
) =>
  call(() =>
    api.PATCH("/api/v0/options", {
      body: { expiresIn, set, unset, revision: view.revision }
    })
  );

/** A write that failed */
export type Refused = Extract<Outcome<unknown>, { ok: false }>;

/**
 * Whether a write failed because what it was made against changed since the view was read: the
 * options, or the file it writes, as another admin's save or an edit on disk changes them
 */
export const changedUnder = (failure: Refused): boolean =>
  failure.status === 409 &&
  (failure.error.startsWith("The options changed since they were read") ||
    failure.error.startsWith("The configuration file changed on disk"));

/** The pages that edit the options no settings section shows, by the start of their keys */
const PAGES = [{ under: "transfers.groups.", name: "Groups" }];

/** What `key` is called where it is edited: its settings label, its page, or the key */
const labelOf = (key: string) =>
  [...sections, BLOCKING].flatMap((s) => s.fields).find((f) => f.key === key)?.label ??
  PAGES.find((page) => key.startsWith(page.under))?.name ??
  key;

/** The options whose values differ between the views `before` and `after`, by their names */
export const changedBetween = (before: ConfigView, after: ConfigView): string[] => [
  ...new Set(
    Object.keys(after.fields)
      .filter((key) => !same(valueAt(before.values, key), valueAt(after.values, key)))
      .map(labelOf)
  )
];

/** What a write `changedUnder` refused says: what changed, when known, and that the edits wait */
export const changedText = (changed: string[]) =>
  changed.length
    ? `${listText(changed)} changed meanwhile. Check your changes and save again.`
    : "The settings changed meanwhile. Check your changes and save again.";

/**
 * The failure of a write `changedUnder` refused, saying what changed since `view` as the daemon
 * now has it; the caller reads the view again before another try
 */
export const refusedUnder = async (view: ConfigView, outcome: Refused): Promise<Refused> => {
  const now = await call(() => api.GET("/api/v0/options/view"));
  return { ...outcome, error: changedText(now.ok ? changedBetween(view, now.data) : []) };
};

/**
 * How a page outside Settings changes `keys` together: locked when any of them is, applied until
 * removed when any of them can't be saved to its file, else saved
 */
export const editabilityOf = (view: ConfigView, keys: string[]): Editability => {
  const each = keys.map((key) => editability(view, key, "page"));
  return (
    each.find((e) => e.mode === "locked") ??
    each.find((e) => e.mode === "runtime") ?? { mode: "save", notice: null }
  );
};

/**
 * Writes a change made on a page outside Settings as `editabilityOf` says: to each key's file,
 * or as runtime overrides that last until removed. One refused because the options changed
 * since the view was read says what changed, and the caller reads the view again before another
 * try
 */
export const writeFromPage = async (
  view: ConfigView,
  set: Record<string, unknown>,
  unset: string[] = []
): Promise<Outcome<null>> => {
  const keys = [...Object.keys(set), ...unset];
  const edit = editabilityOf(view, keys);
  if (edit.mode === "locked")
    return { ok: false, error: edit.notice ?? "Remote configuration is off", status: 0 };
  if (edit.mode === "runtime") {
    // an unset here would drop no more than a runtime override, leaving the file's value
    for (const key of unset) {
      const held = clearing(view, key, "runtime");
      if (held) return { ok: false, error: `${key}: ${held}`, status: 0 };
    }
    const outcome = await patchOverrides(view, set, unset, null);
    if (!outcome.ok && changedUnder(outcome)) return refusedUnder(view, outcome);
    return outcome.ok ? { ok: true, data: null } : outcome;
  }
  const byFile = new Map<string, string[]>();
  for (const key of keys) {
    const file = fileOf(view, key);
    byFile.set(file, [...(byFile.get(file) ?? []), key]);
  }
  for (const [file, inFile] of byFile) {
    const outcome = await patchFile(
      view,
      file,
      Object.fromEntries(inFile.filter((k) => k in set).map((k) => [k, set[k]])),
      inFile.filter((k) => unset.includes(k))
    );
    if (!outcome.ok && changedUnder(outcome)) return refusedUnder(view, outcome);
    if (!outcome.ok) return outcome;
  }
  return { ok: true, data: null };
};
