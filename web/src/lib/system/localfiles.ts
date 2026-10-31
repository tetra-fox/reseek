import type { Track } from "#lib/analysis/playback.svelte.js";
import { api, root as base } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";
import { session } from "#lib/session.svelte.js";
import { encodePath } from "#lib/transfers/content.js";

/** The directories the Files screen shows */
export type Root = "downloads" | "incomplete";

/** A path under a root from its parts, `/`-separated */
export const joinPath = (parts: string[]) => parts.filter(Boolean).join("/");

/**
 * The URL that loads a file under a root, shown in the browser or saved, as it is stored or as
 * the WAV the daemon makes of it. Without a session cookie, as on a trusted network, the link
 * carries a ten-minute token for that one file
 */
export const fileUrl = async (
  root: Root,
  path: string,
  disposition: "inline" | "attachment",
  as: "content" | "wav" = "content"
): Promise<Outcome<string>> => {
  const encoded = encodePath(path);
  const url = `${base()}/api/v0/files/${root}/files/${encoded}/${as}`;
  const query = new URLSearchParams({ disposition });
  if (!session.canSignOut) {
    const token = await call(() =>
      api.POST("/api/v0/session/file-token", { body: { path: `${root}/${encoded}` } })
    );
    if (!token.ok) return token;
    query.set("t", token.data.token);
  }
  return { ok: true, data: `${url}?${query}` };
};

/** The URL of a zip of a folder under a root */
export const archiveUrl = (root: Root, path: string) =>
  `${base()}/api/v0/files/${root}/directories/${encodePath(path)}/archive`;

/** A file under a root as the analysis popup plays it */
export const fileTrack = (root: Root, path: string): Track => ({
  filename: path,
  url: (as) => fileUrl(root, path, "inline", as)
});
