<script lang="ts">
  import LoadError from "#lib/components/LoadError.svelte";
  import Badge from "#lib/components/ui/Badge.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { interestsOf } from "#lib/users/recommendations.svelte.js";

  // what a user likes and dislikes, as the server has it, each a link to what goes with it
  let { username }: { username: string } = $props();

  const interests = new Loader(() => interestsOf(username));
  void interests.load();

  const lists = $derived([
    { title: "Likes", list: interests.data?.liked ?? [] },
    { title: "Dislikes", list: interests.data?.hated ?? [] }
  ]);
  const interestHref = (item: string) =>
    href(`/users?tab=interests&interest=${encodeURIComponent(item)}`);
</script>

<Card title="Interests">
  <LoadError loader={interests} />
  {#if interests.loading}
    <Skeleton class="h-12" />
  {:else if interests.data && !lists.some(({ list }) => list.length)}
    <p class="text-sm text-fg-muted">No interests listed</p>
  {:else if interests.data}
    <dl class="flex flex-col gap-2">
      {#each lists as { title, list } (title)}
        {#if list.length}
          <div class="flex flex-col gap-1">
            <dt class="text-xs font-medium text-fg-muted">{title}</dt>
            <dd class="flex flex-wrap gap-1">
              {#each list as item (item)}
                <a href={interestHref(item)} class="inline-flex max-w-full">
                  <Badge tone="outline" class="max-w-full whitespace-normal hover:text-fg"
                    ><span class="wrap-anywhere">{item}</span></Badge>
                </a>
              {/each}
            </dd>
          </div>
        {/if}
      {/each}
    </dl>
  {/if}
</Card>
