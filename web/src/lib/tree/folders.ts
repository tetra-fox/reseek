import type { Schemas } from "#lib/api/client.js";
import { getDirectoryName, getFileName } from "#lib/format.js";

/**
 * A folder of a tree: the items directly in it, the folders under it, and what it holds counting
 * everything below
 */
export type FolderNode<T> = {
  /** what keys its line and whether it is open: its path, made unique when trees share a list */
  key: string;
  /** the full path */
  path: string;
  /** what its line shows: the path from the parent node down to this one */
  label: string;
  items: T[];
  children: FolderNode<T>[];
  /** items here and in every folder below */
  count: number;
  /** their sizes added up */
  bytes: number;
  /** folders below it, at every depth */
  folders: number;
  /** items here and below that can be ticked and downloaded: those not locked */
  unlocked: number;
  /** nothing in it can be downloaded: the user shares it only with some */
  locked: boolean;
};

export type Separator = "\\" | "/";

/** The separator a path uses: `/` when it has one, else `\` */
export const separatorOf = (path: string): Separator => (path.includes("/") ? "/" : "\\");

/** The path a folder is in, "" for one at the top */
export const parentOf = (path: string, separator: Separator) => {
  const cut = path.lastIndexOf(separator);
  return cut === -1 ? "" : path.slice(0, cut);
};

/** The deepest folder that holds every one of `paths`, "" when they share none */
export const commonFolder = (paths: string[], separator: Separator) => {
  let common = getDirectoryName(paths[0]);
  for (const path of paths) {
    const folder = getDirectoryName(path);
    while (common && folder !== common && !folder.startsWith(common + separator)) {
      common = parentOf(common, separator);
    }
  }
  return common;
};

/** Every item in a folder and in the folders below it */
export const itemsUnder = <T>(node: FolderNode<T>): T[] => {
  const found: T[] = [];
  const stack = [node];
  while (stack.length) {
    const at = stack.pop()!;
    found.push(...at.items);
    stack.push(...at.children.toReversed());
  }
  return found;
};

/**
 * A folder with what it holds counted from its items and the folders under it. It is locked as
 * `at` says, or when every item under it is, by `lockedOf`
 */
export const folderNode = <T>(
  at: { key: string; path: string; label: string; locked?: boolean },
  items: T[],
  children: FolderNode<T>[],
  sizeOf: (item: T) => number,
  lockedOf?: (item: T) => boolean
): FolderNode<T> => {
  const free = lockedOf ? items.filter((item) => !lockedOf(item)).length : items.length;
  return {
    ...at,
    items,
    children,
    count: items.length + children.reduce((n, c) => n + c.count, 0),
    bytes:
      items.reduce((n, item) => n + sizeOf(item), 0) + children.reduce((n, c) => n + c.bytes, 0),
    folders: children.reduce((n, c) => n + 1 + c.folders, 0),
    unlocked: free + children.reduce((n, c) => n + c.unlocked, 0),
    locked: at.locked ?? (!!lockedOf && items.every(lockedOf) && children.every((c) => c.locked))
  };
};

/** The folder a flat list's items are under: none, so their group is the whole list */
export const NO_FOLDER: FolderNode<never> = folderNode(
  { key: "", path: "", label: "" },
  [],
  [],
  () => 0
);

/** A path's key in a list that holds several trees, unique to the tree `scope` names */
export const scopedKey = (scope: string, path: string) => `${scope}\u0000${path}`;

type Trie<T> = {
  path: string;
  segment: string;
  separator: Separator;
  items: T[];
  children: Map<string, Trie<T>>;
};

/**
 * The folders a file's path is in, outermost first: everything before its last separator, a
 * folder per part. Separators at the start stay with the first part, so a Unix path's first
 * folder is "/home" and the root itself "/". A bare file name, which some peers send, is in none
 */
const foldersOf = (path: string, separator: Separator): string[] => {
  const cut = path.lastIndexOf(separator);
  if (cut === -1) return [];
  let start = 0;
  while (path[start] === separator) start++;
  if (cut < start) return [path.slice(0, start)];
  const [first, ...rest] = path.slice(start, cut).split(separator);
  return [path.slice(0, start) + first, ...rest];
};

/**
 * The trie of the folders the items are in, folders and items in the order they first appear,
 * and the items with no folder at its root
 */
const trieOf = <T>(items: T[], pathOf: (item: T) => string): Trie<T> => {
  const root: Trie<T> = { path: "", segment: "", separator: "\\", items: [], children: new Map() };
  for (const item of items) {
    const path = pathOf(item);
    const separator = separatorOf(path);
    let at = root;
    for (const segment of foldersOf(path, separator)) {
      let next = at.children.get(segment);
      if (!next) {
        const walked = at === root ? segment : at.path + separator + segment;
        next = { path: walked, segment, separator, items: [], children: new Map() };
        at.children.set(segment, next);
      }
      at = next;
    }
    at.items.push(item);
  }
  return root;
};

/**
 * The trie's folders as nodes. A folder holding nothing but one folder is one node with it,
 * labeled with the chain, so a share's `@@abc\Music\Artist` doesn't take three levels before
 * anything shows. With a `scope`, their keys are unique to it
 */
const nodesOf = <T>(
  root: Trie<T>,
  sizeOf: (item: T) => number,
  lockedOf?: (item: T) => boolean,
  scope?: string
): FolderNode<T>[] => {
  const build = (trie: Trie<T>, label: string): FolderNode<T> => {
    if (!trie.items.length && trie.children.size === 1) {
      const [only] = trie.children.values();
      return build(only, label + only.separator + only.segment);
    }
    const children = [...trie.children.values()].map((child) => build(child, child.segment));
    const key = scope === undefined ? trie.path : scopedKey(scope, trie.path);
    return folderNode({ key, path: trie.path, label }, trie.items, children, sizeOf, lockedOf);
  };
  return [...root.children.values()].map((top) => build(top, top.segment));
};

/**
 * The folders the items are in, as a tree, folders and items in the order they first appear.
 * `pathOf` gives an item's full path, its folder being everything before the last separator. A
 * folder is locked when every item under it is, by `lockedOf`. With a `scope`, the folders' keys
 * are unique to it, for a list that holds several trees. Items with no folder, bare file names,
 * are in a node of their own with an empty path and label, the last; `treeAt` puts them in the
 * line above the tree instead
 */
export const folderTree = <T>(
  items: T[],
  pathOf: (item: T) => string,
  sizeOf: (item: T) => number,
  lockedOf?: (item: T) => boolean,
  scope?: string
): FolderNode<T>[] => {
  const root = trieOf(items, pathOf);
  const folders = nodesOf(root, sizeOf, lockedOf, scope);
  if (!root.items.length) return folders;
  const key = scope === undefined ? "" : scopedKey(scope, "");
  return [...folders, folderNode({ key, path: "", label: "" }, root.items, [], sizeOf, lockedOf)];
};

/**
 * A line for the top of a tree, such as a user's, with the items' folders under it as
 * `folderTree` makes them and the items that have no folder directly in it
 */
export const treeAt = <T>(
  at: { key: string; path: string; label: string },
  items: T[],
  pathOf: (item: T) => string,
  sizeOf: (item: T) => number,
  lockedOf?: (item: T) => boolean,
  scope?: string
): FolderNode<T> => {
  const root = trieOf(items, pathOf);
  return folderNode(at, root.items, nodesOf(root, sizeOf, lockedOf, scope), sizeOf, lockedOf);
};

export type RemoteDirectory = Schemas["Directory"] & { locked?: boolean };

/** A file in someone's share, `locked` when they share it only with some */
export type RemoteFile = Schemas["File"] & { locked?: boolean };

/** A browse listing as a tree, and each of its folders by path */
export type Listing = {
  roots: FolderNode<RemoteFile>[];
  byPath: Map<string, FolderNode<RemoteFile>>;
};

/** The separator a share uses: the first folder name with either decides, `\` when none has one */
export const detectSeparator = (directories: { name: string }[]): Separator => {
  for (const { name } of directories) {
    if (name.includes("\\")) return "\\";
    if (name.includes("/")) return "/";
  }
  return "\\";
};

/**
 * The folders of a browse as a tree, in the order the user listed them, each folder a level of
 * its own. A folder whose parent isn't listed is a root: levels the listing skips are not made up.
 * A folder's files, each with its full path, are made and sorted by name the first time they are
 * read, so a share of many folders costs nothing for the folders never opened
 */
export const listingTree = (directories: RemoteDirectory[], separator: Separator): Listing => {
  const byPath: Listing["byPath"] = new Map();
  if (!directories.length || !directories[0].name) return { roots: [], byPath };
  for (const d of directories) {
    if (byPath.has(d.name)) continue;
    let files: RemoteFile[] | null = null;
    const locked = !!d.locked;
    byPath.set(d.name, {
      key: d.name,
      path: d.name,
      label: getFileName(d.name),
      get items() {
        files ??= d.files
          .map((f) => ({ ...f, filename: d.name + separator + f.filename, locked }))
          .sort((a, b) => getFileName(a.filename).localeCompare(getFileName(b.filename)));
        return files;
      },
      children: [],
      count: d.files.length,
      bytes: d.files.reduce((sum, f) => sum + f.size, 0),
      folders: 0,
      unlocked: locked ? 0 : d.files.length,
      locked
    });
  }
  const roots: Listing["roots"] = [];
  for (const node of byPath.values()) {
    const parent = byPath.get(parentOf(node.path, separator));
    if (parent && parent !== node) parent.children.push(node);
    else roots.push(node);
  }
  // totals from the leaves up, without recursion so a deep share can't overflow the stack
  const order: Listing["roots"] = [];
  const stack = [...roots];
  while (stack.length) {
    const node = stack.pop()!;
    order.push(node);
    stack.push(...node.children);
  }
  for (const node of order.toReversed()) {
    for (const child of node.children) {
      node.count += child.count;
      node.bytes += child.bytes;
      node.folders += 1 + child.folders;
      node.unlocked += child.unlocked;
    }
  }
  return { roots, byPath };
};

/** The paths of the folders above `path` that are in the listing, nearest last */
export const ancestors = (path: string, listing: Listing, separator: Separator): string[] => {
  const found: string[] = [];
  for (let at = parentOf(path, separator); at; at = parentOf(at, separator)) {
    if (listing.byPath.has(at)) found.unshift(at);
  }
  return found;
};

/**
 * How many of the selected items are under each folder, by the folder's key: each selected id, a
 * path or a `scopedKey`, counts once for every folder its path is in. Costs the selection's size
 * times its depth, so a folder line's tick reads its count rather than walking the items below it
 */
export const tickedCounts = (selected: Iterable<string>) => {
  const counts = new Map<string, number>();
  for (const id of selected) {
    // the scope may hold either separator, so the walk stays inside the path
    const cut = id.indexOf("\u0000") + 1;
    const scope = id.slice(0, cut);
    const path = id.slice(cut);
    const separator = separatorOf(path);
    for (let at = parentOf(path, separator); at; at = parentOf(at, separator)) {
      counts.set(scope + at, (counts.get(scope + at) ?? 0) + 1);
    }
  }
  return counts;
};

/** The keys of every folder below `node`, at every depth, each its own key or what `keyOf` gives */
export const foldersUnder = <T>(
  node: FolderNode<T>,
  keyOf: (node: FolderNode<T>) => string = (n) => n.key
): string[] => node.children.flatMap((child) => [keyOf(child), ...foldersUnder(child, keyOf)]);

/** One line of a tree laid out flat: a folder, or an item under it */
export type TreeLine<T> =
  | { kind: "folder"; key: string; node: FolderNode<T>; depth: number; siblings: string[] }
  | { kind: "item"; key: string; item: T; depth: number; folder: FolderNode<T> };

/** What a filter keeps of a tree: folders that match, and items */
export type TreeFilter<T> = {
  folder: (node: FolderNode<T>) => boolean;
  item: (item: T) => boolean;
};

/**
 * The tree as lines, depth first: each folder, then while it is open its items and the folders
 * under it, by their keys. `siblings` lists the keys of the folders beside a folder, itself
 * included, which an Alt-click opens or closes together.
 *
 * With a `filter` the open folders are ignored: a folder shows, open, when it or a folder under
 * it matches, so the way to every match is in view, and of its items those the filter keeps
 */
export const treeLines = <T>(
  roots: FolderNode<T>[],
  isOpen: (key: string) => boolean,
  keyOf: (item: T) => string,
  filter?: TreeFilter<T> | null
): TreeLine<T>[] => {
  const lines: TreeLine<T>[] = [];
  const hit = new Map<FolderNode<T>, boolean>();
  const shown = (node: FolderNode<T>): boolean => {
    if (!filter) return true;
    const known = hit.get(node);
    if (known !== undefined) return known;
    let found = filter.folder(node);
    for (const child of node.children) found = shown(child) || found;
    hit.set(node, found);
    return found;
  };
  const walk = (nodes: FolderNode<T>[], depth: number) => {
    const kept = filter ? nodes.filter(shown) : nodes;
    const siblings = kept.map((n) => n.key);
    for (const node of kept) {
      lines.push({ kind: "folder", key: `folder:${node.key}`, node, depth, siblings });
      if (!filter && !isOpen(node.key)) continue;
      for (const item of node.items) {
        if (filter && !filter.item(item)) continue;
        lines.push({ kind: "item", key: keyOf(item), item, depth: depth + 1, folder: node });
      }
      walk(node.children, depth + 1);
    }
  };
  walk(roots, 0);
  return lines;
};
