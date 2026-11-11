<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import Crumbs from "#lib/components/shell/Crumbs.svelte";
  import BlockedList from "#lib/components/users/BlockedList.svelte";
  import BuddyList from "#lib/components/users/BuddyList.svelte";
  import Directory from "#lib/components/users/Directory.svelte";
  import GroupsPage from "#lib/components/users/groups/GroupsPage.svelte";
  import InterestsPage from "#lib/components/users/interests/InterestsPage.svelte";
  import { href, route, userHref } from "#lib/nav.js";
  import { directoryAt, withDirectory } from "#lib/users/directory.js";
  import { BUDDY } from "#lib/wording.js";

  // /users is the directory, its filters and page in the address as `directoryAt` reads them;
  // ?user=<name> goes to that user's profile, ?tab=buddies shows the buddies, ?tab=groups the
  // groups, ?tab=interests the recommendations and similar users, for one interest with
  // ?interest=, and ?tab=blocked the blocked users
  const query = $derived(route(page.url).query);
  $effect(() => {
    const user = query.get("user");
    if (user) void goto(userHref(user), { replace: true });
  });
  const TABS = [
    { id: "all", title: "All users", to: "/users" },
    { id: "buddies", title: BUDDY.plural, to: "/users?tab=buddies" },
    { id: "groups", title: "Groups", to: "/users?tab=groups" },
    { id: "interests", title: "Interests", to: "/users?tab=interests" },
    { id: "blocked", title: "Blocked users", to: "/users?tab=blocked" }
  ];
  const tab = $derived(TABS.find((t) => t.id === query.get("tab")) ?? TABS[0]);
  const interest = $derived(query.get("interest")?.trim().toLowerCase() || null);
</script>

{#if tab.id !== "all"}
  <Crumbs
    steps={[
      { label: tab.title, href: href(tab.to) },
      ...(tab.id === "interests" && interest ? [{ label: interest }] : [])
    ]} />
{/if}

<!-- the tabs wrap where a large text size leaves no room for all of them on a phone -->
<nav aria-label="Users" class="flex flex-wrap gap-1">
  {#each TABS as t (t.id)}
    <a href={href(t.to)} aria-current={tab.id === t.id ? "page" : undefined} class="nav-tab">
      {t.title}
    </a>
  {/each}
</nav>

{#if tab.id === "blocked"}
  <BlockedList />
{:else if tab.id === "buddies"}
  <BuddyList />
{:else if tab.id === "groups"}
  <GroupsPage />
{:else if tab.id === "interests"}
  <InterestsPage {interest} />
{:else}
  <Directory
    address={directoryAt(query)}
    onaddress={(at) => {
      const next = withDirectory(query, at);
      void goto(href(`/users${next.size ? `?${next}` : ""}`), { replace: true, reset: false });
    }} />
{/if}
