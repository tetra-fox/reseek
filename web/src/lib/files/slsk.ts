import { getDirectoryName } from "#lib/format.js";
import { sharesHref } from "#lib/nav.js";

/**
 * A `slsk://` link: a user's shared file or folder. The path's backslashes are written as
 * slashes, each part percent-encoded, and a folder ends with a slash
 */
export type SlskLink = { username: string; path: string; folder: boolean };

const decode = (text: string) => {
  try {
    return decodeURIComponent(text);
  } catch {
    // a stray % in the link: its text is taken as it is
    return text;
  }
};

/** The `slsk://` link to a shared file, or with `folder` to a shared folder */
export const slskLink = (username: string, path: string, folder = false): string => {
  const parts = path.split(/[\\/]/).filter(Boolean).map(encodeURIComponent);
  return `slsk://${encodeURIComponent(username)}/${parts.join("/")}${folder ? "/" : ""}`;
};

/** A `slsk://` link read back, null for text that is not one */
export const parseSlsk = (text: string): SlskLink | null => {
  const match = /^slsk:\/\/([^/\s]+)\/(\S*)$/i.exec(text.trim());
  if (!match) return null;
  const [, user, rest] = match;
  const parts = rest.split("/").filter(Boolean).map(decode);
  if (!parts.length) return null;
  return { username: decode(user), path: parts.join("\\"), folder: rest.endsWith("/") };
};

/** Where a link opens: the user's Shares tab at its folder, or at the folder holding its file */
export const slskBrowse = (link: SlskLink): string =>
  sharesHref(link.username, link.folder ? link.path : getDirectoryName(link.path));
