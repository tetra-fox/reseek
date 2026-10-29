import type { Schemas } from "#lib/api/client.js";
import { fileMatches, parseFilters } from "#lib/files/filters.js";
import type { TreeFilter } from "#lib/tree/folders.js";

/**
 * Reads the browse filter: the search results' filter language over full paths, `ext:flac`
 * being its `type:`. A folder is kept when its name has every word, or when a file directly in
 * it is kept. Null for an empty filter
 */
export const browseFilter = (text: string): TreeFilter<Schemas["File"]> | null => {
  if (!text.trim()) return null;
  const filters = parseFilters(text);
  const item = (f: Schemas["File"]) => fileMatches(filters, f);
  const words = filters.include;
  return {
    item,
    folder: (node) => {
      const name = node.label.toLowerCase();
      if (words.length && words.every((w) => name.includes(w))) return true;
      return node.items.some(item);
    }
  };
};
