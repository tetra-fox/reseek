<script lang="ts">
  import { page } from "$app/state";
  import { onMount } from "svelte";

  import { api } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Overview from "#lib/components/settings/Overview.svelte";
  import RawFile from "#lib/components/settings/RawFile.svelte";
  import Section from "#lib/components/settings/Section.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { leftovers } from "#lib/config/fields.js";
  import { sections } from "#lib/config/sections.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href, route } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";

  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  const admin = $derived(session.can("Administrator"));

  onMount(() => {
    if (admin) void view.load();
  });
  // a change from anywhere (another tab, the file, an expiry) arrives as new options
  $effect(() => live.on("application.options", () => void view.load()));

  const all = $derived(view.data ? [...sections, leftovers(view.data)] : []);
  // ?tab=raw shows the configuration file as text
  const raw = $derived(route(page.url).query.get("tab") === "raw");
  // ?section=<id> scrolls to that section once the page has drawn it, as links from elsewhere ask
  const wanted = $derived(route(page.url).query.get("section"));
  $effect(() => {
    if (!wanted || !view.data || raw) return;
    requestAnimationFrame(() => document.getElementById(wanted)?.scrollIntoView());
  });
</script>

{#if !admin}
  <Notice kind="info">The settings are for administrators.</Notice>
{:else if view.loading}
  <Skeleton class="h-40" />
  <Skeleton class="h-96" />
{:else if view.data}
  {@const data = view.data}
  <nav aria-label="Settings views" class="flex gap-1">
    {#each [{ id: false, title: "Settings", to: "/system/settings" }, { id: true, title: "Raw file", to: "/system/settings?tab=raw" }] as t (t.title)}
      <a
        href={href(t.to)}
        aria-current={raw === t.id ? "page" : undefined}
        class="flex h-control items-center rounded-control px-3 text-sm text-fg-muted hover:bg-hover hover:text-fg aria-[current]:nav-current"
        >{t.title}</a>
    {/each}
  </nav>
  <LoadError loader={view} />
  {#if raw}
    <RawFile view={data} />
  {:else}
    <nav aria-label="Settings sections" class="flex flex-wrap gap-x-3 gap-y-1 text-sm">
      {#each all as section (section.id)}
        <a
          href="#/system/settings"
          class="text-fg-muted hover:text-fg"
          onclick={(e) => {
            e.preventDefault();
            document.getElementById(section.id)?.scrollIntoView({ behavior: "smooth" });
          }}>{section.title}</a>
      {/each}
    </nav>
    <Overview view={data} onchanged={view.load} />
    {#each all as section (section.id)}
      <Section {section} view={data} onchanged={view.load} />
    {/each}
  {/if}
{:else}
  <LoadError loader={view} />
{/if}
