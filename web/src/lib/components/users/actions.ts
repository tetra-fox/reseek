import { api, byUser } from "#lib/api/client.js";
import { href } from "#lib/nav.js";
import { call } from "#lib/request.js";

/** Where the menu's links go for `username`, with the folder it was shown in when there is one */
export const links = (username: string, folder?: string) => ({
  browse: href(`/browse?user=${encodeURIComponent(username)}`),
  message: href(`/chat?user=${encodeURIComponent(username)}`),
  search: href(
    `/searches?user=${encodeURIComponent(username)}${folder ? `&text=${encodeURIComponent(folder)}` : ""}`
  )
});

export const pin = (username: string, on: boolean) =>
  call(() =>
    on
      ? api.PUT("/api/v0/users/{username}/pin", { params: byUser(username) })
      : api.DELETE("/api/v0/users/{username}/pin", { params: byUser(username) })
  );
