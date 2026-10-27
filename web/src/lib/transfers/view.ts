import { getDirectoryName, getFileName } from "#lib/format.js";

import { type ChipId, STATE_CHIPS, type Transfer } from "./states";

export type GroupBy = "user" | "folder" | "batch" | "flat";

/**
 * The text filter over transfers: `user:`, `folder:` and `ext:` match those parts, any other word
 * the username, file name or folder; every token must match. Null for an empty filter
 */
export const textFilter = (text: string): ((t: Transfer) => boolean) | null => {
  const tokens = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return null;
  return (t) => {
    const user = t.username.toLowerCase();
    const name = getFileName(t.filename).toLowerCase();
    const folder = getDirectoryName(t.filename).toLowerCase();
    return tokens.every((token) => {
      if (token.startsWith("user:")) return user.includes(token.slice(5));
      if (token.startsWith("folder:")) return folder.includes(token.slice(7));
      if (token.startsWith("ext:")) {
        return name.split(".").pop() === token.slice(4).replace(/^\./, "");
      }
      return user.includes(token) || name.includes(token) || folder.includes(token);
    });
  };
};

/** Whether a transfer is in one of the chosen state chips; every transfer when none is chosen */
export const inStates = (chips: ChipId[]) => {
  const tests = STATE_CHIPS.filter((c) => chips.includes(c.id)).map((c) => c.test);
  return (t: Transfer) => !tests.length || tests.some((test) => test(t.state));
};

/** A group header in the table: a user, a folder of a user, or a batch */
export type Group = {
  kind: "user" | "folder" | "batch";
  key: string;
  label: string;
  username: string;
  /** 1 for a folder under a user header */
  depth: number;
  transfers: Transfer[];
};

export type Item =
  | { type: "group"; key: string; group: Group }
  | { type: "row"; key: string; transfer: Transfer; group: Group | null };

export type SortKey =
  | "file"
  | "user"
  | "size"
  | "progress"
  | "speed"
  | "eta"
  | "requested"
  | "started"
  | "ended"
  | "attempts";

const time = (iso: string | null | undefined) => (iso ? Date.parse(iso) : 0);

const VALUE: Record<SortKey, (t: Transfer) => number | string> = {
  file: (t) => getFileName(t.filename).toLowerCase(),
  user: (t) => t.username.toLowerCase(),
  size: (t) => t.size,
  progress: (t) => t.percentComplete,
  speed: (t) => t.averageSpeed,
  eta: (t) => t.remainingTime ?? Number.MAX_SAFE_INTEGER,
  requested: (t) => time(t.requestedAt),
  started: (t) => time(t.startedAt),
  ended: (t) => time(t.endedAt),
  attempts: (t) => t.attempts
};

/** Transfers in the order a column says, ties by file name */
export const sortTransfers = (rows: Transfer[], key: SortKey, dir: "asc" | "desc") => {
  const sign = dir === "asc" ? 1 : -1;
  const value = VALUE[key];
  const name = VALUE.file;
  return rows.toSorted((a, b) => {
    const x = value(a);
    const y = value(b);
    const by = typeof x === "string" ? x.localeCompare(y as string) : (x as number) - (y as number);
    return sign * by || (name(a) as string).localeCompare(name(b) as string);
  });
};

const bucket = <K>(rows: Transfer[], key: (t: Transfer) => K) => {
  const groups = new Map<K, Transfer[]>();
  for (const t of rows) {
    const k = key(t);
    groups.set(k, [...(groups.get(k) ?? []), t]);
  }
  return groups;
};

/**
 * The table's lines: group headers each followed by their rows, or the rows alone when flat.
 * Groups keep the order their first transfer has in `sorted`, and the rows inside a group keep
 * the sort. A folded group shows its header only
 */
export const layout = (sorted: Transfer[], groupBy: GroupBy, folded: Set<string>): Item[] => {
  const items: Item[] = [];
  const rows = (group: Group | null, list: Transfer[]) => {
    for (const t of list) items.push({ type: "row", key: t.id, transfer: t, group });
  };
  if (groupBy === "flat") {
    rows(null, sorted);
    return items;
  }
  const add = (group: Group, children?: () => void) => {
    items.push({ type: "group", key: group.key, group });
    if (folded.has(group.key)) return;
    if (children) children();
    else rows(group, group.transfers);
  };
  if (groupBy === "user") {
    for (const [username, list] of bucket(sorted, (t) => t.username)) {
      const user: Group = {
        kind: "user",
        key: `user:${username}`,
        label: username,
        username,
        depth: 0,
        transfers: list
      };
      add(user, () => {
        for (const [folder, inFolder] of bucket(list, (t) => getDirectoryName(t.filename))) {
          add({
            kind: "folder",
            key: `folder:${username}\u0000${folder}`,
            label: folder,
            username,
            depth: 1,
            transfers: inFolder
          });
        }
      });
    }
  } else if (groupBy === "folder") {
    for (const [key, list] of bucket(
      sorted,
      (t) => `${t.username}\u0000${getDirectoryName(t.filename)}`
    )) {
      add({
        kind: "folder",
        key: `folder:${key}`,
        label: getDirectoryName(list[0].filename),
        username: list[0].username,
        depth: 0,
        transfers: list
      });
    }
  } else {
    for (const [batch, list] of bucket(sorted, (t) => t.batchId ?? null)) {
      add({
        kind: "batch",
        key: `batch:${batch ?? "none"}`,
        label: batch ? `Batch ${batch.slice(0, 8)}` : "Not in a batch",
        username: list[0].username,
        depth: 0,
        transfers: list
      });
    }
  }
  return items;
};
