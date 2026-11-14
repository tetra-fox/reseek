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

/** The error of a 403 for a session whose user has no role right now; the session goes on */
export const WITHHELD = "access_withheld";

/** Whether an error body is the one of a session whose user has no role right now */
export const isWithheld = (body: unknown): boolean =>
  !!body && typeof body === "object" && (body as { error?: unknown }).error === WITHHELD;

let unauthorized: () => void = () => {};
let withheld: () => void = () => {};
let roleChanged: () => Promise<void> = async () => {};

/** What to do when a request outside the session routes is answered 401: the session is gone */
export const onUnauthorized = (handler: () => void) => {
  unauthorized = handler;
};

/** What to do when a request outside the session routes finds the session without a role */
export const onWithheld = (handler: () => void) => {
  withheld = handler;
};

/** What to do when the event stream says the principal's role changed */
export const onRoleChanged = (handler: () => Promise<void>) => {
  roleChanged = handler;
};

/** Says the session is gone, for answers that don't go through `api`, like the event stream */
export const sessionGone = () => unauthorized();

/** Says the session has no role right now, for answers that don't go through `api` */
export const accessWithheld = () => withheld();

/** Says the principal's role changed; resolves once the session was read again */
export const principalChanged = () => roleChanged();

api.use({
  async onResponse({ request, response }) {
    if (response.status !== 401 && response.status !== 403) return;
    const path = new URL(request.url).pathname;
    if (SESSION_ROUTES.some((route) => path.endsWith(route))) return;
    if (response.status === 401) return sessionGone();
    // a 403 says why in its body, which the caller reads too, so a copy is read here
    const body: unknown = await response
      .clone()
      .json()
      .catch(() => undefined);
    if (isWithheld(body)) accessWithheld();
  }
});

/**
 * A path segment for `name`: the browser resolves a segment of dots alone, `.` or `..`, even
 * percent-encoded, so `/users/../profile` would become `/profile`. Such a name is written as `_`,
 * and the query parameter beside it carries the real one
 */
const pathSafe = (name: string) => (/^\.+$/.test(name) ? "_" : name);

/** A username in a path, plus the query parameter that carries usernames a path can't, like `..` */
export const byUser = (username: string) => ({
  path: { username: pathSafe(username) },
  query: { username }
});

/** A room's name in a path, plus the query parameter that carries names a path can't */
export const byRoom = (roomName: string) => ({
  path: { roomName: pathSafe(roomName) },
  query: { roomName }
});
