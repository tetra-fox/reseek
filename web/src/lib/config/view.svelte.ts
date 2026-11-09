import { untrack } from "svelte";

import { api } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { Loader } from "#lib/loader.svelte.js";
import { call } from "#lib/request.js";
import { session } from "#lib/session.svelte.js";

/**
 * The daemon's view of the options for a page that edits a few of them, read while the session
 * is an administrator's, the only role that may, and again on each change of the options. Call
 * it while the component is being set up
 */
export const adminView = () => {
  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  const admin = $derived(session.can("Administrator"));
  $effect(() => {
    if (admin) untrack(() => void view.load());
  });
  $effect(() =>
    live.on("application.options", () => {
      if (admin) void view.load();
    })
  );
  return view;
};
