<script lang="ts">
  import { page } from "$app/state";

  import Logs from "#lib/components/system/Logs.svelte";
  import { route } from "#lib/nav.js";

  const tab = $derived(route(page.url).query.get("tab") === "history" ? "history" : "live");
</script>

<Logs {tab} />
