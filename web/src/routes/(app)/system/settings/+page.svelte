<script lang="ts">
  import { page } from "$app/state";
  import SearchIcon from "@lucide/svelte/icons/search";
  import { onMount, tick } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { fly } from "svelte/transition";

  import { api } from "#lib/api/client.js";
  import LoadError from "#lib/components/LoadError.svelte";
  import Overview from "#lib/components/settings/Overview.svelte";
  import RawFile from "#lib/components/settings/RawFile.svelte";
  import Section from "#lib/components/settings/Section.svelte";
  import Crumbs from "#lib/components/shell/Crumbs.svelte";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Input from "#lib/components/ui/Input.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { leftovers } from "#lib/config/fields.js";
  import { currentSection, fieldMatches } from "#lib/config/find.js";
  import { sections } from "#lib/config/sections.js";
  import { plural } from "#lib/format.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { motionFast } from "#lib/motion.js";
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
  const query = $derived(route(page.url).query);
  // ?tab=raw shows the configuration file as text
  const raw = $derived(query.get("tab") === "raw");
  // ?section=<id> scrolls to that section once the page has drawn it, as links from elsewhere ask
  const wanted = $derived(query.get("section"));
  $effect(() => {
    if (!wanted || !view.data || raw) return;
    requestAnimationFrame(() => document.getElementById(wanted)?.scrollIntoView());
  });

  // the search over every option starts from ?q=, which links from elsewhere use
  // svelte-ignore state_referenced_locally
  let typed = $state(query.get("q") ?? "");
  const q = $derived(typed.trim());
  /** how many options each section shows for the search */
  const hits = $derived(
    new Map(
      all.map((s) => [
        s.id,
        s.fields.filter((f) =>
          view.data && f.key in view.data.fields
            ? fieldMatches(f, view.data.fields[f.key], q)
            : false
        ).length
      ])
    )
  );
  const matched = $derived([...hits.values()].reduce((n, h) => n + h, 0));

  /** each section's unsaved changes, as it reports them */
  const changes = new SvelteMap<string, number>();
  const forms = $state<Record<string, Section>>({});
  const changedSections = $derived(all.filter((s) => (changes.get(s.id) ?? 0) > 0));
  const total = $derived(changedSections.reduce((n, s) => n + (changes.get(s.id) ?? 0), 0));
  const saveAll = async () => {
    for (const s of changedSections) await forms[s.id]?.save();
  };
  const canSaveAll = $derived(changedSections.every((s) => forms[s.id]?.allSaveable()));

  // the section in view, which the side list marks
  let current = $state<string | null>(null);
  /** the line under the sticky header that a section's top passes to become the current one */
  const LINE = 96;
  const spy = () => {
    const tops = all
      .map((s) => ({ id: s.id, el: document.getElementById(s.id) }))
      .filter((s): s is { id: string; el: HTMLElement } => !!s.el && !s.el.hidden)
      .map((s) => ({ id: s.id, top: s.el.getBoundingClientRect().top }));
    const end = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 4;
    current = currentSection(tops, LINE, end);
  };
  let frame = 0;
  const scrolled = () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(spy);
  };
  $effect(() => {
    void all;
    void q;
    if (!view.data || raw) return;
    void tick().then(spy);
  });

  const jump = (id: string) => {
    current = id;
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });
  };
</script>

<svelte:window onscroll={scrolled} onresize={scrolled} />

{#if raw}<Crumbs steps={[{ label: "Raw file" }]} />{/if}

{#if !admin}
  <Notice kind="info">Only administrators can view settings.</Notice>
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
    <div
      class="grid grid-cols-[minmax(0,1fr)] gap-section lg:grid-cols-[13rem_minmax(0,1fr)] lg:items-start">
      <!-- the categories, the one in view marked; on a wide screen they stay beside the page -->
      <aside class="flex flex-col gap-2 lg:sticky lg:top-20 lg:max-h-[calc(100svh-7rem)]">
        <Input
          type="search"
          icon={SearchIcon}
          placeholder="Search settings"
          aria-label="Search settings"
          bind:value={typed} />
        <nav
          aria-label="Settings sections"
          class="flex gap-1 overflow-x-auto max-lg:-mx-1 max-lg:px-1 max-lg:py-1 lg:min-h-0 lg:flex-col lg:overflow-y-auto">
          {#each all as section (section.id)}
            {@const n = hits.get(section.id) ?? 0}
            {@const pending = changes.get(section.id) ?? 0}
            <a
              href={href(`/system/settings?section=${section.id}`)}
              aria-current={current === section.id ? "location" : undefined}
              class={[
                "flex h-control-sm shrink-0 items-center gap-2 rounded-control px-2.5 text-sm whitespace-nowrap text-fg-muted transition-colors duration-(--motion-fast) hover:bg-hover hover:text-fg aria-[current]:nav-current",
                q && !n && "hidden"
              ]}
              onclick={(e) => {
                e.preventDefault();
                jump(section.id);
              }}>
              <span class="truncate">{section.title}</span>
              {#if pending}
                <span
                  class="ml-auto size-1.5 shrink-0 rounded-full bg-accent"
                  title={plural(pending, "unsaved change", "unsaved changes")}></span>
              {:else if q}
                <span class="ml-auto text-xs text-fg-subtle tabular-nums">{n}</span>
              {/if}
            </a>
          {/each}
        </nav>
      </aside>
      <div class="flex min-w-0 flex-col gap-section">
        {#if !q}<Overview view={data} onchanged={view.load} />{/if}
        {#if q && !matched}
          <EmptyState
            icon={SearchIcon}
            title="No settings match"
            description="The search looks in names, descriptions, environment variables and flags." />
        {/if}
        {#each all as section (section.id)}
          <Section
            bind:this={forms[section.id]}
            {section}
            view={data}
            onchanged={view.load}
            query={q}
            bind:changes={() => changes.get(section.id) ?? 0, (n) => changes.set(section.id, n)} />
        {/each}
      </div>
    </div>
    {#if total}
      <!-- every section's unsaved changes, pinned to the bottom of the window -->
      <div
        role="region"
        aria-label="Unsaved changes"
        class="sticky bottom-12 z-20 flex flex-wrap items-center gap-inline rounded-card border border-accent/40 bg-overlay px-4 py-2 shadow-raised"
        transition:fly={{ y: 16, duration: motionFast() }}>
        <span class="text-sm font-medium tabular-nums">
          {plural(total, "unsaved change", "unsaved changes")}
        </span>
        <span class="flex flex-wrap gap-1 text-sm text-fg-muted">
          in
          {#each changedSections as s, i (s.id)}
            <button type="button" class="text-accent hover:underline" onclick={() => jump(s.id)}
              >{s.title}</button
            >{i < changedSections.length - 1 ? "," : ""}
          {/each}
        </span>
        <div class="ml-auto flex gap-inline">
          <Button
            variant="ghost"
            onclick={() => {
              for (const s of changedSections) forms[s.id]?.reset();
            }}>Discard all</Button>
          {#if canSaveAll}
            <Button onclick={() => void saveAll()}>Save all</Button>
          {:else}
            <Button variant="outline" onclick={() => jump(changedSections[0].id)}>Review</Button>
          {/if}
        </div>
      </div>
    {/if}
  {/if}
{:else}
  <LoadError loader={view} />
{/if}
