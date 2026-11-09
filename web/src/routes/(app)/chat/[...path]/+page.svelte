<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { untrack } from "svelte";

  import { chatTarget, isChatPath, searchKind } from "#lib/chat/route.js";
  import ChatPage from "#lib/components/chat/ChatPage.svelte";
  import { chatHref, route } from "#lib/nav.js";

  // /chat, /chat/<user>, /chat/rooms/<room>, /chat/search?q=&in=, with ?at=<localId> on a pane
  // and ?q= on one opened from the search results; ?user=<name> opens that conversation
  // an address typed into the browser's bar reaches page.url while this screen is still up, and
  // the hash router reloads the app for it after, so only a chat address is followed
  let followed = untrack(() => route(page.url));
  const at = $derived.by(() => {
    const next = route(page.url);
    if (isChatPath(next.path)) followed = next;
    return followed;
  });
  const target = $derived(chatTarget(at.path));
  $effect(() => {
    const user = at.query.get("user");
    if (user) void goto(chatHref(user), { replace: true });
  });
  const message = $derived(Number(at.query.get("at")) || null);
</script>

<ChatPage {target} at={message} q={at.query.get("q") ?? ""} kind={searchKind(at.query.get("in"))} />
