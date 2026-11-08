<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import { formatCount } from "#lib/format.js";
  import type { UserLine } from "#lib/users/recommendations.svelte.js";

  // users beside the recommendations, each name opening the user's card; those alike to us show
  // how alike the server rates them
  let { users }: { users: UserLine[] } = $props();
</script>

<!-- a block list: in a flex column of capped height, the lines would shrink below their text -->
<ul class="max-h-[32rem] overflow-y-auto" aria-label="Users">
  {#each users as { username, rating } (username)}
    <li class="flex min-h-control-sm items-center gap-inline border-b py-1 last:border-b-0">
      <span class="min-w-0 flex-1 truncate"><Username name={username} card /></span>
      {#if rating !== null}
        <Tooltip text="Similarity" dense spoken>
          {#snippet child({ props })}
            <span {...props} class="shrink-0 text-xs text-fg-muted tabular-nums"
              >{formatCount(rating)}</span>
          {/snippet}
        </Tooltip>
      {/if}
    </li>
  {/each}
</ul>
