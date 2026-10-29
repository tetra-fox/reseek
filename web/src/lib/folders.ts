import { api, byUser, type Schemas } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";
import { separatorOf } from "#lib/search/results.js";

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
    return { name, files: d.files.map((f) => ({ ...f, filename: name + sep + f.filename })) };
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
