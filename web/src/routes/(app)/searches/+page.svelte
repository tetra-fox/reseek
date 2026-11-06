<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import SearchBar from "#lib/components/search/SearchBar.svelte";
  import SearchList from "#lib/components/search/SearchList.svelte";
  import Unreachable from "#lib/components/search/Unreachable.svelte";
  import Wishlist from "#lib/components/search/Wishlist.svelte";
  import { href, route } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";

  // a link can open the page with a user to search and the text to search for
  const query = $derived(route(page.url).query);
  const user = $derived(query.get("user"));
</script>

{#if session.can("ReadWrite")}
  {#key query.toString()}
    <SearchBar
      text={query.get("text") ?? ""}
      history
      scope={user ? { type: "user", subjects: [user] } : undefined}
      onclearscope={() => void goto(href("/searches"), { replaceState: true })} />
  {/key}
{/if}
<Unreachable />
<SearchList />
<Wishlist />
