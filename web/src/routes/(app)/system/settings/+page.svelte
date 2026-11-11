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
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { currentSection, fieldMatches } from "#lib/config/find.js";
  import { sections } from "#lib/config/sections.js";
  import { plural } from "#lib/format.js";
  import { live } from "#lib/live/live.svelte.js";
  import { Loader } from "#lib/loader.svelte.js";
  import { href, route } from "#lib/nav.js";
  import { call } from "#lib/request.js";
  import { session } from "#lib/session.svelte.js";
  import { content } from "#lib/shell/content.svelte.js";
  import { motionFast } from "#lib/ui/motion.js";

  const view = new Loader(() => call(() => api.GET("/api/v0/options/view")));
  const admin = $derived(session.can("Administrator"));

  onMount(() => {
    if (admin) void view.load();
  });
  // a change from anywhere (another tab, the file, an expiry) arrives as new options
  $effect(() => live.on("application.options", () => void view.load()));

  const all = $derived(view.data ? sections : []);
  /** the sections of the side list by their headings, in the order they come on the page */
  const groups = $derived(
    [...new Set(all.map((s) => s.group))].map((group) => ({
      group,
      members: all.filter((s) => s.group === group)
    }))
  );
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
  /**
   * the line, measured down from the content area's top, that a section's top passes to become
   * the current one: a little below where a section scrolled to lands, past its scroll margin
   */
  const LINE = 40;
  const spy = () => {
    const box = content.scroller;
    if (!box) return;
    const origin = box.getBoundingClientRect().top;
    const tops = all
      .map((s) => ({ id: s.id, el: document.getElementById(s.id) }))
      .filter((s): s is { id: string; el: HTMLElement } => !!s.el && !s.el.hidden)
      .map((s) => ({ id: s.id, top: s.el.getBoundingClientRect().top - origin }));
    const end = box.scrollTop + box.clientHeight >= box.scrollHeight - 4;
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
  // the shell's content area scrolls, not the window
  $effect(() => {
    const box = content.scroller;
    if (!box) return;
    box.addEventListener("scroll", scrolled, { passive: true });
    return () => box.removeEventListener("scroll", scrolled);
  });

  const jump = (id: string) => {
    current = id;
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth" });
  };
</script>

<svelte:window onresize={scrolled} />

{#if raw}<Crumbs steps={[{ label: "Raw file" }]} />{/if}

{#if !admin}
  <Notice kind="info">Only administrators can view settings</Notice>
{:else if view.loading}
  <Skeleton class="h-40" />
  <Skeleton class="h-96" />
{:else if view.data}
  {@const data = view.data}
  <nav aria-label="Settings views" class="flex gap-1">
    {#each [{ id: false, title: "Settings", to: "/system/settings" }, { id: true, title: "Raw file", to: "/system/settings?tab=raw" }] as t (t.title)}
      <a href={href(t.to)} aria-current={raw === t.id ? "page" : undefined} class="nav-tab"
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
      <aside
        class="flex flex-col gap-2 lg:sticky lg:top-gutter lg:max-h-[calc(var(--content-h)-2*var(--spacing-gutter))] lg:gap-4">
        <Input
          type="search"
          icon={SearchIcon}
          placeholder="Search settings"
          aria-label="Search settings"
          bind:value={typed} />
        <nav
          aria-label="Settings sections"
          class="sections flex gap-1 overflow-x-auto lg:min-h-0 lg:flex-col lg:gap-5 lg:overflow-y-auto">
          {#each groups as { group, members } (group)}
            <!-- one line of tabs on a phone, where the headings have no place -->
            <div role="group" aria-label={group} class="contents lg:flex lg:flex-col lg:gap-0.5">
              <p
                aria-hidden="true"
                class={[
                  "px-2.5 pb-1 text-xs font-medium text-fg-muted max-lg:hidden",
                  q && !members.some((s) => hits.get(s.id)) && "hidden"
                ]}>
                {group}
              </p>
              {#each members as section (section.id)}
                {@const n = hits.get(section.id) ?? 0}
                {@const pending = changes.get(section.id) ?? 0}
                <a
                  href={href(`/system/settings?section=${section.id}`)}
                  aria-current={current === section.id ? "location" : undefined}
                  class={["nav-tab h-control-sm gap-2 px-2.5", q && !n && "hidden"]}
                  onclick={(e) => {
                    e.preventDefault();
                    jump(section.id);
                  }}>
                  <span class="truncate">{section.title}</span>
                  {#if pending}
                    <Tooltip
                      text={plural(pending, "unsaved change", "unsaved changes")}
                      dense
                      spoken>
                      {#snippet child({ props })}
                        <span {...props} class="ml-auto size-1.5 shrink-0 rounded-full bg-accent"
                        ></span>
                      {/snippet}
                    </Tooltip>
                  {:else if q}
                    <span class="ml-auto text-xs text-fg-muted tabular-nums">{n}</span>
                  {/if}
                </a>
              {/each}
            </div>
          {/each}
        </nav>
      </aside>
      <div class="flex min-w-0 flex-col gap-section">
        {#if !q}<Overview view={data} onchanged={view.load} />{/if}
        {#if q && !matched}
          <EmptyState
            icon={SearchIcon}
            title="No settings match"
            description="The search covers names, descriptions, environment variables and flags" />
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
        class="sticky bottom-2 z-20 flex flex-wrap items-center gap-inline rounded-card border border-accent/40 bg-overlay px-4 py-2 shadow-raised"
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

<style>
  /* below lg the sections are one line of tabs that scrolls sideways: an edge with more tabs past
     it fades out. Where the browser can't tie an animation to scrolling, the edges stay sharp */
  @property --fade-start {
    syntax: "<length>";
    inherits: false;
    initial-value: 0px;
  }
  @property --fade-end {
    syntax: "<length>";
    inherits: false;
    initial-value: 0px;
  }
  @keyframes fade-edges {
    0% {
      --fade-start: 0px;
      --fade-end: 2rem;
    }
    10% {
      --fade-start: 2rem;
    }
    90% {
      --fade-end: 2rem;
    }
    100% {
      --fade-start: 2rem;
      --fade-end: 0px;
    }
  }
  @supports (animation-timeline: scroll()) {
    .sections {
      mask-image: linear-gradient(
        to right,
        transparent,
        black var(--fade-start),
        black calc(100% - var(--fade-end)),
        transparent
      );
      animation: fade-edges linear both;
      animation-timeline: scroll(self inline);
    }
  }
</style>
