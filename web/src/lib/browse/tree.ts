import type { Schemas } from "#lib/api/client.js";

export type RemoteDirectory = Schemas["Directory"] & { locked?: boolean };
export type Separator = "\\" | "/";

/** A shared folder with what is under it, counted once when the tree is built */
export type TreeNode = {
  name: string;
  /** its files, each `filename` the bare name */
  files: Schemas["File"][];
  locked: boolean;
  children: TreeNode[];
  totalDirectoryCount: number;
  totalFileCount: number;
  totalBytes: number;
};

export type Tree = { roots: TreeNode[]; byName: Map<string, TreeNode> };

/** The separator a share uses: the first folder name with either decides, `\` when none has one */
export const detectSeparator = (directories: { name: string }[]): Separator => {
  for (const { name } of directories) {
    if (name.includes("\\")) return "\\";
    if (name.includes("/")) return "/";
  }
  return "\\";
};

/** The last part of a folder's path */
export const leaf = (name: string) => name.split(/[/\\]/).pop() ?? name;

/** The path a folder is in, "" for one at the top */
export const parentOf = (name: string, separator: Separator) => {
  const cut = name.lastIndexOf(separator);
  return cut === -1 ? "" : name.slice(0, cut);
};

/**
 * The folders of a browse as a tree, in the order the user listed them. A folder whose parent
 * isn't listed is a root of its own: levels the listing skips are not made up
 */
export const buildTree = (directories: RemoteDirectory[], separator: Separator): Tree => {
  const byName = new Map<string, TreeNode>();
  if (!directories.length || !directories[0].name) return { roots: [], byName };
  for (const d of directories) {
    if (byName.has(d.name)) continue;
    byName.set(d.name, {
      name: d.name,
      files: d.files,
      locked: !!d.locked,
      children: [],
      totalDirectoryCount: 0,
      totalFileCount: 0,
      totalBytes: 0
    });
  }
  const roots: TreeNode[] = [];
  for (const node of byName.values()) {
    const parent = byName.get(parentOf(node.name, separator));
    if (parent && parent !== node) parent.children.push(node);
    else roots.push(node);
  }
  // totals from the leaves up, without recursion so a deep share can't overflow the stack
  const order: TreeNode[] = [];
  const stack = [...roots];
  while (stack.length) {
    const node = stack.pop()!;
    order.push(node);
    stack.push(...node.children);
  }
  for (const node of order.toReversed()) {
    node.totalFileCount = node.files.length;
    node.totalBytes = node.files.reduce((sum, f) => sum + f.size, 0);
    node.totalDirectoryCount = node.children.length;
    for (const child of node.children) {
      node.totalFileCount += child.totalFileCount;
      node.totalBytes += child.totalBytes;
      node.totalDirectoryCount += child.totalDirectoryCount;
    }
  }
  return { roots, byName };
};

export type TreeRow = { node: TreeNode; depth: number };

/**
 * The rows the tree shows: depth first, a folder's children only when it is expanded. With a
 * `matches` test the expansion is ignored and a folder shows when it or anything under it
 * matches, so the way to every match stays visible
 */
export const flatten = (
  roots: TreeNode[],
  expanded: Set<string>,
  matches?: (node: TreeNode) => boolean
): TreeRow[] => {
  const rows: TreeRow[] = [];
  if (!matches) {
    const walk = (nodes: TreeNode[], depth: number) => {
      for (const node of nodes) {
        rows.push({ node, depth });
        if (expanded.has(node.name)) walk(node.children, depth + 1);
      }
    };
    walk(roots, 0);
    return rows;
  }
  const hit = new Map<TreeNode, boolean>();
  const any = (node: TreeNode): boolean => {
    const known = hit.get(node);
    if (known !== undefined) return known;
    let found = matches(node);
    for (const child of node.children) found = any(child) || found;
    hit.set(node, found);
    return found;
  };
  const walk = (nodes: TreeNode[], depth: number) => {
    for (const node of nodes) {
      if (!any(node)) continue;
      rows.push({ node, depth });
      walk(node.children, depth + 1);
    }
  };
  walk(roots, 0);
  return rows;
};

/** A folder's own files and those of every folder under it, each with its full remote path */
export const filesUnder = (node: TreeNode, separator: Separator): Schemas["File"][] => {
  const files: Schemas["File"][] = [];
  const stack = [node];
  while (stack.length) {
    const at = stack.pop()!;
    for (const f of at.files) files.push({ ...f, filename: at.name + separator + f.filename });
    stack.push(...at.children.toReversed());
  }
  return files;
};

/** The ancestors of a folder that are in the tree, nearest last */
export const ancestors = (name: string, tree: Tree, separator: Separator): string[] => {
  const found: string[] = [];
  for (let at = parentOf(name, separator); at; at = parentOf(at, separator)) {
    if (tree.byName.has(at)) found.unshift(at);
  }
  return found;
};

export type DownloadGroup = {
  files: Schemas["File"][];
  /** where the files go under the downloads directory, null for where the pattern puts them */
  destination: string | null;
};

/**
 * Selected files of the folder `name` split for downloading: those directly in it go where the
 * download pattern puts them, and those in each of its subfolders go to that subfolder's path
 * from the folder's parent, with `/` between, so `Music\Album\Disc 1` lands in `Album/Disc 1`
 */
export const downloadGroups = (
  name: string,
  files: Schemas["File"][],
  separator: Separator
): DownloadGroup[] => {
  const parent = name.split(separator).slice(0, -1).join(separator);
  const prefix = name + separator;
  const groups = new Map<string, Schemas["File"][]>();
  for (const file of files) {
    const relative = file.filename.startsWith(prefix)
      ? file.filename.slice(prefix.length)
      : file.filename;
    const parts = relative.split(separator).filter(Boolean);
    const group = parts.length > 1 ? parts[0] : "";
    groups.set(group, [...(groups.get(group) ?? []), file]);
  }
  return [...groups.entries()]
    .toSorted(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : 0))
    .map(([group, grouped]) => ({
      files: grouped,
      destination:
        group === ""
          ? null
          : (prefix + group)
              .slice(parent.length > 0 ? parent.length + 1 : 0)
              .split(separator)
              .join("/")
    }));
};
