<script lang="ts">
  import { page } from "$app/state";

  import SearchDetail from "#lib/components/search/SearchDetail.svelte";

  const id = $derived(page.params.id ?? "");
</script>

<!-- a new id, as after Search Again, starts the page over -->
{#key id}
  <SearchDetail {id} />
{/key}
