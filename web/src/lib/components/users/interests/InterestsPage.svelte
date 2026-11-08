<script lang="ts">
  import ArrowLeftIcon from "@lucide/svelte/icons/arrow-left";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import SparklesIcon from "@lucide/svelte/icons/sparkles";
  import UsersIcon from "@lucide/svelte/icons/users";
  import { untrack } from "svelte";

  import { api } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import Card from "#lib/components/ui/Card.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Segmented from "#lib/components/ui/Segmented.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import InterestsEditor from "#lib/components/users/profile/InterestsEditor.svelte";
  import { valueAt } from "#lib/config/sections.js";
  import { plural } from "#lib/format.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import {
    INTERESTS_HINT,
    OwnInterests,
    interestsEditability
  } from "#lib/users/interests.svelte.js";
  import { ownProfile } from "#lib/users/profile.svelte.js";
  import { InterestFinder } from "#lib/users/recommendations.svelte.js";

  import Recommended from "./Recommended.svelte";
  import SimilarUsers from "./SimilarUsers.svelte";

  // what the server makes of interests: recommendations from ours or the network's, users alike
  // to us, and for one interest picked from a list, what goes with it and who likes it
  type Props = { interest: string | null };
  let { interest }: Props = $props();

  const admin = $derived(session.can("Administrator"));
  // the lists are options, so whether they can be changed from here is the configuration's
  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  $effect(() => {
    if (admin) untrack(() => void view.load());
  });
  const edit = $derived(view.data ? interestsEditability(view.data) : null);
  const canMark = $derived(admin && !!edit && edit.mode !== "locked");
  // an option's editability has no notice while remote configuration is off, so this gives one
  const notice = $derived(
    view.data && valueAt(view.data.values, "remote_configuration") !== true
      ? "Read-only because remote configuration is off"
      : (edit?.notice ?? null)
  );
  const own = new OwnInterests();
  const finder = new InterestFinder();

  let source = $state<"personal" | "popular">("personal");
  $effect(() => {
    const picked = interest;
    const from = source;
    untrack(() => finder.show(picked ? { kind: "related", of: picked } : { kind: from }));
  });

  const read = $derived(finder.recommendations.data);
  const title = $derived(
    interest
      ? `Recommendations for ${interest}`
      : source === "popular" || read?.popularInstead
        ? "Popular interests"
        : "Recommended for you"
  );
  const note = $derived(
    !interest && source === "personal" && read?.popularInstead
      ? "Nothing is recommended for you yet, so these are popular interests"
      : undefined
  );

  /** Liking or disliking from a list changes what is recommended, so ours are read again */
  const mark = async (side: "liked" | "hated", item: string) => {
    own.add(side, item);
    await own.settled;
    if (!interest && source === "personal") finder.show({ kind: "personal" });
  };
  const refresh = () => finder.show(finder.shown);
</script>

<Card title="Your interests" description={INTERESTS_HINT}>
  <LoadError loader={ownProfile.loader} />
  <InterestsEditor interests={own} disabled={!canMark} {notice} />
</Card>

<div class="grid grid-cols-[minmax(0,1fr)] items-start gap-section lg:grid-cols-2">
  <Card {title} description={note}>
    {#snippet action()}
      <div class="flex flex-wrap items-center gap-inline">
        {#if interest}
          <Button variant="outline" size="sm" href={href("/users?tab=interests")}
            ><ArrowLeftIcon />Your recommendations</Button>
        {:else}
          <Segmented
            size="sm"
            label="Recommendations to show"
            options={[
              { id: "personal", label: "For you" },
              { id: "popular", label: "Popular" }
            ]}
            bind:value={source} />
        {/if}
        <Tooltip text="Refresh">
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" aria-label="Refresh" onclick={refresh}
              ><RefreshCwIcon /></Button>
          {/snippet}
        </Tooltip>
      </div>
    {/snippet}
    <LoadError loader={finder.recommendations} />
    {#if finder.recommendations.loading}
      <Skeleton class="h-48" />
    {:else if read && (read.lists.recommended.length || read.lists.unrecommended.length)}
      <Recommended lists={read.lists} ours={own.lists} {canMark} onmark={mark} />
    {:else if read}
      <EmptyState icon={SparklesIcon} title="Nothing to recommend" />
    {/if}
  </Card>

  <Card title={interest ? `Users who like ${interest}` : "Users like you"}>
    {#snippet action()}
      <span class="text-xs text-fg-muted">
        {finder.users.data ? plural(finder.users.data.length, "user", "users") : ""}
      </span>
    {/snippet}
    <LoadError loader={finder.users} />
    {#if finder.users.loading}
      <Skeleton class="h-48" />
    {:else if finder.users.data?.length}
      <SimilarUsers users={finder.users.data} />
    {:else if finder.users.data}
      <EmptyState
        icon={UsersIcon}
        title="No users found"
        description={interest ? undefined : "Add interests to find users who share them"} />
    {/if}
  </Card>
</div>
