/** What happened to a member of a room, as the room's activity pane says it */
export type PresenceKind =
  | "joined"
  | "left"
  | "away"
  | "returned"
  | "added"
  | "removed"
  | "operator"
  | "notOperator";

/** One member's change, as it arrived */
export type PresenceEvent = { id: number; at: string; kind: PresenceKind; username: string };

/**
 * Changes of one kind close together, shown as one line at the time of the first; `last` is
 * when its latest change came
 */
export type Burst = { key: string; at: string; last: string; kind: PresenceKind; names: string[] };

/** Changes of the same kind this close to the one before fold into one line */
export const FOLD_MS = 10_000;

/** How many names a line spells out before "and N others" */
export const NAMED = 3;

/**
 * Adds a change to the bursts it follows, oldest first, in place: it joins the last burst when
 * it is of the same kind and comes within `FOLD_MS` of that burst's last change, and starts a
 * new one otherwise. A name shows once in a burst. A burst is keyed by its first change, so it
 * keeps its line as it grows
 */
export const addChange = (bursts: Burst[], e: PresenceEvent) => {
  const open = bursts.at(-1);
  if (open && open.kind === e.kind && Date.parse(e.at) - Date.parse(open.last) <= FOLD_MS) {
    if (!open.names.includes(e.username)) open.names.push(e.username);
    open.last = e.at;
    return;
  }
  bursts.push({ key: `presence:${e.id}`, at: e.at, last: e.at, kind: e.kind, names: [e.username] });
};

/** Changes, in the order they arrived, folded into bursts */
export const fold = (events: PresenceEvent[]): Burst[] => {
  const bursts: Burst[] = [];
  for (const e of events) addChange(bursts, e);
  return bursts;
};

/** What a line says after its names, by kind, for one name and for several */
const VERBS: Record<PresenceKind, [string, string]> = {
  joined: ["joined the room", "joined"],
  left: ["left the room", "left"],
  away: ["has gone away", "have gone away"],
  returned: ["has returned", "have returned"],
  added: ["was added to the room", "were added to the room"],
  removed: ["was removed from the room", "were removed from the room"],
  operator: ["was made an operator", "were made operators"],
  notOperator: ["is no longer an operator", "are no longer operators"]
};

/**
 * A burst's line in parts: the names it spells out, how many others it counts, and the words
 * after. "alice joined the room", "alice and bob joined", "alice, bob and 3 others joined"
 */
export const burstLine = (b: Burst) => {
  const shown = b.names.length > NAMED + 1 ? b.names.slice(0, NAMED - 1) : b.names;
  const others = b.names.length - shown.length;
  return { shown, others, verb: VERBS[b.kind][b.names.length === 1 ? 0 : 1] };
};

/** A burst's line as text */
export const burstText = (b: Burst): string => {
  const { shown, others, verb } = burstLine(b);
  const names = others ? [...shown, `${others} others`] : shown;
  const list =
    names.length === 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
  return `${list} ${verb}`;
};
