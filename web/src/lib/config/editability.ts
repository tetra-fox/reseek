import { dateTime } from "#lib/format.js";
import { UNTIL_RESTART } from "#lib/wording.js";

import type { ConfigView } from "./fields";
import { valueAt } from "./sections";

/** The manager of configuration files found in the Nix store */
const NIX = "Nix";

/**
 * How an option can be changed from here: saved to its file, applied at runtime only, or not at
 * all; and the one line that says why when it isn't simply saved
 */
export type Editability = { mode: "save" | "runtime" | "locked"; notice: string | null };

/** The file a save of `key` writes: the one the daemon names for it, else the first file */
export const fileOf = (view: ConfigView, key: string): string =>
  view.fields[key]?.file ?? view.files[0]?.name ?? "config.toml";

/** How long a runtime override lasts, in words: until restart, until a time, or until removed */
export const lasting = (o: { untilRestart: boolean; expiresAt?: string | null }): string =>
  o.untilRestart ? UNTIL_RESTART : o.expiresAt ? `until ${dateTime(o.expiresAt)}` : "until removed";

/** What manages the file a save of `key` writes, null when nothing does */
export const managerOf = (view: ConfigView, key: string): string | null => {
  const file = view.files.find((f) => f.name === fileOf(view, key));
  return view.managedBy ?? (file?.path.startsWith("/nix/store/") ? NIX : null);
};

/** What manages the configuration files, for the one line the settings page says it in */
export const managerOfFiles = (view: ConfigView): string | null =>
  view.managedBy ?? (view.files.some((f) => f.path.startsWith("/nix/store/")) ? NIX : null);

/**
 * Where a change is made: the settings page, which asks how long a value applied at runtime
 * lasts and offers Copy as Nix, or a page that keeps it until removed and offers neither
 */
export type ChangedFrom = "settings" | "page";

export const editability = (
  view: ConfigView,
  key: string,
  from: ChangedFrom = "settings"
): Editability => {
  const field = view.fields[key];
  if (!field) return { mode: "locked", notice: "Not recognized by this version of reseek" };
  const { source, apply } = field;
  if (source.layer === "env") return { mode: "locked", notice: `Set by ${source.var}` };
  if (source.layer === "cli") return { mode: "locked", notice: `Set by ${source.flag}` };
  // the daemon refuses every change while remote configuration is off, which the page says once
  if (valueAt(view.values, "remote_configuration") !== true)
    return { mode: "locked", notice: null };
  const name = fileOf(view, key);
  const file = view.files.find((f) => f.name === name);
  if (!view.managedBy && file?.writable) return { mode: "save", notice: null };
  const runtime = apply === "live" || apply === "reconnect";
  const manager = managerOf(view, key);
  const reason = file?.readOnlyReason;
  const readOnly = reason ? `${name} is read-only (${reason})` : `${name} is read-only`;
  if (runtime && from === "page") {
    return {
      mode: "runtime",
      notice: manager
        ? `Overrides the ${manager} configuration until removed`
        : `${readOnly}, so a change here overrides it until removed`
    };
  }
  if (!runtime) {
    return {
      mode: "locked",
      notice: manager ? `Change it in the ${manager} configuration` : readOnly
    };
  }
  if (source.layer === "runtime")
    return { mode: "runtime", notice: `Applied here ${lasting(source)}` };
  // the settings page says once that managed values change here only until restart or removal
  return { mode: "runtime", notice: manager ? null : `${readOnly}, so changes here are temporary` };
};
