import { api, byUser, root } from "#lib/api/client.js";
import { call, type Outcome } from "#lib/request.js";
import { session } from "#lib/session.svelte.js";

import { isSucceeded, type Transfer } from "./states";

/**
 * The URL that loads a finished download's file: shown in the browser (`inline`) or saved
 * (`attachment`), as it is stored or as the WAV the daemon makes of it. With a session cookie the
 * browser sends it along; without one, as on a trusted network, the link carries a ten-minute
 * token for that one file
 */
export const contentUrl = async (
  t: Pick<Transfer, "username" | "id">,
  disposition: "inline" | "attachment",
  as: "content" | "wav" = "content"
): Promise<Outcome<string>> => {
  const user = byUser(t.username);
  const path = `${root()}/api/v0/transfers/downloads/${encodeURIComponent(user.path.username)}/${t.id}/${as}`;
  const query = new URLSearchParams({ ...user.query, disposition });
  if (!session.canSignOut) {
    const token = await call(() =>
      api.POST("/api/v0/session/file-token", { body: { transferId: t.id } })
    );
    if (!token.ok) return token;
    query.set("t", token.data.token);
  }
  return { ok: true, data: `${path}?${query}` };
};

/** A path relative to a root, in the URL-safe base64 the files endpoints take */
export const encodePath = (relative: string) =>
  btoa(String.fromCodePoint(...new TextEncoder().encode(relative)))
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/, "");

/**
 * The local folder of a transfer group under the downloads directory, from its first finished
 * download's path; null when none finished or it lies outside
 */
export const localFolder = (transfers: Transfer[], downloads: string | null): string | null => {
  const done = transfers.find((t) => isSucceeded(t.state) && t.localPath);
  if (!done?.localPath || !downloads) return null;
  const sep = done.localPath.includes("\\") && !done.localPath.includes("/") ? "\\" : "/";
  const base = downloads.replace(/[\\/]+$/, "") + sep;
  if (!done.localPath.startsWith(base)) return null;
  const relative = done.localPath.slice(base.length);
  const cut = relative.lastIndexOf(sep);
  return cut === -1 ? "" : relative.slice(0, cut);
};
