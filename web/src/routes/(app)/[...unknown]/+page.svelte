<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import { app } from "#lib/app.svelte.js";
  import { href, route, unknownTarget } from "#lib/nav.js";

  // a path no screen renders: it goes to the screen it belongs to, or the dashboard. A relay
  // agent's own redirect, to System, is the layout's
  $effect(() => {
    if (app.mode === "agent") return;
    void goto(href(unknownTarget(route(page.url).path)), { replaceState: true });
  });
</script>
