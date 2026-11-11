import { api } from "#lib/api/client.js";
import { listText } from "#lib/format.js";
import { same } from "#lib/lists/keyed.svelte.js";
import { call, type Outcome } from "#lib/request.js";

import { clearing, type Editability, editability, fileOf } from "./editability";
import type { ConfigView } from "./fields";
import { BLOCKING, sections, valueAt } from "./sections";

/**
 * Writes `set` and `unset` into the configuration file `file`, at the file's revision the view
 * read and at `revision` of the options: the view's, or the one the write before this one left
 */
export const patchFile = (
  view: ConfigView,
  file: string,
  set: Record<string, unknown>,
  unset: string[],
  revision: string = view.revision
) =>
  call(() =>
    api.PATCH("/api/v0/options/file", {
      body: {
        file,
        revision: view.files.find((f) => f.name === file)?.revision ?? null,
        optionsRevision: revision,
        set,
        unset
      }
    })
  );

/**
 * Sets and removes runtime overrides lasting `expiresIn`, made against `revision` of the options,
 * none when the options were never read
 */
export const patchOverrides = (
  revision: string | null,
  set: Record<string, unknown>,
  unset: string[],
  expiresIn: string | null
) =>
  call(() =>
    api.PATCH("/api/v0/options", {
      body: { expiresIn, set, unset, revision }
    })
  );

/** Removes the runtime override of `key`, made against the revision of the options the view read */
export const removeOverride = (view: ConfigView, key: string) =>
  call(() =>
    api.DELETE("/api/v0/options/overrides/{key}", {
      params: { path: { key }, query: { revision: view.revision } }
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
const nameOf = (key: string) =>
  [...sections, BLOCKING].flatMap((s) => s.fields).find((f) => f.key === key)?.label ??
  PAGES.find((page) => key.startsWith(page.under))?.name ??
  key;

/** The options of the view `after` whose values differ from those in `before` */
export const changedBetween = (before: unknown, after: ConfigView): string[] =>
  Object.keys(after.fields).filter(
    (key) => !same(valueAt(before, key), valueAt(after.values, key))
  );

/** What a write `changedUnder` refused says: the options that changed, when known, and that the edits wait */
export const changedText = (keys: string[]) => {
  const names = [...new Set(keys.map(nameOf))];
  return names.length
    ? `${listText(names)} changed meanwhile. Check your changes and save again.`
    : "The settings changed meanwhile. Check your changes and save again.";
};

/** The options view as a page holds it, read again on demand */
export type Views = { data: ConfigView | undefined; load: () => Promise<void> };

/**
 * The failure of a write `changedUnder` refused, once `views` holds the options as they are now,
 * saying what changed since `before`
 */
export const refusedUnder = async (
  views: Views,
  before: ConfigView,
  failure: Refused
): Promise<Refused> => {
  await views.load();
  return {
    ...failure,
    error: changedText(views.data ? changedBetween(before.values, views.data) : [])
  };
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
 * Writes a change made on a page outside Settings, from the options `views` holds, as
 * `editabilityOf` says: to each key's file, or as runtime overrides that last until removed.
 * Files are written one after another in the order of their first key in `set`, then `unset`,
 * so what must already hold should a later write fail goes first. Ends once `views` holds the
 * options the write left, so the page's next change is made from them; one refused because the
 * options changed since says what changed
 */
export const writeFromPage = async (
  views: Views,
  set: Record<string, unknown>,
  unset: string[] = []
): Promise<Outcome<null>> => {
  const view = views.data;
  if (!view) return { ok: false, error: "Settings are still loading", status: 0 };
  const outcome = await written(view, set, unset);
  if (!outcome.ok && changedUnder(outcome)) return refusedUnder(views, view, outcome);
  if (outcome.ok) await views.load();
  return outcome;
};

/** `set` and `unset` written from `view` as `writeFromPage` says, each write made from the last */
const written = async (
  view: ConfigView,
  set: Record<string, unknown>,
  unset: string[]
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
    const outcome = await patchOverrides(view.revision, set, unset, null);
    return outcome.ok ? { ok: true, data: null } : outcome;
  }
  const byFile = new Map<string, string[]>();
  for (const key of keys) {
    const file = fileOf(view, key);
    byFile.set(file, [...(byFile.get(file) ?? []), key]);
  }
  let revision = view.revision;
  for (const [file, inFile] of byFile) {
    const outcome = await patchFile(
      view,
      file,
      Object.fromEntries(inFile.filter((k) => k in set).map((k) => [k, set[k]])),
      inFile.filter((k) => unset.includes(k)),
      revision
    );
    if (!outcome.ok) return outcome;
    revision = outcome.data.optionsRevision;
  }
  return { ok: true, data: null };
};
