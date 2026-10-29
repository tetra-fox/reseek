import type { Schemas } from "#lib/api/client.js";
import { getDirectoryName, getFileName } from "#lib/format.js";

/**
 * A folder of a tree: the items directly in it, the folders under it, and what it holds counting
 * everything below
 */
export type FolderNode<T> = {
  /** the full path, which also keys the node */
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

type Trie<T> = { path: string; segment: string; items: T[]; children: Map<string, Trie<T>> };

/**
 * The folders the items are in, as a tree, folders and items in the order they first appear.
 * `pathOf` gives an item's full path, its folder being everything before the last separator. A
 * chain of folders that hold nothing but the next one is one node, labeled with the whole chain,
 * so a share's `@@abc\Music\Artist` doesn't take three levels before anything shows. A folder is
 * locked when every item under it is, by `lockedOf`
 */
export const folderTree = <T>(
  items: T[],
  pathOf: (item: T) => string,
  sizeOf: (item: T) => number,
  lockedOf?: (item: T) => boolean
): FolderNode<T>[] => {
  const root: Trie<T> = { path: "", segment: "", items: [], children: new Map() };
  for (const item of items) {
    const path = pathOf(item);
    const folder = getDirectoryName(path);
    const separator = separatorOf(path);
    let at = root;
    let walked = "";
    for (const segment of folder === path ? [folder] : folder.split(separator)) {
      walked = walked ? walked + separator + segment : segment;
      let next = at.children.get(segment);
      if (!next) {
        next = { path: walked, segment, items: [], children: new Map() };
        at.children.set(segment, next);
      }
      at = next;
    }
    at.items.push(item);
  }
  const build = (trie: Trie<T>, label: string): FolderNode<T> => {
    // a folder holding only one folder is the same node as that folder
    if (!trie.items.length && trie.children.size === 1) {
      const [only] = trie.children.values();
      return build(only, label + separatorOf(only.path) + only.segment);
    }
    const children = [...trie.children.values()].map((child) => build(child, child.segment));
    const count = trie.items.length + children.reduce((n, c) => n + c.count, 0);
    return {
      path: trie.path,
      label,
      items: trie.items,
      children,
      count,
      bytes:
        trie.items.reduce((n, item) => n + sizeOf(item), 0) +
        children.reduce((n, c) => n + c.bytes, 0),
      folders: children.reduce((n, c) => n + 1 + c.folders, 0),
      locked: !!lockedOf && trie.items.every(lockedOf) && children.every((c) => c.locked)
    };
  };
  return [...root.children.values()].map((top) => build(top, top.segment));
};

export type RemoteDirectory = Schemas["Directory"] & { locked?: boolean };

/** A browse listing as a tree, and each of its folders by path */
export type Listing = {
  roots: FolderNode<Schemas["File"]>[];
  byPath: Map<string, FolderNode<Schemas["File"]>>;
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
    let files: Schemas["File"][] | null = null;
    byPath.set(d.name, {
      path: d.name,
      label: getFileName(d.name),
      get items() {
        files ??= d.files
          .map((f) => ({ ...f, filename: d.name + separator + f.filename }))
          .sort((a, b) => getFileName(a.filename).localeCompare(getFileName(b.filename)));
        return files;
      },
      children: [],
      count: d.files.length,
      bytes: d.files.reduce((sum, f) => sum + f.size, 0),
      folders: 0,
      locked: !!d.locked
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
 * under it. `siblings` lists the paths of the folders beside a folder, itself included, which
 * Alt-click opens or closes together.
 *
 * With a `filter` the open folders are ignored: a folder shows, open, when it or a folder under
 * it matches, so the way to every match is in view, and of its items those the filter keeps
 */
export const treeLines = <T>(
  roots: FolderNode<T>[],
  isOpen: (path: string) => boolean,
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
    const siblings = kept.map((n) => n.path);
    for (const node of kept) {
      lines.push({ kind: "folder", key: `folder:${node.path}`, node, depth, siblings });
      if (!filter && !isOpen(node.path)) continue;
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
