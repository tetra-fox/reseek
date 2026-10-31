<script lang="ts">
  import { nameParts, type UserFound } from "#lib/chat/search/match.js";
  import Avatar from "#lib/components/users/Avatar.svelte";
  import Presence from "#lib/components/users/Presence.svelte";
  import { nameColor } from "#lib/users/colors.js";
  import { users } from "#lib/users/users.svelte.js";

  import Marked from "./Marked.svelte";

  let { user, text }: { user: UserFound; text: string } = $props();

  const summary = $derived(user.summary ?? users.get(user.name));
</script>

<Avatar name={user.name} pictureUrl={summary?.info?.pictureUrl} class="size-8 text-xs" />
<span class="flex min-w-0 flex-1 flex-col">
  <span class="truncate text-sm font-medium" style:color={nameColor(user.name)}
    ><Marked parts={nameParts(user.name, text)} /></span>
  {#if summary?.presence}
    <span class="flex items-center gap-1.5 text-xs text-fg-muted">
      <Presence presence={summary.presence} />{summary.presence}
    </span>
  {/if}
</span>
