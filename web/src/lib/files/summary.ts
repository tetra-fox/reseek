import type { Schemas } from "#lib/api/client.js";
import { formatBytes, plural } from "#lib/format.js";
import type { FolderNode } from "#lib/tree/folders.js";

type File = Schemas["File"];

/** The quality of several files in a word: all VBR, one bit rate, or "Mixed"; "" when unknown */
export const attributesOf = (files: File[]): string => {
  if (!files.length) return "";
  const vbr = files.filter((f) => f.isVariableBitRate).length;
  if (vbr === files.length) return "VBR";
  if (vbr > 0) return "Mixed";
  const rates = new Set(files.map((f) => f.bitRate).filter((r) => r));
  if (rates.size === 1) return `${[...rates][0]} kbps`;
  return rates.size > 1 ? "Mixed" : "";
};

/** The lengths of several files added up, in seconds */
export const lengthOf = (files: File[]) => files.reduce((sum, f) => sum + (f.length ?? 0), 0);

/** The sizes of several files added up, in bytes */
export const sizeOf = (files: File[]) => files.reduce((sum, f) => sum + f.size, 0);

/** What a folder holds, as its line says it: "3 files, 1 folder · 24.0 MB" */
export const folderCaption = <T>(node: FolderNode<T>) => {
  const parts = [
    node.count || !node.folders ? plural(node.count, "file", "files") : "",
    node.folders ? plural(node.folders, "folder", "folders") : ""
  ].filter(Boolean);
  return `${parts.join(", ")}${node.bytes ? ` · ${formatBytes(node.bytes)}` : ""}`;
};
