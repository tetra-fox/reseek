<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import { SERVER } from "#lib/chat/notices.js";
  import ProfilePage, { type ProfileTab } from "#lib/components/users/profile/ProfilePage.svelte";
  import { chatHref, route } from "#lib/nav.js";

  // /users/<name>[/shares|transfers|messages|activity], the Shares tab taking ?path=
  const TABS: ProfileTab[] = ["overview", "shares", "transfers", "messages", "activity"];
  const username = $derived(page.params.username ?? "");
  const tab = $derived(TABS.find((t) => t === page.params.tab) ?? "overview");
  const path = $derived(route(page.url).query.get("path"));

  // the name the server's notices come from has no profile, only its pane
  $effect(() => {
    if (username === SERVER) void goto(chatHref(SERVER), { replaceState: true });
  });
</script>

{#if username !== SERVER}
  {#key username}
    <ProfilePage {username} {tab} {path} />
  {/key}
{/if}
