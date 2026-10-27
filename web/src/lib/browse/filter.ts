import type { Schemas } from "#lib/api/client.js";
import { fileMatches, parseFilters } from "#lib/search/filters.js";

import { leaf, type Separator, type TreeNode } from "./tree";

/** What a browse filter keeps: files by path, kind and size, and the folders that hold them */
export type BrowseFilter = {
  file: (file: Schemas["File"], path: string) => boolean;
  folder: (node: TreeNode) => boolean;
};

/**
 * Reads the browse filter: the search results' filter language over full paths, plus `ext:flac`
 * to keep files of some extensions. A folder is kept when its name has every word, or when a
 * file directly in it is kept. Null for an empty filter
 */
export const browseFilter = (text: string, separator: Separator): BrowseFilter | null => {
  const tokens = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return null;
  const extensions = tokens
    .filter((t) => t.startsWith("ext:"))
    .map((t) => t.slice(4).replace(/^\./, ""))
    .filter(Boolean);
  const filters = parseFilters(text);
  const file = (f: Schemas["File"], path: string) => {
    const extension = (f.extension || path.split(".").pop() || "").toLowerCase();
    return (
      (!extensions.length || extensions.includes(extension)) &&
      fileMatches(filters, { ...f, filename: path })
    );
  };
  const words = filters.include;
  return {
    file,
    folder: (node) => {
      const name = leaf(node.name).toLowerCase();
      if (words.length && words.every((w) => name.includes(w))) return true;
      return node.files.some((f) => file(f, node.name + separator + f.filename));
    }
  };
};
