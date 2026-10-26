import type { ConfigView } from "./fields";

/**
 * How an option can be changed from here: saved to its file, applied at runtime only, or not at
 * all; and the one line that says why when it isn't simply saved
 */
export type Editability = { mode: "save" | "runtime" | "locked"; notice: string | null };

export const editability = (view: ConfigView, key: string): Editability => {
  const field = view.fields[key];
  if (!field) return { mode: "locked", notice: "Not an option this reseek knows" };
  const { source, apply } = field;
  if (source.layer === "env") {
    return { mode: "locked", notice: `Set by ${source.var}; change it there` };
  }
  if (source.layer === "cli") {
    return { mode: "locked", notice: `Set on the command line (${source.flag}); change it there` };
  }
  const name = field.file ?? view.files[0]?.name ?? "config.toml";
  const file = view.files.find((f) => f.name === name);
  const writable = !view.managedBy && (file?.writable ?? false);
  if (writable) {
    return {
      mode: "save",
      notice: apply === "restart" ? "Needs a restart to take effect" : null
    };
  }
  const runtime = apply === "live" || apply === "reconnect";
  const manager = view.managedBy ?? (file?.path.startsWith("/nix/store/") ? "nix" : null);
  if (manager) {
    return runtime
      ? {
          mode: "runtime",
          notice: `Set by ${manager}. Changes here apply now but won't persist: click Copy as Nix and add it to your configuration.`
        }
      : {
          mode: "locked",
          notice: `Set by ${manager} and needs a restart: add it to your configuration (Copy as Nix)`
        };
  }
  const why = file?.readOnlyReason ?? "read-only";
  return runtime
    ? {
        mode: "runtime",
        notice: `${name} isn't writable (${why}). Changes apply now but won't persist.`
      }
    : { mode: "locked", notice: `${name} isn't writable (${why}) and this needs a restart` };
};
