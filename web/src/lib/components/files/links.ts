import { getDirectoryName } from "#lib/format.js";
import { href } from "#lib/nav.js";

/** The Browse links to a user's remote folder: the tree there, or every file under it */
export const folderLinks = (username: string, folder: string) => {
  const query = `user=${encodeURIComponent(username)}&path=${encodeURIComponent(folder)}`;
  return { folder, browse: href(`/browse?${query}`), flat: href(`/browse?${query}&flat=1`) };
};

/** The folder a remote file is in, and the Browse links to it */
export const remoteFolder = (username: string, path: string) =>
  folderLinks(username, getDirectoryName(path));

/** What a browser is asked to play an audio file as, by its extension */
const AUDIO: Record<string, string> = {
  mp3: "audio/mpeg",
  flac: "audio/flac",
  ogg: "audio/ogg",
  oga: "audio/ogg",
  opus: "audio/ogg; codecs=opus",
  m4a: "audio/mp4",
  aac: "audio/aac",
  wav: "audio/wav",
  webm: "audio/webm"
};

/** The type of an audio file by its name, "" for anything else */
export const audioType = (path: string) => AUDIO[path.split(".").pop()?.toLowerCase() ?? ""] ?? "";

/** Whether the browser can play a file in the page, judged by its name */
export const isAudio = (path: string) => audioType(path) !== "";
