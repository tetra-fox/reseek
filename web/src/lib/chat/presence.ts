/** What happened to a member of a room, as the room's muted lines say it */
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

/** Changes of one kind close together, shown as one line at the time of the first */
export type Burst = { key: string; at: string; kind: PresenceKind; names: string[] };

/** Changes of the same kind this close to the one before fold into one line */
export const FOLD_MS = 10_000;

/** How many names a line spells out before "and N others" */
export const NAMED = 3;

/**
 * Folds changes, in the order they arrived, into bursts: a change joins the burst before it
 * when it is of the same kind and comes within `FOLD_MS` of that burst's last change. A name
 * shows once in a burst. A burst is keyed by its first change, so it keeps its line as it grows
 */
export const fold = (events: PresenceEvent[]): Burst[] => {
  const bursts: Burst[] = [];
  let open: { burst: Burst; last: number } | null = null;
  for (const e of events) {
    const at = Date.parse(e.at);
    if (open && open.burst.kind === e.kind && at - open.last <= FOLD_MS) {
      if (!open.burst.names.includes(e.username)) open.burst.names.push(e.username);
      open.last = at;
      continue;
    }
    const burst: Burst = { key: `presence:${e.id}`, at: e.at, kind: e.kind, names: [e.username] };
    bursts.push(burst);
    open = { burst, last: at };
  }
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
