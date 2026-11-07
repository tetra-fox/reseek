<script lang="ts">
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import Username from "#lib/components/users/Username.svelte";
  import type { UserLine } from "#lib/users/recommendations.svelte.js";

  // users beside the recommendations, each name opening the user's card; those alike to us show
  // how alike the server rates them
  let { users }: { users: UserLine[] } = $props();
</script>

<ul class="flex max-h-[32rem] flex-col overflow-y-auto" aria-label="Users">
  {#each users as { username, rating } (username)}
    <li class="flex min-h-control-sm items-center gap-inline border-b py-1 last:border-b-0">
      <span class="min-w-0 flex-1 truncate"><Username name={username} card /></span>
      {#if rating !== null}
        <Tooltip text="Similarity" dense spoken>
          {#snippet child({ props })}
            <span {...props} class="shrink-0 text-xs text-fg-muted tabular-nums"
              >{rating.toLocaleString()}</span>
          {/snippet}
        </Tooltip>
      {/if}
    </li>
  {/each}
</ul>
