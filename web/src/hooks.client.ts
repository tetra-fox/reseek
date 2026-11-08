import type { ClientInit, HandleClientError } from "@sveltejs/kit/hooks";

/**
 * What a screen's error boundary is handed: SvelteKit turns every error thrown while rendering
 * into an `App.Error`, whose message is "Internal Error" for an error of our own unless this says
 * otherwise. The boundary shows the error's own message, as the rest of the app does
 */
export const handleError: HandleClientError = ({ kind, error }) => {
  if (kind !== "unknown") return {};
  // what SvelteKit's own hook does, which this one replaces: the stack stays in the console,
  // the only log the browser side has
  // oxlint-disable-next-line no-console
  console.error(error);
  return { message: error instanceof Error ? error.message : String(error) };
};

/**
 * Runs before the router reads the address. SvelteKit 3's hash router passes the whole hash
 * through `decodeURIComponent` once on start (for Safari, which writes a typed `#` as `%23`), so
 * a query value holding an encoded `&`, `=` or `+` would split or change on a reload or an opened
 * link. Doubling every `%` makes that decode give back the hash as it was written.
 * TODO: remove when the router splits the hash before decoding its parts
 */
export const init: ClientInit = () => {
  if (location.hash.includes("%")) {
    const url = new URL(location.href);
    url.hash = url.hash.replaceAll("%", "%25");
    history.replaceState(history.state, "", url);
  }
};
