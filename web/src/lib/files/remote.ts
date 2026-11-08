import { api, byUser, type Schemas } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";
import { separatorOf } from "#lib/tree/folders.js";

/**
 * The longest length a peer is believed about, in seconds. Some clients send -1 for a length
 * they don't know, which reads as 4,294,967,295 seconds
 */
const LONGEST_S = 7 * 86_400;

/** A remote file as the screens keep it, its length unknown when it says more than a week */
export const withKnownLength = <F extends { length?: number | null }>(file: F): F =>
  file.length && file.length > LONGEST_S ? { ...file, length: null } : file;

/** A remote folder with each file's `filename` its full path */
export type RemoteFolder = { name: string; files: Schemas["File"][] };

/**
 * Asks `username` for the contents of one of their folders. The folder asked for comes first;
 * some clients send its subfolders after it. File names come back bare and are made full paths
 * with the separator the folder's own path uses
 */
export const fetchFolder = async (
  username: string,
  directory: string
): Promise<Outcome<RemoteFolder[]>> => {
  const outcome = await call(() =>
    api.POST("/api/v0/users/{username}/directory", {
      params: byUser(username),
      body: { directory }
    })
  );
  if (!outcome.ok) return outcome;
  const folders = outcome.data.map((d, i) => {
    // the first is the folder asked for, whatever spelling of its name the peer sent back
    const name = i === 0 ? directory : d.name;
    const sep = separatorOf(name);
    return {
      name,
      files: d.files.map((f) => withKnownLength({ ...f, filename: name + sep + f.filename }))
    };
  });
  if (!folders.length) {
    return {
      ok: false,
      error: "The user returned no folder",
      status: 200
    };
  }
  return { ok: true, data: folders };
};
