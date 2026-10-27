<script lang="ts">
  import { page } from "$app/state";

  import BrowsePage from "#lib/components/browse/BrowsePage.svelte";
  import { route } from "#lib/nav.js";

  // /browse?user=<name>&path=<remote folder>&flat=1, every part optional
  const query = $derived(route(page.url).query);
</script>

<BrowsePage user={query.get("user")} path={query.get("path")} flat={query.get("flat") === "1"} />
