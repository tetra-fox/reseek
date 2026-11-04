import { formatBytes, plural } from "#lib/format.js";
import type { FolderNode } from "#lib/tree/folders.js";

/** What a folder holds, as its line says it: "3 files, 1 folder · 24.0 MB" */
export const folderCaption = <T>(node: FolderNode<T>) => {
  const parts = [
    node.count || !node.folders ? plural(node.count, "file", "files") : "",
    node.folders ? plural(node.folders, "folder", "folders") : ""
  ].filter(Boolean);
  return `${parts.join(", ")}${node.bytes ? ` · ${formatBytes(node.bytes)}` : ""}`;
};
