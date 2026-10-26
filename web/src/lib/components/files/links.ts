import { getDirectoryName } from "#lib/format.js";
import { href } from "#lib/nav.js";

/** The folder a remote file is in, and the Browse links to it: the tree there, or every file under it */
export const remoteFolder = (username: string, path: string) => {
  const folder = getDirectoryName(path);
  const query = `user=${encodeURIComponent(username)}&path=${encodeURIComponent(folder)}`;
  return { folder, browse: href(`/browse?${query}`), flat: href(`/browse?${query}&flat=1`) };
};

/** Whether the browser can play a file in the page, judged by its name */
export const isAudio = (path: string) => /\.(mp3|flac|ogg|oga|opus|m4a|aac|wav|webm)$/i.test(path);
