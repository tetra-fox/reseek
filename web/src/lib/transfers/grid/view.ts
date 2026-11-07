import { type Mark, MARK_FILTERS, markToken, passes } from "#lib/analysis/marks.js";
import { fileMatches, filterPlaceholder, parseFilters } from "#lib/files/filters.js";
import { fromWhom, getDirectoryName, getFileName } from "#lib/format.js";
import { type ActiveFilter, typedFilter } from "#lib/lists/filtered.js";
import type { Layout } from "#lib/lists/layout.js";
import { type ChipId, STATE_CHIPS, type Transfer } from "#lib/transfers/states.js";
import {
  commonFolder,
  type FolderNode,
  folderNode,
  folderTree,
  foldersUnder,
  itemsUnder,
  NO_FOLDER,
  separatorOf,
  type TreeLine
} from "#lib/tree/folders.js";

import type { ViewSettings } from "./settings.svelte";

/** What the tree layout groups transfers by */
export type GroupBy = "user" | "folder" | "job";

/** The grouping a view's lines take: its own in the tree layout, none in the list */
export const groupingOf = (view: { layout: Layout; groupBy: GroupBy }): GroupBy | null =>
  view.layout === "tree" ? view.groupBy : null;

/** A transfer's analysis as filters read it: its mark, and whether it could be analyzed */
export type MarkLookup = (t: Transfer) => { mark: Mark | undefined; analyzable: boolean };

/** The filter tokens of the transfers screen's own, beside those every file filter takes */
export const TRANSFER_KEYS = ["user", "folder", "marked", "analyzed"] as const;

/** The transfers filter box's placeholder */
export const TRANSFER_PLACEHOLDER = filterPlaceholder([
  "user",
  "folder",
  "word",
  "not",
  "type",
  "size"
]);

/**
 * The text filter over transfers: the file filter's language over the user and the path, plus
 * `user:` and `folder:`, which match those parts, and `marked:fake`, `marked:genuine`,
 * `analyzed:yes` and `analyzed:no`, which read the file's analysis through `marks` (nothing
 * without it); every token must hold. Null for an empty filter
 */
export const textFilter = (text: string, marks?: MarkLookup): ((t: Transfer) => boolean) | null => {
  if (!text.trim()) return null;
  const filters = parseFilters(text, TRANSFER_KEYS);
  return (t) => {
    const user = t.username.toLowerCase();
    const folder = getDirectoryName(t.filename).toLowerCase();
    for (const token of filters.screen) {
      let holds: boolean;
      if (token.name === "user") holds = user.includes(token.value);
      else if (token.name === "folder") holds = folder.includes(token.value);
      else {
        const byMark = markToken(`${token.name}:${token.value}`);
        const read = marks?.(t);
        holds = !!byMark && !!read && passes(byMark, read.mark, read.analyzable);
      }
      if (holds === token.negated) return false;
    }
    return fileMatches(filters, t, `${t.username}\n${t.filename}`);
  };
};

/** Whether a transfer is in one of the chosen state chips; every transfer when none is chosen */
export const inStates = (chips: ChipId[]) => {
  const tests = STATE_CHIPS.filter((c) => chips.includes(c.id)).map((c) => c.test);
  return (t: Transfer) => !tests.length || tests.some((test) => test(t.state));
};

/**
 * The filters narrowing the transfers screen, each taken off alone: the state chips in their
 * order, the analysis chip, then the text, which `clearText` empties
 */
export const activeFilters = (
  filters: ViewSettings["filters"],
  text: string,
  clearText: () => void
): ActiveFilter[] => {
  const on: ActiveFilter[] = STATE_CHIPS.filter((c) => filters.states.includes(c.id)).map((c) => ({
    id: `state:${c.id}`,
    label: c.label,
    clear: () => (filters.states = filters.states.filter((s) => s !== c.id))
  }));
  const mark = MARK_FILTERS.find((f) => f.id === filters.analysis);
  if (mark) on.push({ id: "analysis", label: mark.label, clear: () => (filters.analysis = null) });
  return [...on, ...typedFilter(text, clearText)];
};

/** A group header in the table: a user, a folder of a user, or a download job */
export type Group = {
  kind: "user" | "folder" | "job";
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
  /** the keys of the groups inside it, at every depth, which Ctrl-click opens or closes with it */
  inside: () => string[];
};

export type Item =
  | { type: "group"; key: string; group: Group }
  | { type: "row"; key: string; transfer: Transfer; group: Group | null };

/** How far a line is nested: a group header at its group's depth, a row one past its group's */
export const depthOf = (item: Item) =>
  item.type === "group" ? item.group.depth : item.group ? item.group.depth + 1 : 0;

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

/**
 * A download job's name from its downloads: its one file, else the folder holding every file
 * when they come from one user, else who they came from
 */
const jobLabel = (list: Transfer[]) => {
  const who = fromWhom([...new Set(list.map((t) => t.username))]);
  if (list.length === 1) return `${getFileName(list[0].filename)} from ${who}`;
  const separator = separatorOf(list[0].filename);
  const folder = list.every((t) => t.username === list[0].username)
    ? commonFolder(
        list.map((t) => t.filename),
        separator
      )
    : "";
  // a folder at the top of a share names the share, not what was picked from it
  return folder.includes(separator)
    ? `${getFileName(folder)} from ${who}`
    : `Downloads from ${who}`;
};

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
 * The table's lines: group headers each followed by their rows, or the rows alone with no
 * grouping. A user's folders are a tree, a folder nested under the folder it is in. Groups keep
 * the order their first transfer has in `sorted`, and the rows inside a group keep the sort. A
 * closed group shows its header only
 */
export const layout = (
  sorted: Transfer[],
  groupBy: GroupBy | null,
  isOpen: (key: string) => boolean
): Item[] => {
  const items: Item[] = [];
  const rows = (group: Group | null, list: Transfer[]) => {
    for (const t of list) items.push({ type: "row", key: t.id, transfer: t, group });
  };
  if (groupBy === null) {
    rows(null, sorted);
    return items;
  }
  const add = (group: Group, children?: () => void) => {
    items.push({ type: "group", key: group.key, group });
    if (!isOpen(group.key)) return;
    if (children) children();
    else rows(group, group.transfers);
  };
  const folderKey = (username: string) => (node: FolderNode<Transfer>) =>
    `folder:${username}\u0000${node.path}`;
  const tree = (list: Transfer[]) =>
    folderTree(
      list,
      (t) => t.filename,
      (t) => t.size
    );
  /** a user's folders as nested groups, from `depth` down */
  const folders = (username: string, list: Transfer[], depth: number) => {
    const key = folderKey(username);
    const walk = (nodes: FolderNode<Transfer>[], at: number) => {
      const siblings = nodes.map(key);
      for (const node of nodes) {
        const group: Group = {
          kind: "folder",
          key: key(node),
          label: node.label,
          username,
          depth: at,
          transfers: itemsUnder(node),
          folder: node.path,
          siblings,
          inside: () => foldersUnder(node, key)
        };
        add(group, () => {
          rows(group, node.items);
          walk(node.children, at + 1);
        });
      }
    };
    walk(tree(list), depth);
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
        siblings,
        inside: () => {
          const key = folderKey(username);
          return tree(list).flatMap((node) => [key(node), ...foldersUnder(node, key)]);
        }
      };
      add(user, () => folders(username, list, 1));
    }
  } else if (groupBy === "folder") {
    for (const [username, list] of bucket(sorted, (t) => t.username)) folders(username, list, 0);
  } else {
    const jobs = [...bucket(sorted, (t) => t.jobId ?? null)];
    const siblings = jobs.map(([job]) => `job:${job ?? "none"}`);
    for (const [job, list] of jobs) {
      add({
        kind: "job",
        key: `job:${job ?? "none"}`,
        label: job ? jobLabel(list) : "Other downloads",
        username: list[0].username,
        depth: 0,
        transfers: list,
        siblings,
        inside: () => []
      });
    }
  }
  return items;
};

const size = (t: Transfer) => t.size;

/**
 * The table's lines as a tree's: a group header as a folder line holding every transfer under
 * the group, a row as an item line, each keyed as its line is; and each group by its key
 */
export const treeOf = (items: Item[]) => {
  const groups = new Map<string, Group>();
  const nodes = new Map<Group, FolderNode<Transfer>>();
  const nodeOf = (group: Group) => {
    let node = nodes.get(group);
    if (!node) {
      const at = { key: group.key, path: group.folder ?? group.label, label: group.label };
      node = folderNode(at, group.transfers, [], size);
      nodes.set(group, node);
      groups.set(group.key, group);
    }
    return node;
  };
  const lines = items.map((item): TreeLine<Transfer> =>
    item.type === "group"
      ? {
          kind: "folder",
          key: item.key,
          node: nodeOf(item.group),
          depth: item.group.depth,
          siblings: item.group.siblings
        }
      : {
          kind: "item",
          key: item.key,
          item: item.transfer,
          depth: depthOf(item),
          folder: item.group ? nodeOf(item.group) : NO_FOLDER
        }
  );
  return { lines, groups };
};
