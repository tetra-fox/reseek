import { api, type Schemas } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";

/** A result filter saved under a name */
export type Preset = { name: string; filter: string };

/** The option the presets are kept in, one list replaced whole */
export const PRESETS_KEY = "search.filter_presets";

/** The presets in the options in effect, those that can't be read left out */
export const presetsOf = (options: Schemas["OptionsDocument"] | null): Preset[] => {
  const list = fields(fields(options).search).filter_presets;
  if (!Array.isArray(list)) return [];
  return list.flatMap((entry) => {
    const { name, filter } = fields(entry);
    return typeof name === "string" && typeof filter === "string" && name.trim()
      ? [{ name, filter }]
      : [];
  });
};

/** Why presets can't be saved from here, or null when they can */
export const whyNotSaved = (
  options: Schemas["OptionsDocument"] | null,
  administrator: boolean
): string | null => {
  if (!administrator) return "Only administrators can save presets";
  if (fields(options).remote_configuration !== true) {
    return "Presets cannot be saved while remote configuration is off";
  }
  return null;
};

/** The presets with `preset` saved: in place of one of the same name, else after the others */
export const withPreset = (presets: Preset[], preset: Preset): Preset[] => {
  const at = presets.findIndex((p) => p.name === preset.name);
  return at === -1 ? [...presets, preset] : presets.with(at, preset);
};

/** Keeps a new list of presets in the daemon; the options frame brings it back to every tab */
export const savePresets = (presets: Preset[]): Promise<Outcome<unknown>> =>
  call(() => api.PATCH("/api/v0/options", { body: { set: { [PRESETS_KEY]: presets } } }));
