<script lang="ts">
  import { page } from "$app/state";

  import ProfilePage, { type ProfileTab } from "#lib/components/users/ProfilePage.svelte";
  import { route } from "#lib/nav.js";

  // /users/<name>[/shares|transfers|messages|activity], the Shares tab taking ?path=
  const TABS: ProfileTab[] = ["overview", "shares", "transfers", "messages", "activity"];
  const username = $derived(page.params.username ?? "");
  const tab = $derived(TABS.find((t) => t === page.params.tab) ?? "overview");
  const path = $derived(route(page.url).query.get("path"));
</script>

{#key username}
  <ProfilePage {username} {tab} {path} />
{/key}
