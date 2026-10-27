<script lang="ts">
  import { page } from "$app/state";

  import Files from "#lib/components/system/Files.svelte";
  import { route } from "#lib/nav.js";

  // /system/files?root=downloads|incomplete&path=<folder under it>
  const query = $derived(route(page.url).query);
  const root = $derived(query.get("root") === "incomplete" ? "incomplete" : "downloads");
</script>

<Files {root} path={query.get("path") ?? ""} />
