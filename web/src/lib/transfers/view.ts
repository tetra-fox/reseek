import { type Mark, markToken, passes } from "#lib/analysis/marks.js";
import { getDirectoryName, getFileName } from "#lib/format.js";
import { type FolderNode, folderTree } from "#lib/tree/folders.js";

import { type ChipId, STATE_CHIPS, type Transfer } from "./states";

export type GroupBy = "user" | "folder" | "batch" | "flat";

/** A transfer's analysis as filters read it: its mark, and whether it could be analyzed */
export type MarkLookup = (t: Transfer) => { mark: Mark | undefined; analyzable: boolean };

/**
 * The text filter over transfers: `user:`, `folder:` and `ext:` match those parts, `marked:fake`,
 * `marked:genuine`, `analyzed:yes` and `analyzed:no` the file's analysis read through `marks`
 * (nothing without it), any other word the username, file name or folder; every token must
 * match. Null for an empty filter
 */
export const textFilter = (text: string, marks?: MarkLookup): ((t: Transfer) => boolean) | null => {
  const tokens = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return null;
  return (t) => {
    const user = t.username.toLowerCase();
    const name = getFileName(t.filename).toLowerCase();
    const folder = getDirectoryName(t.filename).toLowerCase();
    return tokens.every((token) => {
      const byMark = markToken(token);
      if (byMark) {
        if (!marks) return false;
        const { mark, analyzable } = marks(t);
        return passes(byMark, mark, analyzable);
      }
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
  /** how far it is nested: under a user, under a folder */
  depth: number;
  /** every transfer under it, in subfolders too */
  transfers: Transfer[];
  /** a folder's remote path */
  folder?: string;
  /** the keys of the groups beside it, itself included, which Alt-click opens or closes with it */
  siblings: string[];
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

const bucket = <K>(rows: Transfer[], key: (t: Transfer) => K) => {
  const groups = new Map<K, Transfer[]>();
  for (const t of rows) {
    const k = key(t);
    const group = groups.get(k);
    if (group) group.push(t);
    else groups.set(k, [t]);
  }
  return groups;
};

/**
 * The table's lines: group headers each followed by their rows, or the rows alone when flat. A
 * user's folders are a tree, a folder nested under the folder it is in. Groups keep the order
 * their first transfer has in `sorted`, and the rows inside a group keep the sort. A closed
 * group shows its header only
 */
export const layout = (
  sorted: Transfer[],
  groupBy: GroupBy,
  isOpen: (key: string) => boolean
): Item[] => {
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
    if (!isOpen(group.key)) return;
    if (children) children();
    else rows(group, group.transfers);
  };
  /** a user's folders as nested groups, from `depth` down */
  const folders = (username: string, list: Transfer[], depth: number) => {
    const key = (node: FolderNode<Transfer>) => `folder:${username}\u0000${node.path}`;
    const under = (node: FolderNode<Transfer>): Transfer[] => [
      ...node.items,
      ...node.children.flatMap(under)
    ];
    const walk = (nodes: FolderNode<Transfer>[], at: number) => {
      const siblings = nodes.map(key);
      for (const node of nodes) {
        const group: Group = {
          kind: "folder",
          key: key(node),
          label: node.label,
          username,
          depth: at,
          transfers: under(node),
          folder: node.path,
          siblings
        };
        add(group, () => {
          rows(group, node.items);
          walk(node.children, at + 1);
        });
      }
    };
    walk(
      folderTree(
        list,
        (t) => t.filename,
        (t) => t.size
      ),
      depth
    );
  };
  if (groupBy === "user") {
    const byUser = [...bucket(sorted, (t) => t.username)];
    const siblings = byUser.map(([username]) => `user:${username}`);
    for (const [username, list] of byUser) {
      const user: Group = {
        kind: "user",
        key: `user:${username}`,
        label: username,
        username,
        depth: 0,
        transfers: list,
        siblings
      };
      add(user, () => folders(username, list, 1));
    }
  } else if (groupBy === "folder") {
    for (const [username, list] of bucket(sorted, (t) => t.username)) folders(username, list, 0);
  } else {
    const batches = [...bucket(sorted, (t) => t.batchId ?? null)];
    const siblings = batches.map(([batch]) => `batch:${batch ?? "none"}`);
    for (const [batch, list] of batches) {
      add({
        kind: "batch",
        key: `batch:${batch ?? "none"}`,
        label: batch ? `Batch ${batch.slice(0, 8)}` : "Not in a batch",
        username: list[0].username,
        depth: 0,
        transfers: list,
        siblings
      });
    }
  }
  return items;
};
