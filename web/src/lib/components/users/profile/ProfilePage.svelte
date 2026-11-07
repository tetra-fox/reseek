<script lang="ts">
  import type { Schemas } from "#lib/api/client.js";
  import BrowsePage from "#lib/components/browse/BrowsePage.svelte";
  import Pane from "#lib/components/chat/Pane.svelte";
  import ErrorSegment from "#lib/components/ErrorSegment.svelte";
  import Crumbs from "#lib/components/shell/Crumbs.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { keepCurrent } from "#lib/live/keep.svelte.js";
  import { live } from "#lib/live/live.svelte.js";
  import { userHref } from "#lib/nav.js";
  import { profileOf } from "#lib/users/actions.js";

  import ProfileActivity from "./ProfileActivity.svelte";
  import ProfileHeader from "./ProfileHeader.svelte";
  import ProfileOverview from "./ProfileOverview.svelte";
  import ProfileTransfers from "./ProfileTransfers.svelte";

  export type ProfileTab = "overview" | "shares" | "transfers" | "messages" | "activity";

  type Props = {
    username: string;
    tab: ProfileTab;
    /** the remote folder the Shares tab opens */
    path: string | null;
  };

  let { username, tab, path }: Props = $props();

  /** how old the stored statistics and info may be before opening the page fetches them again */
  const STALE_MS = 10 * 60_000;

  let profile = $state.raw<Schemas["UserProfile"] | null>(null);
  let error = $state("");
  let refreshing = $state(false);

  const fetch = async (refresh: boolean) => {
    const outcome = await profileOf(username, refresh);
    if (outcome.ok) {
      profile = outcome.data;
      error = "";
    } else error = outcome.error;
    return outcome.ok ? outcome.data : null;
  };
  const refresh = async () => {
    refreshing = true;
    await fetch(true);
    refreshing = false;
  };
  const old = (at: string | null | undefined) => !at || Date.now() - Date.parse(at) > STALE_MS;

  // what is stored shows first; the network is asked when that is missing or old
  void fetch(false).then((p) => {
    if (p && (old(p.statistics?.fetchedAt) || old(p.info?.fetchedAt))) void refresh();
  });
  keepCurrent(
    () => void fetch(false),
    () => "users"
  );
  $effect(() =>
    live.on("users.updated", (u) => {
      if (u.username === username) void fetch(false);
    })
  );

  const TABS: { id: ProfileTab; title: string }[] = [
    { id: "overview", title: "Overview" },
    { id: "shares", title: "Shares" },
    { id: "transfers", title: "Transfers" },
    { id: "messages", title: "Messages" },
    { id: "activity", title: "Activity" }
  ];
</script>

<!-- the Shares tab has its own steps, which name the folders -->
{#if tab !== "shares"}
  <Crumbs
    steps={[
      { label: username, href: userHref(username) },
      ...(tab === "overview" ? [] : [{ label: TABS.find((t) => t.id === tab)?.title ?? tab }])
    ]} />
{/if}

{#if profile}
  <ProfileHeader
    {profile}
    {refreshing}
    onrefresh={() => void refresh()}
    onchanged={() => void fetch(false)} />
  <nav aria-label="Profile" class="flex gap-1 overflow-x-auto">
    {#each TABS as t (t.id)}
      <a
        href={userHref(username, t.id === "overview" ? undefined : t.id)}
        aria-current={tab === t.id ? "page" : undefined}
        class="nav-tab">
        {t.title}
      </a>
    {/each}
  </nav>
  {#if tab === "shares"}
    <BrowsePage user={username} {path} />
  {:else if tab === "transfers"}
    <ProfileTransfers {username} />
  {:else if tab === "messages"}
    <div
      class="flex h-[calc(var(--content-h)-16rem)] min-h-80 overflow-hidden rounded-card border bg-surface">
      <Pane target={{ kind: "user", name: username }} />
    </div>
  {:else if tab === "activity"}
    <ProfileActivity {username} />
  {:else}
    <ProfileOverview {profile} />
  {/if}
{:else if error}
  <ErrorSegment caption={error} retry={() => void fetch(false)} retryLabel="Retry" />
{:else}
  <Skeleton class="h-40" />
  <Skeleton class="h-64" />
{/if}
