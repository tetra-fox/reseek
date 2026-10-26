<script lang="ts">
  import { onMount } from "svelte";

  import { api } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Overview from "#lib/components/settings/Overview.svelte";
  import Section from "#lib/components/settings/Section.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { leftovers } from "#lib/config/fields.js";
  import { sections } from "#lib/config/sections.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
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
</script>

{#if !admin}
  <Notice kind="info">The settings are for administrators.</Notice>
{:else if view.loading}
  <Skeleton class="h-40" />
  <Skeleton class="h-96" />
{:else if view.data}
  {@const data = view.data}
  <LoadError loader={view} />
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
{:else}
  <LoadError loader={view} />
{/if}
