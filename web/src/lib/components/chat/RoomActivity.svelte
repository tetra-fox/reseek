<script lang="ts">
  import UsersRoundIcon from "@lucide/svelte/icons/users-round";

  import { type Burst, burstText } from "#lib/chat/presence.js";
  import type { RoomMembers } from "#lib/chat/room.svelte.js";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import { freezesWhileUsed } from "#lib/interaction.svelte.js";
  import { ListControl } from "#lib/list.svelte.js";

  import PresenceRow from "./PresenceRow.svelte";

  // members joining, leaving, going away and coming back since the room was opened, newest
  // first; a burst keeps its row as names join it
  let { members }: { members: RoomMembers } = $props();

  const shown = $derived(members.bursts.toReversed());
  const list = new ListControl<Burst>({
    lines: () => shown,
    key: (b) => b.key,
    id: (b) => b.key
  });
</script>

<section class="flex min-h-0 w-full flex-col p-2" aria-label="Member activity">
  <div
    class="min-h-0 flex-1 overflow-y-auto rounded-control text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
    role="grid"
    aria-label="Joins, leaves and status changes"
    aria-multiselectable="true"
    aria-activedescendant={list.activeId}
    tabindex="0"
    {@attach list.attach}
    {@attach freezesWhileUsed}>
    {#each shown as burst, i (burst.key)}
      <div
        role="row"
        {...list.line(burst, i)}
        aria-label={burstText(burst)}
        class={[
          "log-row",
          list.selected.has(burst.key) && "bg-accent-soft/60",
          list.isFocused(burst) && "ring-1 ring-accent ring-inset"
        ]}>
        <PresenceRow {burst} />
      </div>
    {:else}
      <div>
        <EmptyState icon={UsersRoundIcon} title="No member activity since you opened this room" />
      </div>
    {/each}
  </div>
</section>
