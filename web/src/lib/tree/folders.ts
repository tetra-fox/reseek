import { getDirectoryName } from "#lib/format.js";

/**
 * A folder of a tree built from paths: the items directly in it, the folders under it, and what
 * it holds counting everything below. A chain of folders that hold nothing but the next one is
 * one node, labeled with the whole chain, so a share's `@@abc\Music\Artist` doesn't take three
 * levels before anything shows
 */
export type FolderNode<T> = {
  /** the full path, which also keys the node */
  path: string;
  /** the path from the parent node down to this one */
  label: string;
  items: T[];
  children: FolderNode<T>[];
  /** items here and in every folder below */
  count: number;
  /** their sizes added up */
  bytes: number;
};

type Trie<T> = { path: string; segment: string; items: T[]; children: Map<string, Trie<T>> };

/** The separator a path uses: `/` when it has one, else `\` */
const separatorOf = (path: string) => (path.includes("/") ? "/" : "\\");

/**
 * The folders the items are in, as a tree, folders and items in the order they first appear.
 * `pathOf` gives an item's full path, its folder being everything before the last separator
 */
export const folderTree = <T>(
  items: T[],
  pathOf: (item: T) => string,
  sizeOf: (item: T) => number
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
    return {
      path: trie.path,
      label,
      items: trie.items,
      children,
      count: trie.items.length + children.reduce((n, c) => n + c.count, 0),
      bytes:
        trie.items.reduce((n, item) => n + sizeOf(item), 0) +
        children.reduce((n, c) => n + c.bytes, 0)
    };
  };
  return [...root.children.values()].map((top) => build(top, top.segment));
};

/** One line of a tree laid out flat: a folder, or an item under it */
export type TreeLine<T> =
  | { kind: "folder"; key: string; node: FolderNode<T>; depth: number; siblings: string[] }
  | { kind: "item"; key: string; item: T; depth: number; folder: FolderNode<T> };

/**
 * The tree as lines, depth first: each folder, then while it is open its items and the folders
 * under it. `siblings` lists the paths of the folders beside a folder, itself included, which
 * Alt-click opens or closes together
 */
export const treeLines = <T>(
  roots: FolderNode<T>[],
  isOpen: (path: string) => boolean,
  keyOf: (item: T) => string
): TreeLine<T>[] => {
  const lines: TreeLine<T>[] = [];
  const walk = (nodes: FolderNode<T>[], depth: number) => {
    const siblings = nodes.map((n) => n.path);
    for (const node of nodes) {
      lines.push({ kind: "folder", key: `folder:${node.path}`, node, depth, siblings });
      if (!isOpen(node.path)) continue;
      for (const item of node.items) {
        lines.push({ kind: "item", key: keyOf(item), item, depth: depth + 1, folder: node });
      }
      walk(node.children, depth + 1);
    }
  };
  walk(roots, 0);
  return lines;
};
