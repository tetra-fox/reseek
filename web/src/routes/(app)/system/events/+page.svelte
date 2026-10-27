<script lang="ts">
  import { page } from "$app/state";

  import Events from "#lib/components/system/Events.svelte";
  import { route } from "#lib/nav.js";

  // /system/events?page=&per_page=&type=A,B
  const query = $derived(route(page.url).query);
  const current = $derived(Math.max(1, Number(query.get("page")) || 1));
  const perPage = $derived(
    [10, 25, 50, 100].find((n) => n === Number(query.get("per_page"))) ?? 25
  );
  const types = $derived((query.get("type") ?? "").split(",").filter(Boolean));
</script>

<Events page={current} {perPage} {types} />
