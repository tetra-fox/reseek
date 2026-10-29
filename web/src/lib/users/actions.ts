import { api, byUser } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { chatHref, href } from "#lib/nav.js";
import { call } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";

/** Where the menu's links go for `username`, with the folder it was shown in when there is one */
export const links = (username: string, folder?: string) => ({
  browse: href(`/browse?user=${encodeURIComponent(username)}`),
  message: chatHref(username),
  search: href(
    `/searches?user=${encodeURIComponent(username)}${folder ? `&text=${encodeURIComponent(folder)}` : ""}`
  )
});

/** Pins `username` to the top of the directory, or unpins them when `on` is false */
export const pin = (username: string, on: boolean) =>
  call(() =>
    on
      ? api.PUT("/api/v0/users/{username}/pin", { params: byUser(username) })
      : api.DELETE("/api/v0/users/{username}/pin", { params: byUser(username) })
  );

/** The names of the user-defined upload groups in the options in effect */
export const userGroups = (): string[] =>
  Object.keys(fields(fields(fields(fields(app.options).transfers).groups).user_defined)).toSorted();

/** Puts a user in a user-defined group, or with null takes them out of it */
export const assignGroup = (username: string, group: string | null) =>
  call(() =>
    api.PUT("/api/v0/users/{username}/group", { params: byUser(username), body: { group } })
  );
