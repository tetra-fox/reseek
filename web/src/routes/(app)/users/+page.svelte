<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";

  import Crumbs from "#lib/components/shell/Crumbs.svelte";
  import BlockedList from "#lib/components/users/BlockedList.svelte";
  import Directory from "#lib/components/users/Directory.svelte";
  import { href, route, userHref } from "#lib/nav.js";

  // /users is the directory; ?user=<name> goes to that user's profile, ?tag= filters by a tag,
  // ?tab=blocked shows the blocked users
  const query = $derived(route(page.url).query);
  $effect(() => {
    const user = query.get("user");
    if (user) void goto(userHref(user), { replace: true });
  });
  const tab = $derived(query.get("tab") === "blocked" ? "blocked" : "all");
</script>

{#if tab === "blocked"}<Crumbs steps={[{ label: "Blocked users" }]} />{/if}

<nav aria-label="Users" class="flex gap-1">
  {#each [{ id: "all", title: "All users", to: "/users" }, { id: "blocked", title: "Blocked users", to: "/users?tab=blocked" }] as t (t.id)}
    <a
      href={href(t.to)}
      aria-current={tab === t.id ? "page" : undefined}
      class="flex h-control items-center rounded-control px-3 text-sm text-fg-muted hover:bg-hover hover:text-fg aria-[current]:nav-current">
      {t.title}
    </a>
  {/each}
</nav>

{#if tab === "blocked"}
  <BlockedList />
{:else}
  {#key query.get("tag")}
    <Directory tag={query.get("tag")} />
  {/key}
{/if}
