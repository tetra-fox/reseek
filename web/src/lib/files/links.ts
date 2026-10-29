import { getDirectoryName } from "#lib/format.js";
import { href } from "#lib/nav.js";

/** The Browse link to a user's remote folder, which opens their tree there */
export const folderLinks = (username: string, folder: string) => {
  const query = `user=${encodeURIComponent(username)}&path=${encodeURIComponent(folder)}`;
  return { folder, browse: href(`/browse?${query}`) };
};

/** The folder a remote file is in, and the Browse links to it */
export const remoteFolder = (username: string, path: string) =>
  folderLinks(username, getDirectoryName(path));
