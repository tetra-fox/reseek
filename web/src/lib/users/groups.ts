import type { Schemas } from "#lib/api/client.js";
import { valueAt as at } from "#lib/config/sections.js";
import { listText, plural } from "#lib/format.js";
import { same } from "#lib/lists/keyed.svelte.js";

export type GroupSummary = Schemas["GroupSummary"];
export type UserGroup = Schemas["UserGroup"];

/** The option that holds the user-defined groups by name */
export const USER_DEFINED = "transfers.groups.user_defined";
/** The option that holds, for each share not everyone sees, the groups that see it */
export const VISIBLE_TO = "shares.visible_to";
/** The option that holds the groups in the order they go, all but the privileged */
export const ORDER = "transfers.groups.order";
/** The group of our buddies, whose members only the buddy list changes */
export const BUDDIES = "buddies";
/** The group of the users who share too little, which has share thresholds */
export const LEECHERS = "leechers";
/** The group everyone is in, below every other, which holds what no group above sets */
export const DEFAULT = "default";

/** The names a user-defined group cannot take */
const BUILTIN = ["privileged", LEECHERS, BUDDIES, DEFAULT, "blacklisted"];

/** Why `name` can't name a user-defined group, null when it can; `taken` are the names in use */
export const nameProblem = (name: string, taken: string[]): string | null => {
  if (!name) return "Give the group a name";
  if (BUILTIN.includes(name)) return `${name} is a built-in group`;
  if (!/^[a-z0-9_-]{1,32}$/.test(name)) return "Use up to 32 of a-z, 0-9, _ and -";
  if (taken.includes(name)) return `There is already a group named ${name}`;
  return null;
};

/** The groups we put a user in: their manual groups but the buddies, which the buddy list keeps */
export const chosenGroups = (groups: UserGroup[]): string[] =>
  groups.filter((g) => g.kind === "manual" && g.name !== BUDDIES).map((g) => g.name);

/** `names` with `name` in it, once */
export const withGroup = (names: string[], name: string) =>
  names.includes(name) ? names : [...names, name];

/** `names` without `name` */
export const withoutGroup = (names: string[], name: string) => names.filter((n) => n !== name);

/** Each share alias with the groups that see it; a share not listed is seen by everyone */
export type VisibleTo = Record<string, string[]>;

const sameAlias = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** The groups that see the share `alias`, matched ignoring case; null when everyone does */
export const audienceOf = (map: VisibleTo, alias: string): string[] | null =>
  Object.entries(map).find(([listed]) => sameAlias(listed, alias))?.[1] ?? null;

/** `map` with the share `alias` seen by `groups`, or by everyone for null */
export const withAudience = (map: VisibleTo, alias: string, groups: string[] | null): VisibleTo => {
  const others = Object.entries(map).filter(([listed]) => !sameAlias(listed, alias));
  return Object.fromEntries(groups === null ? others : [...others, [alias, groups]]);
};

/**
 * `map` with `group` seeing exactly the shares `aliases` of those not everyone sees. A share
 * everyone saw becomes one only `group` sees; a share left with no group stays listed, so nobody
 * sees it rather than everyone
 */
export const withShares = (map: VisibleTo, group: string, aliases: string[]): VisibleTo => {
  const next: VisibleTo = Object.fromEntries(
    Object.entries(map).map(([listed, groups]) => {
      const rest = groups.filter((g) => g !== group);
      return [listed, aliases.some((a) => sameAlias(a, listed)) ? [...rest, group] : rest];
    })
  );
  for (const alias of aliases) if (audienceOf(next, alias) === null) next[alias] = [group];
  return next;
};

/**
 * `map` with the group `from` called `to` in every share's list, or taken out of them for null. A
 * share left with no group stays listed, so nobody sees it rather than everyone
 */
const renamedIn = (map: VisibleTo, from: string, to: string | null): VisibleTo =>
  Object.fromEntries(
    Object.entries(map).map(([alias, groups]) => [
      alias,
      [...new Set(groups.flatMap((g) => (g !== from ? [g] : to ? [to] : [])))]
    ])
  );

/** `order` with the group `from` called `to`, or left out for null */
const renamedInOrder = (order: string[], from: string, to: string | null): string[] =>
  order.flatMap((g) => (g !== from ? [g] : to ? [to] : []));

/** `names` with the one at `from` moved to `to`, the others keeping their order */
export const movedIn = (names: string[], from: number, to: number): string[] => {
  const next = names.filter((_, i) => i !== from);
  next.splice(to, 0, names[from]);
  return next;
};

/** A group's upload settings as the options hold them, nested as `limits.daily.files` */
export type Upload = Record<string, unknown>;

type LimitWindow = Schemas["LimitWindow"];
type LimitMeasure = Schemas["LimitMeasure"];
const LIMIT_WINDOWS: LimitWindow[] = ["queued", "daily", "weekly"];
const LIMIT_MEASURES: LimitMeasure[] = ["files", "megabytes", "failures"];

/** A count a group can set: its dotted key inside the upload settings, and what `0` means */
export type UploadField = {
  key: string;
  label: string;
  /** what `0` is called in a summary */
  none: string;
  /** a set value in a summary */
  text: (n: number) => string;
  /** the per-user limit it is, for one */
  limit?: { window: LimitWindow; measure: LimitMeasure };
};

const WINDOW_WORDS: Record<LimitWindow, string> = {
  queued: "queued",
  daily: "daily",
  weekly: "weekly"
};
const MEASURE_WORDS: Record<LimitMeasure, [one: string, many: string, unit: string]> = {
  files: ["file", "files", "file"],
  megabytes: ["MB", "MB", "size"],
  failures: ["failure", "failures", "failure"]
};

const capital = (text: string) => `${text[0].toUpperCase()}${text.slice(1)}`;

/** A per-user limit in a few words, "Daily 10 files", or "No daily file limit" for none */
export const limitText = (window: LimitWindow, measure: LimitMeasure, limit: number | null) => {
  const [one, many, unit] = MEASURE_WORDS[measure];
  return limit === null || limit === 0
    ? `No ${WINDOW_WORDS[window]} ${unit} limit`
    : `${capital(WINDOW_WORDS[window])} ${measure === "megabytes" ? `${limit} MB` : plural(limit, one, many)}`;
};

/** The counts a group can set, in the order a form shows them */
export const UPLOAD_COUNTS: UploadField[] = [
  {
    key: "slots",
    label: "Upload slots",
    none: "Every slot",
    text: (n) => plural(n, "slot", "slots")
  },
  {
    key: "speed_limit",
    label: "Speed limit (KiB/s)",
    none: "No speed limit",
    text: (n) => `${n} KiB/s`
  },
  ...LIMIT_WINDOWS.flatMap((window) =>
    LIMIT_MEASURES.map((measure) => ({
      key: `limits.${window}.${measure}`,
      label: `${capital(WINDOW_WORDS[window])} ${measure}`,
      none: limitText(window, measure, null),
      text: (n: number) => limitText(window, measure, n),
      limit: { window, measure }
    }))
  )
];

/** The queue orders, by the names the options hold */
export const STRATEGIES: Record<string, string> = {
  roundrobin: "Round robin",
  firstinfirstout: "First in, first out"
};

/** Every setting of a group's uploads, by its dotted key */
const UPLOAD_KEYS = ["strategy", ...UPLOAD_COUNTS.map((f) => f.key)];

/**
 * The draft of a group's upload settings: the text each field's input holds, empty for one not
 * set and `0` for no limit
 */
export const uploadDraft = (upload: Upload | null | undefined): Record<string, string> =>
  Object.fromEntries(
    UPLOAD_KEYS.map((key) => {
      const held = at(upload, key);
      return [key, held === null || held === undefined ? "" : String(held)];
    })
  );

/** The count of at least `min` typed in `text`, null for none, or why `label` can't take it */
export const countIn = (text: string, label: string, min: number): number | null | string => {
  if (!text.trim()) return null;
  const n = Number(text);
  return Number.isInteger(n) && n >= min ? n : `${label} must be a whole number of at least ${min}`;
};

/**
 * The upload settings a draft holds, every one of them with null for not set, or what is wrong
 * with the first field that can't be saved
 */
export const uploadOf = (draft: Record<string, string>): { upload: Upload } | { error: string } => {
  const upload: Upload = {
    strategy: draft.strategy || null,
    limits: Object.fromEntries(LIMIT_WINDOWS.map((w) => [w, {}]))
  };
  for (const field of UPLOAD_COUNTS) {
    const n = countIn(draft[field.key] ?? "", field.label, 0);
    if (typeof n === "string") return { error: n };
    if (field.limit)
      (upload.limits as Record<string, Upload>)[field.limit.window][field.limit.measure] = n;
    else upload[field.key] = n;
  }
  return { upload };
};

/** What a group sets, a phrase each, in the order a form shows them; nothing for none */
export const settingsText = (upload: Upload | null | undefined): string[] => {
  const strategy = at(upload, "strategy");
  return [
    ...(typeof strategy === "string" ? [STRATEGIES[strategy] ?? strategy] : []),
    ...UPLOAD_COUNTS.flatMap((field) => {
      const held = at(upload, field.key);
      if (typeof held !== "number") return [];
      return [held === 0 ? field.none : field.text(held)];
    })
  ];
};

/** The names of the settings a group sets, as a person reads them, "daily file limit" */
export const settingNames = (upload: Upload | null | undefined): string[] => {
  const strategy = at(upload, "strategy");
  return [
    ...(typeof strategy === "string" ? ["queue order"] : []),
    ...UPLOAD_COUNTS.flatMap((field) => {
      if (typeof at(upload, field.key) !== "number") return [];
      if (!field.limit) return [field.key === "slots" ? "slots" : "speed limit"];
      return [`${WINDOW_WORDS[field.limit.window]} ${MEASURE_WORDS[field.limit.measure][2]} limit`];
    })
  ];
};

/** The settings of `names` that `upload` does not set */
export const notSetBy = (names: string[], upload: Upload | null | undefined) => {
  const held = new Set(settingNames(upload));
  return names.filter((name) => !held.has(name));
};

/** The name a group is shown by: the default group is everyone */
export const groupLabel = (name: string) => (name === DEFAULT ? "Everyone" : name);

/**
 * What a user's uploads get and which group each comes from, "Queue: buddies · Speed: no limit
 * (buddies) · Daily 10 files (leechers)"; a group that sets nothing is not named
 */
export const uploadsText = (uploads: Schemas["UserUploads"]): string =>
  [
    `Queue: ${groupLabel(uploads.queue)} (${uploads.slots ? plural(uploads.slots, "slot", "slots") : "every slot"})`,
    `Speed: ${uploads.speedLimit ? `${uploads.speedLimit} KiB/s` : "no limit"} (${groupLabel(uploads.speed)})`,
    ...uploads.limits.map(
      (l) => `${limitText(l.window, l.measure, l.limit ?? null)} (${groupLabel(l.group)})`
    )
  ].join(" · ");

/** The group whose queue a user waits in, with its color, null for everyone's */
export const queueOf = (user: { groups: UserGroup[]; uploads: Schemas["UserUploads"] }) =>
  user.groups.find((g) => g.name === user.uploads.queue) ?? null;

/** The settings buddies always have, every slot and no speed limit unless a count is given */
const BUDDIES_ALWAYS = ["slots", "speed_limit"];

/** Whether the setting `key` of `group` always holds a value, so empty means no limit */
export const alwaysSet = (group: string, key: string) =>
  group === BUDDIES && BUDDIES_ALWAYS.includes(key);

/** What the group editor holds when it saves */
export type GroupForm = {
  /** the group's name as the form opened, empty for a new group */
  from: string;
  /** a user-defined group or a new one, which is an entry of the user-defined groups */
  own: boolean;
  name: string;
  color: string;
  /** the upload settings as drafted, and as they were when the form opened */
  draft: Record<string, string>;
  started: Record<string, string>;
  /** the share aliases the group sees, null for a group that can't be shown shares */
  seen: string[] | null;
  /** the leechers' thresholds as typed, and as they were, for the leechers */
  thresholds?: { files: string; directories: string; was: { files: string; directories: string } };
};

/** An options write: what is set and what is taken out */
export type OptionsPatch = { set: Record<string, unknown>; unset: string[] };

/** The shares that `after` shows to no one and `before` did not */
const hiddenBy = (before: VisibleTo, after: VisibleTo) =>
  Object.entries(after).flatMap(([alias, groups]) =>
    groups.length === 0 && audienceOf(before, alias)?.length !== 0 ? [alias] : []
  );

const listOf = (values: unknown, key: string) => (at(values, key) ?? []) as string[];
const mapOf = <T>(values: unknown, key: string) => (at(values, key) ?? {}) as Record<string, T>;

/**
 * The options a group editor's save writes over `values`, the options as they run, with the
 * shares it leaves seen by no one; or what is wrong with the form. A user-defined group is one
 * whole entry of the user-defined groups, null for a setting not set; a built-in group's settings
 * are each an option, written when changed and taken out when not set. A rename follows into the
 * order and the share lists
 */
export const groupPatch = (
  values: unknown,
  form: GroupForm
): (OptionsPatch & { hidden: string[] }) | { error: string } => {
  const set: Record<string, unknown> = {};
  const unset: string[] = [];
  const draft = { ...form.draft };
  for (const key of BUDDIES_ALWAYS) {
    if (alwaysSet(form.from, key) && !draft[key].trim()) draft[key] = "0";
  }
  const parsed = uploadOf(draft);
  if ("error" in parsed) return parsed;
  const renamed = !!form.from && form.name !== form.from;
  if (form.own) {
    const groups = mapOf<unknown>(values, USER_DEFINED);
    set[USER_DEFINED] = {
      ...Object.fromEntries(Object.entries(groups).filter(([n]) => n !== form.from)),
      [form.name]: { color: form.color, upload: parsed.upload }
    };
    const order = listOf(values, ORDER);
    if (renamed && order.includes(form.from))
      set[ORDER] = renamedInOrder(order, form.from, form.name);
  } else {
    for (const key of UPLOAD_KEYS) {
      if (draft[key] === form.started[key]) continue;
      const option = `transfers.groups.${form.from}.upload.${key}`;
      const value = at(parsed.upload, key);
      if (value === null || value === undefined) unset.push(option);
      else set[option] = value;
    }
  }
  if (form.thresholds) {
    const { was } = form.thresholds;
    for (const [key, label] of [
      ["files", "Minimum shared files"],
      ["directories", "Minimum shared folders"]
    ] as const) {
      const text = form.thresholds[key];
      if (text === was[key]) continue;
      const n = countIn(text, label, 1);
      if (typeof n === "string") return { error: n };
      const option = `transfers.groups.leechers.thresholds.${key}`;
      if (n === null) unset.push(option);
      else set[option] = n;
    }
  }
  const before = mapOf<string[]>(values, VISIBLE_TO);
  let after = before;
  if (form.seen) {
    after = withShares(
      renamed ? renamedIn(before, form.from, form.name) : before,
      form.name,
      form.seen
    );
    if (!same(after, before)) set[VISIBLE_TO] = after;
  }
  return { set, unset, hidden: hiddenBy(before, after) };
};

/**
 * The options that delete the user-defined group `name` over `values`: out of the groups, the
 * order and every share's list, with the shares that leaves seen by no one
 */
export const deletePatch = (values: unknown, name: string): OptionsPatch & { hidden: string[] } => {
  const groups = mapOf<unknown>(values, USER_DEFINED);
  const set: Record<string, unknown> = {
    [USER_DEFINED]: Object.fromEntries(Object.entries(groups).filter(([n]) => n !== name))
  };
  const order = listOf(values, ORDER);
  if (order.includes(name)) set[ORDER] = renamedInOrder(order, name, null);
  const before = mapOf<string[]>(values, VISIBLE_TO);
  const after = renamedIn(before, name, null);
  if (!same(after, before)) set[VISIBLE_TO] = after;
  return { set, unset: [], hidden: hiddenBy(before, after) };
};

/** `done`, then which shares are now seen by no one, when any are */
export const withHidden = (done: string, hidden: string[]) =>
  hidden.length
    ? `${done} · ${listText(hidden)} ${hidden.length === 1 ? "is" : "are"} now seen by no one`
    : done;
