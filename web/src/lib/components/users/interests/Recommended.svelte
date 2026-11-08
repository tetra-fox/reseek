<script lang="ts">
  import SearchIcon from "@lucide/svelte/icons/search";
  import ThumbsDownIcon from "@lucide/svelte/icons/thumbs-down";
  import ThumbsUpIcon from "@lucide/svelte/icons/thumbs-up";

  import Badge from "#lib/components/ui/Badge.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { formatCount } from "#lib/format.js";
  import { href } from "#lib/nav.js";
  import { session } from "#lib/session.svelte.js";
  import type { Interests, Side } from "#lib/users/interests.svelte.js";
  import type { Recommendations } from "#lib/users/recommendations.svelte.js";

  // the interests recommended and those recommended against, each named by a link that shows what
  // goes with it, with its score and the ways to act on it
  type Props = {
    lists: Recommendations;
    /** our own interests, so one already listed shows as such */
    ours: Interests;
    /** we may like and dislike from here */
    canMark: boolean;
    onmark: (side: Side, interest: string) => void;
  };
  let { lists, ours, canMark, onmark }: Props = $props();

  const search = $derived(session.can("ReadWrite"));
  const interestHref = (item: string) =>
    href(`/users?tab=interests&interest=${encodeURIComponent(item)}`);
  const searchHref = (item: string) => href(`/searches?text=${encodeURIComponent(item)}`);
</script>

{#snippet list(title: string, items: Recommendations["recommended"])}
  <section class="flex flex-col gap-1" aria-label={title}>
    <h4 class="text-xs font-medium text-fg-muted">{title}</h4>
    <!-- a block list: in a flex column of capped height, the lines would shrink below their text -->
    <ul class="max-h-[32rem] overflow-y-auto">
      {#each items as { interest, score } (interest)}
        {@const mine = ours.liked.includes(interest)
          ? "Liked"
          : ours.hated.includes(interest)
            ? "Disliked"
            : null}
        <li class="flex min-h-control-sm items-center gap-inline border-b py-1 last:border-b-0">
          <a
            class="min-w-0 flex-1 text-sm wrap-anywhere hover:underline"
            href={interestHref(interest)}>{interest}</a>
          <Tooltip text="Score" dense spoken>
            {#snippet child({ props })}
              <span {...props} class="w-14 shrink-0 text-right text-xs text-fg-muted tabular-nums"
                >{formatCount(score)}</span>
            {/snippet}
          </Tooltip>
          <span class="flex w-28 shrink-0 items-center justify-end gap-0.5">
            {#if mine}
              <Badge tone="outline">{mine}</Badge>
            {:else if canMark}
              <Tooltip text="Like">
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Like ${interest}`}
                    onclick={() => onmark("liked", interest)}><ThumbsUpIcon /></Button>
                {/snippet}
              </Tooltip>
              <Tooltip text="Dislike">
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Dislike ${interest}`}
                    onclick={() => onmark("hated", interest)}><ThumbsDownIcon /></Button>
                {/snippet}
              </Tooltip>
            {/if}
            {#if search}
              <Tooltip text="Search for it">
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon-sm"
                    href={searchHref(interest)}
                    aria-label={`Search for ${interest}`}><SearchIcon /></Button>
                {/snippet}
              </Tooltip>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  </section>
{/snippet}

{#if lists.recommended.length}{@render list("Recommended", lists.recommended)}{/if}
{#if lists.unrecommended.length}{@render list("Recommended against", lists.unrecommended)}{/if}
