import type { HandleClientError } from "@sveltejs/kit/hooks";

/**
 * What a screen's error boundary is handed: SvelteKit turns every error thrown while rendering
 * into an `App.Error`, whose message is "Internal Error" for an error of our own unless this says
 * otherwise. The boundary shows the error's own message, as the rest of the app does
 */
export const handleError: HandleClientError = ({ kind, error }) =>
  kind === "unknown" ? { message: error instanceof Error ? error.message : String(error) } : {};
