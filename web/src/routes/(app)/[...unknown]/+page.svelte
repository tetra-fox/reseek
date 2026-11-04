<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import { href, route, unknownTarget } from "#lib/nav.js";

  // a path no screen renders: it goes to the screen it belongs to, or the dashboard
  $effect(() => {
    void goto(href(unknownTarget(route(page.url).path)), { replaceState: true });
  });
</script>
