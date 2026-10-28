<script lang="ts">
  import { page } from "$app/state";

  import TransfersPage from "#lib/components/transfers/TransfersPage.svelte";
  import { route } from "#lib/nav.js";

  // ?state=<filter> opens the list filtered, as the dashboard's figures link to it
  const filter = $derived(route(page.url).query.get("state"));
</script>

<TransfersPage direction="download" {filter} />
