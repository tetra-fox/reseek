import { dateTime } from "#lib/format.js";
import { UNTIL_RESTART } from "#lib/wording.js";

import type { ConfigView } from "./fields";

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

export const editability = (view: ConfigView, key: string): Editability => {
  const field = view.fields[key];
  if (!field)
    return { mode: "locked", notice: "This option is not recognized by this version of reseek" };
  const { source, apply } = field;
  if (source.layer === "env") {
    return { mode: "locked", notice: `Set by ${source.var}. Change it there.` };
  }
  if (source.layer === "cli") {
    return {
      mode: "locked",
      notice: `Set on the command line with ${source.flag}. Change it there.`
    };
  }
  const name = fileOf(view, key);
  const file = view.files.find((f) => f.name === name);
  const writable = !view.managedBy && (file?.writable ?? false);
  if (writable) {
    return {
      mode: "save",
      notice: apply === "restart" ? "Requires a restart to take effect" : null
    };
  }
  const runtime = apply === "live" || apply === "reconnect";
  // a value applied here lasts as long as the choice made when applying it
  const applied =
    source.layer === "runtime"
      ? `The value applied here lasts ${lasting(source)}.`
      : "Changes made here apply immediately and last until restart or until removed, as chosen when applying.";
  const manager = view.managedBy ?? (file?.path.startsWith("/nix/store/") ? "nix" : null);
  if (manager) {
    return runtime
      ? {
          mode: "runtime",
          notice: `Set by ${manager}. ${applied} Use Copy as Nix and add the result to the configuration.`
        }
      : {
          mode: "locked",
          notice: `Set by ${manager}. Requires a restart. Use Copy as Nix and add the result to the configuration.`
        };
  }
  const why = file?.readOnlyReason ?? "read-only";
  return runtime
    ? {
        mode: "runtime",
        notice: `${name} is not writable (${why}). ${applied}`
      }
    : {
        mode: "locked",
        notice: `${name} is not writable (${why}). This option requires a restart.`
      };
};
