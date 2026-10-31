<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import { browseTarget, route } from "#lib/nav.js";

  // browsing lives on each user's Shares tab, and links to this address still land there
  $effect(() => {
    void goto(browseTarget(route(page.url).query), { replaceState: true });
  });
</script>
