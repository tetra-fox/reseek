import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
import Rows3Icon from "@lucide/svelte/icons/rows-3";

/** How a view of files lays them out: in their folders, or one row per file */
export type Layout = "tree" | "list";

/** The layouts as every view's toggle offers them */
export const LAYOUTS = [
  { id: "tree", label: "Tree", icon: FolderTreeIcon },
  { id: "list", label: "List", icon: Rows3Icon }
] as const;

/** A saved layout, or `otherwise` for anything that is not one */
export const readLayout = (saved: unknown, otherwise: Layout): Layout =>
  saved === "tree" || saved === "list" ? saved : otherwise;
