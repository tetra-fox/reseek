import { api } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";

import { clearing, type Editability, editability, fileOf } from "./editability";
import type { ConfigView } from "./fields";

/** Writes `set` and `unset` into the configuration file `file`, at the revision the view read */
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
        set,
        unset
      }
    })
  );

/**
 * The message for a write refused because `file` changed since the view read it, null for any
 * other failure
 */
export const changedOnDisk = (outcome: Outcome<unknown>, file: string): string | null =>
  !outcome.ok &&
  outcome.status === 409 &&
  outcome.error.startsWith("The configuration file changed on disk")
    ? `${file} changed on disk. Check the values and save again.`
    : null;

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
 * or as runtime overrides that last until removed. A file that changed on disk is named in the
 * error, and the caller reads the view again before another try
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
    const outcome = await call(() =>
      api.PATCH("/api/v0/options", { body: { expiresIn: null, set, unset } })
    );
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
    if (!outcome.ok) {
      const moved = changedOnDisk(outcome, file);
      return moved ? { ...outcome, error: moved } : outcome;
    }
  }
  return { ok: true, data: null };
};
