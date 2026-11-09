<script lang="ts">
  import { page } from "$app/state";

  import Events, { EVENT_PAGES } from "#lib/components/system/Events.svelte";
  import { placeIn } from "#lib/lists/pages.js";
  import { route } from "#lib/nav.js";

  const query = $derived(route(page.url).query);
  const place = $derived(placeIn(query, EVENT_PAGES));
  const types = $derived((query.get("type") ?? "").split(",").filter(Boolean));
</script>

<Events {place} {types} />
