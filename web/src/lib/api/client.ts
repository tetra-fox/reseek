import createClient from "openapi-fetch";

import type { components, paths } from "./schema";

export type Schemas = components["schemas"];

/**
 * Where the daemon is: the directory the page was loaded from, so the UI works under whatever
 * URL base the daemon is served at. The API paths carry their own `/api/v0` prefix
 */
export const root = (): string => new URL(".", document.baseURI).href.replace(/\/$/, "");

export const api = createClient<paths>({
  baseUrl: root(),
  credentials: "same-origin",
  // looked up on every call rather than captured once, so tests can stand in for the network
  fetch: (request) => globalThis.fetch(request)
});

/** The routes that answer 401 as part of logging in, which never end the session */
const SESSION_ROUTES = ["/api/v0/session"];

let unauthorized: () => void = () => {};

/** What to do when a request outside the session routes is answered 401: the session is gone */
export const onUnauthorized = (handler: () => void) => {
  unauthorized = handler;
};

/** Says the session is gone, for answers that don't go through `api`, like the event stream */
export const sessionGone = () => unauthorized();

api.use({
  onResponse({ request, response }) {
    if (response.status !== 401) return;
    const path = new URL(request.url).pathname;
    if (!SESSION_ROUTES.some((route) => path.endsWith(route))) sessionGone();
  }
});

/** A username in a path, plus the query parameter that carries usernames a path can't, like `..` */
export const byUser = (username: string) => ({
  path: { username },
  query: { username }
});
