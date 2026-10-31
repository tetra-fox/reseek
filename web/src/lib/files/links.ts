import { getDirectoryName } from "#lib/format.js";
import { sharesHref } from "#lib/nav.js";

/** The link to a user's remote folder, which opens their Shares tab there */
export const folderLinks = (username: string, folder: string) => ({
  folder,
  browse: sharesHref(username, folder)
});

/** The folder a remote file is in, and the link to it */
export const remoteFolder = (username: string, path: string) =>
  folderLinks(username, getDirectoryName(path));
