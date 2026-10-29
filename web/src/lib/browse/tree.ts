import type { Schemas } from "#lib/api/client.js";
import type { Separator } from "#lib/tree/folders.js";

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
    const grouped = groups.get(group);
    if (grouped) grouped.push(file);
    else groups.set(group, [file]);
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
