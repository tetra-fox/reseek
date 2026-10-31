<script lang="ts">
  import HashIcon from "@lucide/svelte/icons/hash";

  import { nameParts, type RoomFound } from "#lib/chat/search/match.js";
  import { plural } from "#lib/format.js";

  import Marked from "./Marked.svelte";

  let { room, text }: { room: RoomFound; text: string } = $props();

  const about = $derived(
    [room.joined && "Joined", room.userCount !== null && plural(room.userCount, "user", "users")]
      .filter(Boolean)
      .join(", ")
  );
</script>

<span class="flex size-8 shrink-0 items-center justify-center rounded-full bg-surface-muted"
  ><HashIcon class="size-4 text-fg-muted" /></span>
<span class="flex min-w-0 flex-1 flex-col">
  <span class="truncate text-sm font-medium"><Marked parts={nameParts(room.name, text)} /></span>
  {#if about}<span class="truncate text-xs text-fg-muted">{about}</span>{/if}
</span>
