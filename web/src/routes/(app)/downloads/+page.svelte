<script lang="ts">
  import { page } from "$app/state";

  import TransfersPage from "#lib/components/transfers/TransfersPage.svelte";
  import { route } from "#lib/nav.js";

  // ?state=<filter> opens the list filtered, as the dashboard's figures link to it; ?job=<id>
  // shows one download job's files, and ?transfer=<id> opens the inspector on one
  const query = $derived(route(page.url).query);
  const filter = $derived(query.get("state"));
  const job = $derived(query.get("job"));
  const transfer = $derived(query.get("transfer"));
</script>

<TransfersPage direction="download" {filter} {job} {transfer} />
