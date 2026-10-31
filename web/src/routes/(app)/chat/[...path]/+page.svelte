<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import { chatTarget, searchKind } from "#lib/chat/route.js";
  import ChatPage from "#lib/components/chat/ChatPage.svelte";
  import { chatHref, route } from "#lib/nav.js";

  // /chat, /chat/<user>, /chat/rooms/<room>, /chat/search?q=&in=, with ?at=<localId> on a pane
  // and ?q= on one opened from the search results; ?user=<name> opens that conversation
  const at = $derived(route(page.url));
  const target = $derived(chatTarget(at.path));
  $effect(() => {
    const user = at.query.get("user");
    if (user) void goto(chatHref(user), { replaceState: true });
  });
  const message = $derived(Number(at.query.get("at")) || null);
</script>

<ChatPage {target} at={message} q={at.query.get("q") ?? ""} kind={searchKind(at.query.get("in"))} />
