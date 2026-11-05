<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import Crumbs from "#lib/components/shell/Crumbs.svelte";
  import BlockedList from "#lib/components/users/BlockedList.svelte";
  import BuddyList from "#lib/components/users/BuddyList.svelte";
  import Directory from "#lib/components/users/Directory.svelte";
  import { href, route, userHref } from "#lib/nav.js";
  import { BUDDY } from "#lib/wording.js";

  // /users is the directory; ?user=<name> goes to that user's profile, ?tag= filters by a tag,
  // ?has=transfers those we transferred with, ?tab=buddies shows the buddies and ?tab=blocked the
  // blocked users
  const query = $derived(route(page.url).query);
  $effect(() => {
    const user = query.get("user");
    if (user) void goto(userHref(user), { replace: true });
  });
  const TABS = [
    { id: "all", title: "All users", to: "/users" },
    { id: "buddies", title: BUDDY.plural, to: "/users?tab=buddies" },
    { id: "blocked", title: "Blocked users", to: "/users?tab=blocked" }
  ];
  const tab = $derived(TABS.find((t) => t.id === query.get("tab")) ?? TABS[0]);
</script>

{#if tab.id !== "all"}<Crumbs steps={[{ label: tab.title }]} />{/if}

<!-- the tabs wrap where a large text size leaves no room for all three on a phone -->
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
{:else}
  {#key `${query.get("tag")}|${query.get("has")}`}
    <Directory tag={query.get("tag")} withTransfers={query.get("has") === "transfers"} />
  {/key}
{/if}
