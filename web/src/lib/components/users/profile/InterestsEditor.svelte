<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import XIcon from "@lucide/svelte/icons/x";

  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import type { OwnInterests, Side } from "#lib/users/interests.svelte.js";

  // what we like and dislike, two lists that save as they change. Adding something to one list
  // takes it off the other
  type Props = { interests: OwnInterests; disabled: boolean };
  let { interests, disabled }: Props = $props();

  const typed = $state<Record<Side, string>>({ liked: "", hated: "" });
  const lists = $derived(interests.lists);

  const add = (e: SubmitEvent, side: Side) => {
    e.preventDefault();
    interests.add(side, typed[side]);
    typed[side] = "";
  };
</script>

{#snippet list(side: Side, title: string, adding: string)}
  <div class="flex min-w-0 flex-col gap-2">
    <h4 class="text-xs font-medium text-fg-muted">{title}</h4>
    {#if lists[side].length}
      <ul class="flex flex-wrap gap-1" aria-label={title}>
        {#each lists[side] as interest (interest)}
          <li class="min-w-0">
            <Badge tone="outline" class="max-w-full whitespace-normal">
              <span class="wrap-anywhere">{interest}</span>
              {#if !disabled}
                <button
                  type="button"
                  aria-label={`Remove ${interest}`}
                  onclick={() => interests.remove(side, interest)}>
                  <XIcon class="size-3" />
                </button>
              {/if}
            </Badge>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="text-sm text-fg-muted">Nothing listed.</p>
    {/if}
    {#if !disabled}
      <form class="flex gap-inline" onsubmit={(e) => add(e, side)}>
        <Input
          class="h-control-sm"
          placeholder={adding}
          aria-label={adding}
          maxlength={100}
          bind:value={typed[side]} />
        <Button type="submit" variant="outline" size="sm" disabled={!typed[side].trim()}>
          <PlusIcon />Add
        </Button>
      </form>
    {/if}
  </div>
{/snippet}

<LoadError loader={interests.loader} />
<div class="grid grid-cols-[minmax(0,1fr)] gap-group sm:grid-cols-2">
  {@render list("liked", "Interests", "Add an interest")}
  {@render list("hated", "Dislikes", "Add a dislike")}
</div>
