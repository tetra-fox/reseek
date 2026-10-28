import type { HandleClientError } from "@sveltejs/kit/hooks";

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
